//! `salvage trim`: memory tiers for 8, 12 and 16 GB machines, written into a
//! project's config as clearly marked blocks that `--undo` takes out again.
//!
//! The tiers ride on Unreal's memory buckets: device profiles accept
//! `+CVars_<Bucket>=` lines that only apply on machines in that bucket. Salvage
//! sets the desktop bucket thresholds so 16, 12 and 8 GB machines land in
//! Default, Smaller and Smallest, and leaves bigger machines untouched.
//!
//! Every value here is Salvage's own choice, with its reason written next to it.

mod blocks;
mod values;

use std::path::PathBuf;

use crate::fsutil::Written;
use crate::project::Project;

pub use blocks::{BEGIN, Change, END, remove_block, upsert_block};
pub use values::{EDITOR_TIERS, GAME_TIERS, Setting, Tier};

/// Which machines get what, for the command's output: the tiers are per
/// machine size, the audio stream cache is per platform.
pub const SCOPE: &str = "The tiers apply on 8, 12 and 16 GB machines. The smaller audio stream cache is set\n  per platform, so it applies on every machine, including those over 16 GB.";

/// Desktop platforms Salvage writes tiers for: (device profile, platform
/// folder, target settings section).
pub const PLATFORMS: [(&str, &str, &str); 3] = [
    ("Mac", "Mac", "/Script/MacTargetPlatform.MacTargetSettings"),
    (
        "Windows",
        "Windows",
        "/Script/WindowsTargetPlatform.WindowsTargetSettings",
    ),
    (
        "Linux",
        "Linux",
        "/Script/LinuxTargetPlatform.LinuxTargetSettings",
    ),
];

/// Every file `salvage trim` may touch, relative to the project.
pub fn files() -> Vec<String> {
    let mut files = vec![
        "Config/DefaultDeviceProfiles.ini".to_string(),
        "Config/DefaultEngine.ini".to_string(),
    ];
    files.extend(
        PLATFORMS
            .iter()
            .map(|(_, folder, _)| format!("Config/{folder}/{folder}Engine.ini")),
    );
    files.push("Config/DefaultEditorPerProjectUserSettings.ini".to_string());
    files
}

/// The blocks to write, keyed by project-relative path.
pub fn plan(editor: bool) -> Vec<(String, String)> {
    let mut plan = vec![(
        "Config/DefaultDeviceProfiles.ini".to_string(),
        device_profiles(editor),
    )];
    for (_, folder, section) in PLATFORMS {
        plan.push((
            format!("Config/{folder}/{folder}Engine.ini"),
            platform_engine(section),
        ));
    }
    if editor {
        plan.push(("Config/DefaultEngine.ini".to_string(), editor_engine()));
        plan.push((
            "Config/DefaultEditorPerProjectUserSettings.ini".to_string(),
            editor_user_settings(),
        ));
    }
    plan
}

/// Writes (or rewrites) Salvage's blocks. Files not in this plan lose any old block.
pub fn apply(
    project: &Project,
    editor: bool,
    stamp: u64,
) -> Result<Vec<(PathBuf, Written)>, String> {
    let plan = plan(editor);
    let mut results = Vec::new();
    for relative in files() {
        let path = project.dir.join(&relative);
        let result = match plan.iter().find(|(file, _)| *file == relative) {
            Some((_, block)) => upsert_block(&path, block, stamp)?,
            None => match remove_block(&path, stamp)? {
                Some(written) => written,
                None => continue,
            },
        };
        results.push((path, result));
    }
    Ok(results)
}

/// Takes every Salvage block out. Files that only held a block are deleted.
pub fn undo(project: &Project, stamp: u64) -> Result<Vec<(PathBuf, Written)>, String> {
    let mut results = Vec::new();
    for relative in files() {
        let path = project.dir.join(&relative);
        if let Some(written) = remove_block(&path, stamp)? {
            results.push((path, written));
        }
    }
    Ok(results)
}

/// What `apply` would do to each file, without changing anything.
pub fn preview(project: &Project, editor: bool) -> Result<Vec<(PathBuf, Change)>, String> {
    let plan = plan(editor);
    let mut results = Vec::new();
    for relative in files() {
        let path = project.dir.join(&relative);
        let change = match plan.iter().find(|(file, _)| *file == relative) {
            Some((_, block)) => blocks::preview_upsert(&path, block)?,
            None => match blocks::preview_remove(&path)? {
                Some(change) => change,
                None => continue,
            },
        };
        results.push((path, change));
    }
    Ok(results)
}

/// What `undo` would do to each file, without changing anything.
pub fn preview_undo(project: &Project) -> Result<Vec<(PathBuf, Change)>, String> {
    let mut results = Vec::new();
    for relative in files() {
        let path = project.dir.join(&relative);
        if let Some(change) = blocks::preview_remove(&path)? {
            results.push((path, change));
        }
    }
    Ok(results)
}

/// Whether a project currently has Salvage's tiers.
pub fn is_applied(project: &Project) -> bool {
    files()
        .iter()
        .any(|f| std::fs::read_to_string(project.dir.join(f)).is_ok_and(|t| t.contains(BEGIN)))
}

const DEVICE_PROFILES_DOCS: &str = concat!(
    "https://dev.epicgames.com/documentation/en-us/unreal-engine",
    "/setting-up-device-profiles-in-unreal-engine"
);
const SCALABILITY_DOCS: &str = concat!(
    "https://dev.epicgames.com/documentation/en-us/unreal-engine",
    "/scalability-reference-for-unreal-engine"
);

