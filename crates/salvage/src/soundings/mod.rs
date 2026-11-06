//! `salvage soundings`: reads Unreal memory captures (memreport files and LLM
//! CSVs), shows where the memory goes, and measures a run against the memory
//! of smaller machines.

mod diff;
mod llm;
mod memreport;
mod render;

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::units::{GIB, MIB};

pub use diff::{Change, Diff, diff};
pub use render::{render_diff, render_diff_json, render_json, render_report};

/// Machine sizes a run is measured against unless `--target` says otherwise.
pub const DEFAULT_TARGETS_GB: [u64; 3] = [8, 12, 16];
/// Share of a machine's memory left for the OS and everything else.
pub const DEFAULT_HEADROOM_PERCENT: u64 = 25;
/// A pool this share of the draft or more is flagged as oversized...
pub const OVERSIZED_SHARE: f64 = 0.15;
/// ...as is any pool at least this big.
pub const OVERSIZED_BYTES: u64 = GIB;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    /// Output of the `memreport` console command.
    Memreport,
    /// `-llmcsv` output for the default tracker (`LLM_*.csv`).
    Llm,
    /// `-llmcsv` output for the platform tracker (`LLMPlatform_*.csv`).
    LlmPlatform,
}

impl Kind {
    pub fn describe(self) -> &'static str {
        match self {
            Kind::Memreport => "memreport",
            Kind::Llm => "LLM CSV",
            Kind::LlmPlatform => "LLM platform CSV",
        }
    }
}

/// Which view of memory a pool comes from. Pools in different views overlap
/// (a texture is an object, an RHI resource and a texture), so only pools in
/// the same view can be compared or added up.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum View {
    /// Headline totals from different memreport commands.
    Totals,
    /// GPU resources grouped by rendering system (`rhi.DumpResourceMemory summary`).
    Gpu,
    /// LLM tags.
    Llm,
    /// Object classes by resource size (`obj list -resourcesizesort`).
    Objects,
}

impl View {
    pub fn title(self) -> &'static str {
        match self {
            View::Totals => "Headline numbers",
            View::Gpu => "GPU resources by system",
            View::Llm => "LLM tags",
            View::Objects => "Biggest object classes",
        }
    }

    pub fn note(self) -> Option<&'static str> {
        match self {
            View::Totals => {
                Some("Different views of the same memory; they overlap, so don't add them up.")
            }
            View::Objects => Some("Object memory plus the resources each class owns."),
            _ => None,
        }
    }

    /// Whether pools in this view are worth flagging as oversized. Totals are
    /// flagged only when there's a setting that sizes them.
    fn flaggable(self) -> bool {
        matches!(self, View::Gpu | View::Llm)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Pool {
    pub view: View,
    pub name: String,
    pub bytes: u64,
    /// Highest value seen during the run (LLM CSVs only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peak: Option<u64>,
    /// Instance count, where the capture has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<u64>,
}

impl Pool {
    fn new(view: View, name: impl Into<String>, bytes: u64) -> Pool {
        Pool {
            view,
            name: name.into(),
            bytes,
            peak: None,
            count: None,
        }
    }
}

/// One capture, parsed.
#[derive(Clone, Debug, Serialize)]
pub struct Sounding {
    pub file: PathBuf,
    pub kind: Kind,
    /// Platform, config and device profile, where the capture names them.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<String>,
    /// Process memory at capture time (memreport) or at the last LLM frame.
    pub footprint: Option<u64>,
    /// Highest process memory seen.
    pub peak: Option<u64>,
    /// Physical memory of the machine the capture was taken on.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub machine_memory: Option<u64>,
    pub pools: Vec<Pool>,
}

impl Sounding {
    /// Reads a memreport or LLM CSV, telling them apart by content.
    pub fn read(path: &Path) -> Result<Sounding, String> {
        let bytes =
            std::fs::read(path).map_err(|e| format!("can't read {}: {e}", path.display()))?;
        let text = decode(&bytes);
        Sounding::parse(path, &text)
    }

    pub fn parse(path: &Path, text: &str) -> Result<Sounding, String> {
        if memreport::looks_like(text) {
            return memreport::parse(path, text);
        }
        if llm::looks_like(text) {
            return llm::parse(path, text);
        }
        Err(format!(
            "{} doesn't look like a memreport or an LLM CSV (see docs/benchmark-recipe.md for how to capture one)",
            path.display()
        ))
    }

    /// The number a run is judged by: its peak if known, else its footprint.
    pub fn draft(&self) -> Option<u64> {
        self.peak.or(self.footprint)
    }

    pub fn pools_in(&self, view: View) -> Vec<&Pool> {
        let mut pools: Vec<&Pool> = self.pools.iter().filter(|p| p.view == view).collect();
        pools.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.name.cmp(&b.name)));
        pools
    }

    pub fn views(&self) -> Vec<View> {
        let mut views: Vec<View> = self.pools.iter().map(|p| p.view).collect();
        views.sort();
        views.dedup();
        views
    }

    /// Pools big enough to be worth a look, biggest first.
    pub fn oversized(&self) -> Vec<Flag> {
        let draft = self.draft().unwrap_or(0) as f64;
        let mut flags: Vec<Flag> = self
            .pools
            .iter()
            .filter(|p| {
                p.view.flaggable() || (p.view == View::Totals && knob_for(&p.name).is_some())
            })
            .filter(|p| {
                p.bytes >= OVERSIZED_BYTES
                    || (draft > 0.0 && p.bytes as f64 >= draft * OVERSIZED_SHARE)
            })
            .map(|p| Flag {
                view: p.view,
                name: p.name.clone(),
                bytes: p.bytes,
                share: (draft > 0.0).then(|| p.bytes as f64 / draft),
                knob: knob_for(&p.name),
            })
            .collect();
        flags.sort_by_key(|f| std::cmp::Reverse(f.bytes));
        flags
    }

    /// Measures the draft against each machine size.
    pub fn targets(&self, machines_gb: &[u64], headroom_percent: u64) -> Vec<Target> {
        let Some(draft) = self.draft() else {
            return Vec::new();
        };
        machines_gb
            .iter()
            .map(|&gb| Target::measure(gb, headroom_percent, draft))
            .collect()
    }
}

