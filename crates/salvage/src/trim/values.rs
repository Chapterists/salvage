//! Salvage's chosen values for each tier, each with the reason for it.
//!
//! What these move away from (UE 5.8): Nanite streaming pool 512 MB, unreachable
//! objects purged every 60 s. The rest come from the scalability presets: texture
//! pool 400, 600, 800 and 1000 MB at Low, Medium, High and Epic textures; virtual
//! shadow map pages 512 at Low and Medium shadows, 2048 at High, 4096 at Epic;
//! Lumen atlas 2048 at Medium GI, 3584 at High, 4096 at Epic.
//!
//! Measured and left out (docs/soundings/trim-third-person-m4-16gb.md): a lower
//! r.RenderTargetPoolMin made no difference, and low-memory garbage collection
//! (gc.LowMemory.*) keys off free memory, which macOS keeps below 2 GB on a busy
//! 16 GB Mac, so it was on all the time and saved nothing.
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
    /// Epic's documentation page for the setting, when there is one.
    pub docs: Option<&'static str>,
}

impl Setting {
    const fn docs(self, url: &'static str) -> Self {
        Setting {
            docs: Some(url),
            ..self
        }
    }

    pub fn applies_to(&self, profile: &str) -> bool {
        self.only.is_none_or(|only| only == profile)
    }
}

const TEXTURE_STREAMING: &str = concat!(
    "https://dev.epicgames.com/documentation/en-us/unreal-engine",
    "/texture-streaming-configuration-in-unreal-engine"
);
const VIRTUAL_SHADOW_MAPS: &str = concat!(
    "https://dev.epicgames.com/documentation/en-us/unreal-engine",
    "/virtual-shadow-maps-in-unreal-engine"
);
const GARBAGE_COLLECTION: &str = concat!(
    "https://dev.epicgames.com/documentation/en-us/unreal-engine",
    "/garbage-collection-settings-in-the-unreal-engine-project-settings"
);

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
        docs: None,
    }
}

const fn mac(name: &'static str, value: &'static str, reason: &'static str) -> Setting {
    Setting {
        name,
        value,
        reason,
        only: Some("Mac"),
        docs: None,
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
                "Nanite streaming pool 384 MB (default 512). The pool is allocated up front, so this\nis 128 MB less memory in every scene; when a view needs more mesh data than the\npool holds, Nanite draws coarser detail until it fits.",
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
                "gc.TimeBetweenPurgingPendingKillObjects",
                "45",
                "Purge unreachable objects every 45 s (default 60), so memory comes back sooner.",
            )
            .docs(GARBAGE_COLLECTION),
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
            )
            .docs(TEXTURE_STREAMING),
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
            )
            .docs(VIRTUAL_SHADOW_MAPS),
            all(
                "r.LumenScene.SurfaceCache.AtlasSize",
                "2048",
                "Lumen surface cache atlas 2048, the Medium GI preset's size.",
            ),
            all(
                "gc.TimeBetweenPurgingPendingKillObjects",
                "30",
                "Purge unreachable objects every 30 s (default 60).",
            )
            .docs(GARBAGE_COLLECTION),
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