fn header(lines: &[&str]) -> String {
    let mut text = String::new();
    for line in lines {
        text.push_str("; ");
        text.push_str(line);
        text.push('\n');
    }
    text
}

fn setting_lines(key: &str, settings: &[Setting]) -> String {
    let mut text = String::new();
    for setting in settings {
        for line in setting.reason.lines() {
            text.push_str(&format!("; {line}\n"));
        }
        if let Some(url) = setting.docs {
            text.push_str(&format!("; Epic's docs: {url}\n"));
        }
        text.push_str(&format!("{key}={}={}\n", setting.name, setting.value));
    }
    text
}

fn device_profiles(editor: bool) -> String {
    let mut text = header(&[
        "Memory tiers. A +CVars_<Bucket> line only applies on machines in that memory bucket;",
        "the bucket thresholds are set in Config/<Platform>/<Platform>Engine.ini. Machines over",
        "16 GB are in the Larger or Largest bucket and get none of these.",
        "A device profile value wins over the scalability presets (sg.* quality levels), so a",
        "tier only pins a value that no preset goes below; the reasons name the preset values.",
        format!("Memory buckets and +CVars_<Bucket>: {DEVICE_PROFILES_DOCS}").as_str(),
        format!("Scalability presets: {SCALABILITY_DOCS}").as_str(),
    ]);
    for (profile, _, _) in PLATFORMS {
        text.push_str(&format!("\n[{profile} DeviceProfile]\n"));
        let tiers = GAME_TIERS.iter().chain(if editor {
            EDITOR_TIERS.iter()
        } else {
            [].iter()
        });
        for tier in tiers {
            let settings: Vec<Setting> = tier
                .settings
                .iter()
                .filter(|s| s.applies_to(profile))
                .copied()
                .collect();
            if settings.is_empty() {
                continue;
            }
            text.push_str(&format!(
                "; --- {} ({} GB machines) ---\n",
                tier.label, tier.gb
            ));
            text.push_str(&setting_lines(
                &format!("+CVars_{}", tier.bucket),
                &settings,
            ));
        }
    }
    text
}

fn platform_engine(target_section: &str) -> String {
    let mut text = header(&[
        "Memory buckets for desktop machines, so 16, 12 and 8 GB machines get their own tiers",
        "(see Config/DefaultDeviceProfiles.ini). SmallestMemoryBucket_MinGB is deliberately left",
        "unset: if it's set, the engine refuses to start on machines below it.",
        format!("Epic's docs: {DEVICE_PROFILES_DOCS}").as_str(),
    ]);
    text.push_str("\n[PlatformMemoryBuckets]\n");
    text.push_str(
        "; 32 GB and up: the engine's top bucket, unchanged.\nLargestMemoryBucket_MinGB=32\n",
    );
    text.push_str("; 17 to 31 GB: no Salvage tier, so these machines keep stock settings.\nLargerMemoryBucket_MinGB=17\n");
    text.push_str("; 14 to 16 GB: the 16 GB tier.\nDefaultMemoryBucket_MinGB=14\n");
    text.push_str("; 10 to 13 GB: the 12 GB tier. Anything below is the 8 GB tier.\nSmallerMemoryBucket_MinGB=10\n");
    text.push_str(&format!("\n[{target_section}]\n"));
    text.push_str(
        "; Audio stream cache: 32 MB instead of 64 MB. It holds compressed chunks of streamed sounds,\n\
         ; and half the size still keeps several seconds of many streams resident. Raise it if\n\
         ; streamed audio stutters. It's set per platform, not per tier, so it applies on every\n\
         ; machine, including those over 16 GB that get no tier.\n\
         ; Epic's docs: https://dev.epicgames.com/documentation/en-us/unreal-engine/an-overview-of-audio-stream-caching-in-unreal-engine\n\
         CacheSizeKB=32768\n",
    );
    text
}

fn editor_engine() -> String {
    let mut text = header(&["Editor settings for low-RAM machines (salvage trim --editor)."]);
    text.push_str("\n[DevOptions.Shaders]\n");
    text.push_str(
        "; Leave 5 cores free of shader compile workers instead of 3. Each worker can use from a\n\
         ; few hundred MB to over 1 GB on heavy materials, so fewer workers means a lower peak\n\
         ; while compiling, at the cost of slower first compiles.\n\
         NumUnusedShaderCompilingThreads=5\n\
         ; Same while a game or PIE session is running, where memory is tighter still.\n\
         NumUnusedShaderCompilingThreadsDuringGame=6\n",
    );
    text.push_str("\n[ConsoleVariables]\n");
    text.push_str(
        "; Virtual texture pools stay at their configured size instead of growing when\n\
         ; oversubscribed (blurrier virtual textures instead of more memory). This is already\n\
         ; the default in UE 5.8; it's written here so nothing else turns it on.\n\
         r.VT.PoolAutoGrow=0\n",
    );
    text
}

fn editor_user_settings() -> String {
    let mut text = header(&["Editor defaults for low-RAM machines (salvage trim --editor)."]);
    text.push_str("\n[/Script/UnrealEd.ContentBrowserSettings]\n");
    text.push_str(
        "; Static Content Browser thumbnails. Realtime thumbnails keep rendering material and mesh\n\
         ; previews, which costs GPU memory and time. Each person can turn them back on in their\n\
         ; own Content Browser settings.\n\
         RealTimeThumbnails=False\n",
    );
    text
}

#[cfg(test)]
mod tests;