/// A pool worth a look, with the setting most likely to shrink it.
#[derive(Clone, Debug, Serialize)]
pub struct Flag {
    pub view: View,
    pub name: String,
    pub bytes: u64,
    /// Share of the draft.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub share: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub knob: Option<Knob>,
}

/// A console variable or setting that controls a pool's size.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Knob {
    pub setting: &'static str,
    pub hint: &'static str,
}

/// The usual setting for a pool, matched on its name.
pub fn knob_for(name: &str) -> Option<Knob> {
    let name = name.to_ascii_lowercase();
    let knob = |setting, hint| Some(Knob { setting, hint });
    if name.contains("lumen") {
        knob(
            "sg.GlobalIlluminationQuality",
            "Lower GI scalability shrinks Lumen's scene and surface caches.",
        )
    } else if name.contains("nanite") {
        knob(
            "r.Nanite.Streaming.StreamingPoolSize",
            "Size of Nanite's streaming pool, in MB.",
        )
    } else if name.contains("shadow") {
        knob(
            "r.Shadow.Virtual.MaxPhysicalPages",
            "Fewer physical pages means a smaller virtual shadow map pool.",
        )
    } else if name.contains("virtualtexture") || name == "vt" {
        knob(
            "r.VT.PoolSizeScale",
            "Scales every virtual texture physical pool.",
        )
    } else if name.contains("distancefield") {
        knob(
            "r.GenerateMeshDistanceFields",
            "Project setting; turn off if nothing uses mesh distance fields.",
        )
    } else if name.contains("skincache") {
        knob(
            "r.SkinCache.SceneMemoryLimitInMB",
            "Caps the GPU skin cache.",
        )
    } else if name.contains("rendertarget") || name.contains("render target") {
        knob(
            "r.RenderTargetPoolMin",
            "MB the render target pool keeps before releasing unused targets.",
        )
    } else if name.contains("texture") {
        knob("r.Streaming.PoolSize", "Texture streaming pool, in MB.")
    } else {
        None
    }
}

/// How a run sits against one machine size.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Target {
    pub machine_gb: u64,
    /// What the game may use on that machine after headroom.
    pub budget: u64,
    pub draft: u64,
    pub fits: bool,
    /// How much to trim to fit (0 when it already fits).
    pub trim: u64,
    /// How much is left over (0 when it doesn't fit).
    pub spare: u64,
}

impl Target {
    pub fn measure(machine_gb: u64, headroom_percent: u64, draft: u64) -> Target {
        let machine = machine_gb * GIB;
        let budget = machine - machine * headroom_percent.min(100) / 100;
        Target {
            machine_gb,
            budget,
            draft,
            fits: draft <= budget,
            trim: draft.saturating_sub(budget),
            spare: budget.saturating_sub(draft),
        }
    }
}

/// Unreal writes text files as UTF-8 or UTF-16 (with a byte-order mark).
fn decode(bytes: &[u8]) -> String {
    let utf16 = |be: bool| {
        let units: Vec<u16> = bytes[2..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&pair| {
                if be {
                    u16::from_be_bytes(pair)
                } else {
                    u16::from_le_bytes(pair)
                }
            })
            .collect();
        String::from_utf16_lossy(&units)
    };
    match bytes {
        [0xFF, 0xFE, ..] => utf16(false),
        [0xFE, 0xFF, ..] => utf16(true),
        [0xEF, 0xBB, 0xBF, rest @ ..] => String::from_utf8_lossy(rest).into_owned(),
        _ => String::from_utf8_lossy(bytes).into_owned(),
    }
}

/// `"742.30"` MB as bytes.
fn mb(text: &str) -> Option<u64> {
    let value: f64 = text
        .trim()
        .trim_end_matches("MB")
        .trim_end_matches('M')
        .trim()
        .parse()
        .ok()?;
    (value.is_finite() && value >= 0.0).then(|| (value * MIB as f64).round() as u64)
}

#[cfg(test)]
mod tests;
