//! Salvage's chosen values for each tier, each with the reason for it.
//!
//! What these move away from (UE 5.8): Nanite streaming pool 512 MB, low-memory
//! GC mode off, unreachable objects purged every 60 s. The rest come from the
//! scalability presets: texture pool 400, 600, 800 and 1000 MB at Low, Medium,
//! High and Epic textures; render targets kept up to 300, 350, 400 and 400 MB at
//! those post-process levels; virtual shadow map pages 512 at Low and Medium
//! shadows, 2048 at High, 4096 at Epic; Lumen atlas 2048 at Medium GI, 3584 at
//! High, 4096 at Epic.
//!
//! A device profile value wins over every quality preset, so a tier only pins a
//! value that is no higher than any preset that uses the feature: then no quality
//! level ends up with more memory than it had.

/// One console variable and the value a tier gives it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Setting {
    pub name: &'static str,
    pub value: &'static str,
    pub reason: &'static str,
    /// Only for this device profile (None: all desktop platforms).
    pub only: Option<&'static str>,
}

impl Setting {
    pub fn applies_to(&self, profile: &str) -> bool {
        self.only.is_none_or(|only| only == profile)
    }
}

/// A memory tier: the settings for one machine size.
#[derive(Clone, Copy, Debug)]
pub struct Tier {
    pub gb: u32,
    /// The memory bucket name used in `+CVars_<Bucket>`.
    pub bucket: &'static str,
    pub label: &'static str,
    pub settings: &'static [Setting],
}

const fn all(name: &'static str, value: &'static str, reason: &'static str) -> Setting {
    Setting {
        name,
        value,
        reason,
        only: None,
    }
}

const fn mac(name: &'static str, value: &'static str, reason: &'static str) -> Setting {
    Setting {
        name,
        value,
        reason,
        only: Some("Mac"),
    }
}

pub const GAME_TIERS: [Tier; 3] = [
    Tier {
        gb: 16,
        bucket: "Default",
        label: "16 GB tier",
        settings: &[
            all(
                "r.Nanite.Streaming.StreamingPoolSize",
                "384",
                "Nanite streaming pool 384 MB (default 512): a quarter less mesh data kept resident.",
            ),
            all(
                "r.RenderTargetPoolMin",
                "300",
                "Start releasing unused render targets above 300 MB, what the Low post-process preset\nuses (Medium 350, High and Epic 400).",
            ),
            all(
                "gc.LowMemory.MemoryThresholdMB",
                "2048",
                "Enter low-memory garbage collection when less than 2 GB is free (off by default).",
            ),
            all(
                "gc.LowMemory.TimeBetweenPurgingPendingKillObjects",
                "15",
                "In low-memory mode, purge unreachable objects every 15 s (default 30).",
            ),
        ],
    },
    Tier {
        gb: 12,
        bucket: "Smaller",
        label: "12 GB tier",
        settings: &[
            all(
                "r.Nanite.Streaming.StreamingPoolSize",
                "320",
                "Nanite streaming pool 320 MB (default 512).",
            ),
            all(
                "r.LumenScene.SurfaceCache.AtlasSize",
                "2048",
                "Lumen surface cache atlas 2048, the Medium GI preset's size (High uses 3584, Epic\n4096): less atlas memory, with lower-detail bounce lighting further from the camera.\nNo preset that uses Lumen goes below 2048, so this never adds memory.",
            ),
            all(
                "r.RenderTargetPoolMin",
                "200",
                "Release unused render targets above 200 MB.",
            ),
            all(
                "gc.LowMemory.MemoryThresholdMB",
                "1536",
                "Low-memory garbage collection below 1.5 GB free.",
            ),
            all(
                "gc.LowMemory.TimeBetweenPurgingPendingKillObjects",
                "10",
                "In low-memory mode, purge unreachable objects every 10 s.",
            ),
            all(
                "gc.TimeBetweenPurgingPendingKillObjects",
                "45",
                "Purge unreachable objects every 45 s (default 60), so memory comes back sooner.",
            ),
        ],
    },
    Tier {
        gb: 8,
        bucket: "Smallest",
        label: "8 GB tier",
        settings: &[
            mac(
                "r.Streaming.PoolSize",
                "400",
                "Texture pool of 400 MB, what the Low texture preset uses (Medium 600, High 800,\nEpic 1000): when a scene wants more texture data than that, textures drop to lower\nmips. Mac only: Apple silicon shares memory between CPU and GPU, so the pool comes\nout of the machine's RAM.",
            ),
            all(
                "r.Nanite.Streaming.StreamingPoolSize",
                "256",
                "Nanite streaming pool 256 MB (default 512).",
            ),
            all(
                "r.Shadow.Virtual.MaxPhysicalPages",
                "512",
                "512 virtual shadow map pages, what the Low and Medium shadow presets use (High
uses 2048, Epic 4096): blurrier shadows at High and Epic. Only this tier pins it:
on bigger machines a pin above 512 would add memory at Low and Medium.",
            ),
            all(
                "r.LumenScene.SurfaceCache.AtlasSize",
                "2048",
                "Lumen surface cache atlas 2048, the Medium GI preset's size.",
            ),
            all(
                "r.RenderTargetPoolMin",
                "150",
                "Release unused render targets above 150 MB.",
            ),
            all(
                "gc.LowMemory.MemoryThresholdMB",
                "1024",
                "Low-memory garbage collection below 1 GB free.",
            ),
            all(
                "gc.LowMemory.TimeBetweenPurgingPendingKillObjects",
                "5",
                "In low-memory mode, purge unreachable objects every 5 s.",
            ),
            all(
                "gc.TimeBetweenPurgingPendingKillObjects",
                "30",
                "Purge unreachable objects every 30 s (default 60).",
            ),
            all(
                "foliage.DiscardDataOnLoad",
                "1",
                "Drop foliage instance data on load for foliage types that allow it (their\n\"Enable Discard On Load\" option); other foliage is untouched. Landscape grass\n(grass.DiscardDataOnLoad) is left on: discarding it removes grass entirely.",
            ),
        ],
    },
];

/// Editor settings that depend on machine size (salvage trim --editor).
pub const EDITOR_TIERS: [Tier; 3] = [
    Tier {
        gb: 16,
        bucket: "Default",
        label: "16 GB editor",
        settings: &[all(
            "r.ShaderCompiler.MemoryLimit",
            "4096",
            "Shader compile workers may use 4 GB between them (no limit by default); above that\nthe editor pauses workers and reschedules their jobs instead of pushing into swap.",
        )],
    },
    Tier {
        gb: 12,
        bucket: "Smaller",
        label: "12 GB editor",
        settings: &[all(
            "r.ShaderCompiler.MemoryLimit",
            "3072",
            "Shader compile workers share 3 GB at most.",
        )],
    },
    Tier {
        gb: 8,
        bucket: "Smallest",
        label: "8 GB editor",
        settings: &[all(
            "r.ShaderCompiler.MemoryLimit",
            "2048",
            "Shader compile workers share 2 GB at most.",
        )],
    },
];
