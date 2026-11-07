//! The checks themselves. Each returns one [`Check`] with a concrete fix
//! whenever something needs doing.

use std::path::{Path, PathBuf};

use super::{Check, Context, Fix, Group, Status};
use crate::ini;
use crate::units::{GIB, bytes};
use crate::version::Version;

/// Free space below this on a volume Unreal writes to is a failure.
pub const DISK_FAIL_BELOW: u64 = 10 * GIB;
/// ...and below this, a warning.
pub const DISK_WARN_BELOW: u64 = 30 * GIB;
/// Local caches above this deserve a look.
pub const CACHE_WARN_ABOVE: u64 = 64 * GIB;
/// Below this much RAM the editor is tight (a little under 16 GB, since
/// some systems report slightly less than what's installed).
pub const RAM_TIGHT_BELOW: u64 = 15 * GIB + GIB / 2;
/// Epic's recommended RAM for the editor.
pub const RAM_RECOMMENDED: u64 = 32 * GIB;
/// Swap in use above this share of RAM means the machine is paging hard.
pub const SWAP_WARN_RATIO: f64 = 0.5;

const MAC_SHADER_SECTION: &str = "/Script/MacTargetPlatform.MacTargetSettings";
const XCODE_SELECT: &str = "sudo xcode-select -s /Applications/Xcode.app";
const METAL_DOWNLOAD: &str = "xcodebuild -downloadComponent MetalToolchain";
const XCODE_GUARD: &str = "salvage xcode";

pub fn all(ctx: &Context) -> Vec<Check> {
    vec![
        engine_install(ctx),
        shader_platforms(ctx),
        xcode(ctx),
        xcode_select(ctx),
        metal_toolchain(ctx),
        rosetta(ctx),
        free_space(ctx),
        path_spaces(ctx),
        caches(ctx),
        ram(ctx),
        swap(ctx),
        lldbinit(ctx),
    ]
}

fn mac_only(check: Check, ctx: &Context) -> Option<Check> {
    (!ctx.is_mac()).then(|| check.skip("macOS only"))
}

// --- Engine ----------------------------------------------------------------

fn engine_install(ctx: &Context) -> Check {
    let check = Check::new("engine", Group::Engine, "Engine install");
    let Some(engine) = ctx.engine else {
        return check
            .warn("No Unreal Engine install found, so engine checks were skipped")
            .fix(Fix::hint(
                "Point doctor at one with --engine <path> (or set SALVAGE_ENGINE).",
            ));
    };
    let kind = if engine.installed_build {
        "Launcher build"
    } else {
        "source build"
    };
    check
        .pass(format!("{}, {kind}", engine.label()))
        .detail(format!(
            "{} ({})",
            ctx.tilde(&engine.root),
            engine.found_by.describe()
        ))
}

fn shader_platforms(ctx: &Context) -> Check {
    let check = Check::new("shader-platforms", Group::Engine, "Mac shader formats");
    if let Some(skipped) = mac_only(check.clone(), ctx) {
        return skipped;
    }
    let Some(engine) = ctx.engine else {
        return check.skip("Needs an engine install");
    };
    let mut files: Vec<PathBuf> = vec![
        engine.dir().join("Config/Base.ini"),
        engine.dir().join("Config/BaseEngine.ini"),
        engine.dir().join("Config/Mac/BaseMacEngine.ini"),
    ];
    if let Some(project) = ctx.project {
        files.push(project.dir.join("Config/DefaultEngine.ini"));
        files.push(project.dir.join("Config/Mac/MacEngine.ini"));
    }
    let layers: Vec<String> = files
        .iter()
        .filter_map(|f| std::fs::read_to_string(f).ok())
        .collect();
    let formats = ini::resolve_array(&layers, MAC_SHADER_SECTION, "TargetedRHIs");
    let metal: Vec<&String> = formats
        .iter()
        .filter(|f| f.starts_with("SF_METAL"))
        .collect();
    let whose = match ctx.project {
        Some(project) => format!("{} targets", project.name),
        None => "Projects inherit the engine default of".to_string(),
    };
    let config = match ctx.project {
        Some(_) => "Config/DefaultEngine.ini".to_string(),
        None => "your project's Config/DefaultEngine.ini".to_string(),
    };
    let list = |items: &[&String]| {
        items
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join(" and ")
    };
    match metal.len() {
        0 => check.fail("No Metal shader format is targeted, so nothing can render on a Mac").fix(Fix::hint(format!(
            "In Project Settings > Platforms > Mac, tick a Targeted RHI (or add +TargetedRHIs=SF_METAL_SM6 under [{MAC_SHADER_SECTION}] in {config})."
        ))),
        1 => check.pass(format!("{whose} {}", metal[0])),
        _ => {
            let mut check = check
                .warn(format!("{whose} {}", list(&metal)))
                .detail("Each targeted format is compiled and cached separately when you cook, so two formats is roughly double the shader work and DDC.")
                .fix(Fix::hint(format!(
                    "Keep only the one you ship: untick the other under Project Settings > Platforms > Mac > Targeted RHIs, or add -TargetedRHIs=<format> under [{MAC_SHADER_SECTION}] in {config}."
                )));
            if ctx.project.is_none() {
                check = check.detail("Run with --project to check a specific project.");
            }
            check
        }
    }
}

// --- Xcode and Metal ---------------------------------------------------------

fn xcode(ctx: &Context) -> Check {
    let check = Check::new("xcode", Group::Toolchain, "Xcode");
    if let Some(skipped) = mac_only(check.clone(), ctx) {
        return skipped;
    }
    let Some(out) = ctx.probe.run("xcodebuild", &["-version"]) else {
        return check
            .fail("Xcode isn't installed")
            .fix(Fix::hint("Install Xcode from the App Store, open it once to finish setup, then run doctor again."));
    };
    if !out.success {
        let text = out.text();
        if text.contains("CommandLineTools") || text.contains("requires Xcode") {
            return check
                .fail("Only the Command Line Tools are active, and Unreal needs the full Xcode")
                .fix(Fix::run(
                    "Install Xcode, then point the developer tools at it:",
                    XCODE_SELECT,
                ));
        }
        if text.to_lowercase().contains("license") {
            return check
                .fail("Xcode's license hasn't been accepted yet")
                .fix(Fix::run(
                    "Accept it once:",
                    "sudo xcodebuild -license accept",
                ));
        }
        return check.fail(format!("xcodebuild failed: {}", out.first_line()));
    }
    let mut lines = out.stdout.lines().map(str::trim);
    let version = lines
        .next()
        .and_then(|l| l.strip_prefix("Xcode "))
        .and_then(Version::parse);
    let build = lines
        .find_map(|l| l.strip_prefix("Build version "))
        .map(str::to_string);
    let Some(version) = version else {
        return check.warn(format!(
            "Couldn't read the Xcode version from \"{}\"",
            out.first_line()
        ));
    };
    let named = match &build {
        Some(build) => format!("Xcode {version} ({build})"),
        None => format!("Xcode {version}"),
    };
    let Some(engine) = ctx.engine else {
        return check
            .warn(format!(
                "{named}, but there's no engine to check it against"
            ))
            .fix(Fix::hint(
                "Pass --engine <path> to check Xcode against the engine's supported range.",
            ));
    };
    let sdk = match engine.apple_sdk() {
        Ok(sdk) => sdk,
        Err(error) => {
            return check.warn(format!(
                "{named}; couldn't read the engine's Xcode range: {error}"
            ));
        }
    };
    let range = format!("{} to {}", sdk.min, sdk.max);
    let recommended = sdk.main.clone().unwrap_or_else(|| sdk.max.clone());
    if version < sdk.min {
        return check
            .fail(format!("{named} is older than {} supports ({range})", engine.label()))
            .fix(Fix::hint(format!("Update Xcode to {recommended} or newer (App Store, or developer.apple.com/download/all).")));
    }
    if version > sdk.max {
        return check
            .fail(format!("{named} is newer than {} supports ({range})", engine.label()))
            .detail("The engine's SDK check will reject it when you build.")
            .fix(Fix::run(
                format!("See every installed Xcode, and pin a good one for this engine (or install Xcode {recommended} with xcodes):"),
                XCODE_GUARD,
            ));
    }
    // Epic's documentation is narrower than the engine's range, and names
    // versions that don't work.
    let row = engine
        .version
        .as_ref()
        .and_then(crate::xcode::epic::row_for);
    let judgement = crate::xcode::judge(Some(&version), Some(&sdk), row);
    let warned = matches!(
        judgement.verdict,
        crate::xcode::Verdict::NotCompatible | crate::xcode::Verdict::BelowEpicMinimum
    );
    let mut check = if warned {
        check
            .warn(format!(
                "{named} is within the engine's range ({range}), but Epic warns against it"
            ))
            .fix(Fix::run(
                "See every installed Xcode, and pin a good one for this engine:",
                XCODE_GUARD,
            ))
    } else {
        check.pass(format!("{named}, within the engine's range ({range})"))
    };
    for reason in judgement
        .reasons
        .iter()
        .filter(|r| !r.starts_with("Epic builds"))
    {
        check = check.detail(reason.clone());
    }
    if let Some(main) = &sdk.main
        && *main != version
    {
        check = check.detail(format!("Epic builds this engine with Xcode {main}."));
    }
    if !warned && row.is_some() && judgement.verdict != crate::xcode::Verdict::Recommended {
        check = check
            .detail("`salvage xcode` lists every installed Xcode and how to pin one per build.");
    }
    check
}

fn xcode_select(ctx: &Context) -> Check {
    let check = Check::new("xcode-select", Group::Toolchain, "xcode-select");
    if let Some(skipped) = mac_only(check.clone(), ctx) {
        return skipped;
    }
    let Some(out) = ctx.probe.run("xcode-select", &["-p"]) else {
        return check
            .fail("xcode-select isn't available")
            .fix(Fix::hint("Install Xcode from the App Store."));
    };
    if !out.success {
        return check
            .fail("No developer directory is selected")
            .fix(Fix::run(
                "Point the developer tools at Xcode:",
                XCODE_SELECT,
            ));
    }
    let path = out.first_line();
    let mut check = if path.contains("CommandLineTools") {
        check
            .fail(format!(
                "Points at the Command Line Tools ({path}), not Xcode"
            ))
            .fix(Fix::run(
                "Point the developer tools at Xcode:",
                XCODE_SELECT,
            ))
    } else if !Path::new(&path).is_dir() {
        check
            .fail(format!("Points at {path}, which doesn't exist"))
            .fix(Fix::run(
                "Point the developer tools at an installed Xcode:",
                XCODE_SELECT,
            ))
    } else {
        check.pass(path)
    };
    if let Some(dir) = ctx.probe.env("DEVELOPER_DIR") {
        check = check.detail(format!("DEVELOPER_DIR is set to {dir}, which overrides this for anything started from this shell."));
    }
    check
}

fn metal_toolchain(ctx: &Context) -> Check {
    let check = Check::new("metal-toolchain", Group::Toolchain, "Metal Toolchain");
    if let Some(skipped) = mac_only(check.clone(), ctx) {
        return skipped;
    }
    let Some(out) = ctx.probe.run("xcrun", &["metal", "--version"]) else {
        return check
            .fail("xcrun isn't available, so the Metal compiler can't run")
            .fix(Fix::hint("Install Xcode from the App Store."));
    };
    if out.success {
        return check.pass(out.first_line());
    }
    let text = out.text();
    let missing = text.contains("missing Metal Toolchain") || text.contains("MetalToolchain");
    let check = if missing {
        check
            .fail("Not installed, so the engine can't compile Metal shaders")
            .detail("Since Xcode 26 the Metal compiler is a separate download.")
    } else if text.contains("unable to find utility") || text.contains("not a developer tool") {
        check.fail("xcrun can't find the Metal compiler")
    } else {
        check.fail(format!(
            "The Metal compiler failed to run: {}",
            out.first_line()
        ))
    };
    check.fix(Fix::run("Download it (about 1 GB):", METAL_DOWNLOAD))
}

fn rosetta(ctx: &Context) -> Check {
    let check = Check::new("rosetta", Group::Toolchain, "Rosetta");
    if let Some(skipped) = mac_only(check.clone(), ctx) {
        return skipped;
    }
    if ctx.probe.arch() != "aarch64" {
        return check.skip("Not needed on Intel Macs");
    }
    if ctx.probe.rosetta_installed() {
        return check.pass("Installed");
    }
    check
        .warn("Not installed")
        .detail("Some engine tools and third-party plugins still ship Intel-only binaries.")
        .fix(Fix::run(
            "Install it:",
            "softwareupdate --install-rosetta --agree-to-license",
        ))
}

// --- Disk --------------------------------------------------------------------

fn free_space(ctx: &Context) -> Check {
    let check = Check::new("free-space", Group::Disk, "Free space");
    let mut places: Vec<(&str, PathBuf)> = Vec::new();
    if let Some(engine) = ctx.engine {
        places.push(("engine", engine.root.clone()));
    }
    if let Some(project) = ctx.project {
        places.push(("project", project.dir.clone()));
    }
    if let Some(home) = ctx.home() {
        places.push(("home and caches", home));
    }

    // One row per volume: (roles, free bytes).
    let mut volumes: Vec<(Vec<&str>, u64, Option<u64>)> = Vec::new();
    for (role, path) in &places {
        let Some(free) = ctx.probe.free_space(path) else {
            continue;
        };
        let id = volume_id(path);
        match volumes
            .iter_mut()
            .find(|(_, f, v)| *v == id && (id.is_some() || *f == free))
        {
            Some((roles, _, _)) => roles.push(role),
            None => volumes.push((vec![role], free, id)),
        }
    }
    let Some(lowest) = volumes.iter().map(|(_, free, _)| *free).min() else {
        return check.skip("Couldn't read free space");
    };
    let describe =
        |roles: &[&str], free: u64| format!("{} free for the {}", bytes(free), roles.join(", "));
    let summary = if volumes.len() == 1 {
        describe(&volumes[0].0, lowest)
    } else {
        format!("{} free on the fullest volume", bytes(lowest))
    };
    let mut check = if lowest < DISK_FAIL_BELOW {
        check.fail(summary)
    } else if lowest < DISK_WARN_BELOW {
        check.warn(summary)
    } else {
        check.pass(summary)
    };
    if volumes.len() > 1 {
        for (roles, free, _) in &volumes {
            check = check.detail(describe(roles, *free));
        }
    }
    if check.status != Status::Pass {
        let xcode = if ctx.is_mac() {
            " Xcode's DerivedData (~/Library/Developer/Xcode/DerivedData) is also safe to empty."
        } else {
            ""
        };
        check = check
            .detail(format!(
                "Builds, shader compiles and caches want {} or more.",
                bytes(DISK_WARN_BELOW)
            ))
            .fix(Fix::hint(format!(
                "Make room: the caches under Zen / DDC rebuild on demand.{xcode}"
            )));
    }
    check
}

#[cfg(unix)]
fn volume_id(path: &Path) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(path).ok().map(|m| m.dev())
}

#[cfg(not(unix))]
fn volume_id(path: &Path) -> Option<u64> {
    use std::path::Component;
    match path.components().next() {
        Some(Component::Prefix(prefix)) => {
            let text = prefix.as_os_str().to_string_lossy().to_uppercase();
            Some(text.bytes().fold(0u64, |acc, b| {
                acc.wrapping_mul(31).wrapping_add(u64::from(b))
            }))
        }
        _ => None,
    }
}

fn path_spaces(ctx: &Context) -> Check {
    let check = Check::new("path-spaces", Group::Disk, "Spaces in paths");
    let spaced = |p: &Path| p.to_string_lossy().contains(' ');
    let mut risky: Vec<String> = Vec::new();
    if let Some(project) = ctx.project.filter(|p| spaced(&p.dir)) {
        risky.push(format!("project: {}", project.dir.display()));
    }
    if let Some(home) = ctx.home().filter(|h| spaced(h)) {
        risky.push(format!("home folder: {}", home.display()));
    }
    let engine_spaced = ctx.engine.filter(|e| spaced(&e.root));
    if !risky.is_empty() {
        let mut check =
            check.warn("Some paths have spaces, which trips up shell build steps and some plugins");
        for line in risky {
            check = check.detail(line);
        }
        return check.fix(Fix::hint(
            "Move the project to a path without spaces, such as ~/Projects/MyGame. Unreal's own tools cope; scripts that split paths at spaces don't.",
        ));
    }
    match engine_spaced {
        Some(engine) => check
            .pass("Only the engine path has spaces, and Epic's tools cope with that")
            .detail(format!(
                "{} is the Launcher default. If a third-party plugin's build script breaks at a space, this is why.",
                engine.root.display()
            )),
        None => check.pass("No spaces in the engine or project paths"),
    }
}

fn caches(ctx: &Context) -> Check {
    let check = Check::new("caches", Group::Disk, "Zen / DDC");
    let mut candidates: Vec<(&str, PathBuf)> = Vec::new();
    let home = ctx.home();
    let user_dir = match ctx.probe.os() {
        crate::probe::Os::Macos => home.map(|h| h.join("Library/Application Support/Epic")),
        crate::probe::Os::Windows => ctx
            .probe
            .env("LOCALAPPDATA")
            .map(|d| Path::new(&d).join("UnrealEngine/Common")),
        _ => home.map(|h| h.join(".config/Epic")),
    };
    if let Some(dir) = user_dir {
        candidates.push(("Zen storage", dir.join("Zen/Data")));
        candidates.push(("Shared DDC", dir.join("DerivedDataCache")));
    }
    if let Some(engine) = ctx.engine {
        candidates.push(("Engine DDC", engine.dir().join("DerivedDataCache")));
    }
    if let Some(project) = ctx.project {
        candidates.push(("Project DDC", project.dir.join("DerivedDataCache")));
    }
    for (label, key) in [
        ("DDC override", "UE-LocalDataCachePath"),
        ("Zen override", "UE-ZenSubprocessDataPath"),
    ] {
        if let Some(path) = ctx
            .probe
            .env(key)
            .filter(|p| !p.eq_ignore_ascii_case("None"))
        {
            candidates.push((label, PathBuf::from(path)));
        }
    }

    let mut found: Vec<(&str, PathBuf, u64)> = Vec::new();
    for (label, path) in candidates {
        if found.iter().any(|(_, p, _)| *p == path) || !path.is_dir() {
            continue;
        }
        found.push((label, path.clone(), dir_size(&path)));
    }
    if found.is_empty() {
        return check.pass("No local caches yet");
    }
    found.sort_by_key(|f| std::cmp::Reverse(f.2));
    let total: u64 = found.iter().map(|(_, _, size)| size).sum();
    let noun = if found.len() == 1 { "cache" } else { "caches" };
    let summary = format!("{} across {} {noun}", bytes(total), found.len());
    let mut check = if total > CACHE_WARN_ABOVE {
        check.warn(summary)
    } else {
        check.pass(summary)
    };
    for (label, path, size) in &found {
        check = check.detail(format!("{label}: {} in {}", bytes(*size), ctx.tilde(path)));
    }
    if check.status == Status::Warn {
        check = check.fix(Fix::hint(format!(
            "Close the editor, then delete the biggest one ({}). It refills as you work; expect a slower first launch while shaders recompile.",
            ctx.tilde(&found[0].1)
        )));
    }
    check
}

/// Total size of the files under `dir`, without following symlinks.
pub fn dir_size(dir: &Path) -> u64 {
    let mut total = 0;
    let mut pending = vec![dir.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            let Ok(meta) = entry.metadata() else { continue };
            if meta.is_dir() {
                pending.push(entry.path());
            } else if meta.is_file() {
                total += meta.len();
            }
        }
    }
    total
}

// --- Memory ------------------------------------------------------------------

fn ram(ctx: &Context) -> Check {
    let check = Check::new("ram", Group::Memory, "RAM");
    let Some(memory) = ctx.probe.memory() else {
        return check.skip("Couldn't read memory size");
    };
    let total = bytes(memory.total);
    if memory.total < RAM_TIGHT_BELOW {
        return check
            .warn(format!("{total}, which is tight for the editor"))
            .detail(format!("Epic recommends {} for the editor.", bytes(RAM_RECOMMENDED)))
            .fix(Fix::hint("Close browsers and other heavy apps before opening the editor, and keep big levels unloaded until you need them."));
    }
    let check = check.pass(total);
    if memory.total < RAM_RECOMMENDED {
        return check.detail(format!(
            "Below Epic's recommended {}. Salvage is built for machines like this one.",
            bytes(RAM_RECOMMENDED)
        ));
    }
    check
}

fn swap(ctx: &Context) -> Check {
    let check = Check::new("swap", Group::Memory, "Swap");
    let Some(memory) = ctx.probe.memory() else {
        return check.skip("Couldn't read swap usage");
    };
    if memory.swap_used == 0 {
        return check.pass("None in use");
    }
    let used = bytes(memory.swap_used);
    if (memory.swap_used as f64) > (memory.total as f64) * SWAP_WARN_RATIO {
        let monitor = match ctx.probe.os() {
            crate::probe::Os::Macos => "Activity Monitor's Memory tab",
            crate::probe::Os::Windows => "Task Manager",
            _ => "top or htop",
        };
        return check
            .warn(format!("{used} in use, so the system is paging to disk"))
            .detail("Everything slows down when the editor's working set lands in swap.")
            .fix(Fix::hint(format!(
                "Quit apps you aren't using ({monitor} shows the biggest), or restart before a long build or cook."
            )));
    }
    check.pass(format!("{used} in use"))
}

// --- Debugging -----------------------------------------------------------------

fn lldbinit(ctx: &Context) -> Check {
    let check = Check::new("lldbinit", Group::Debugging, "~/.lldbinit");
    if let Some(skipped) = mac_only(check.clone(), ctx) {
        return skipped;
    }
    let Some(home) = ctx.home() else {
        return check.skip("No home folder");
    };
    let install = |hint: &str| Fix::run(hint, "salvage lldb install");

    // Xcode reads ~/.lldbinit-Xcode when it exists, plain lldb reads ~/.lldbinit.
    let files: Vec<PathBuf> = [".lldbinit", ".lldbinit-Xcode"]
        .iter()
        .map(|n| home.join(n))
        .filter(|p| p.is_file())
        .collect();
    if files.is_empty() {
        return check
            .warn("Missing, so FString, TSharedPtr, TObjectPtr and friends show up as raw memory in the debugger")
            .fix(install("Install Salvage's Unreal formatters (adds one marked line):"));
    }
    let imports: Vec<Import> = files
        .iter()
        .filter_map(|f| std::fs::read_to_string(f).ok())
        .flat_map(|text| {
            text.lines()
                .filter_map(|l| formatter_import(l, &home))
                .collect::<Vec<_>>()
        })
        .collect();
    if let Some(ours) = imports.iter().find(|i| i.salvage && i.path.is_file()) {
        return check.pass(format!(
            "Loads Salvage's Unreal formatters from {}",
            ctx.tilde(&ours.path)
        ));
    }
    if let Some(engine_import) = imports.iter().find(|i| !i.salvage && i.path.is_file()) {
        let mut check = check
            .pass(format!("Loads the engine's formatters from {}", ctx.tilde(&engine_import.path)))
            .detail("Run `salvage lldb install` to add TSharedPtr, TObjectPtr, FText, TOptional, math types and more alongside them.");
        if let Some(engine) = ctx.engine
            && !engine_import.path.starts_with(&engine.root)
        {
            check = check.detail("That's a different engine install than the one doctor checked; fine if it's the same version.");
        }
        return check;
    }
    if let Some(stale) = imports.first() {
        return check
            .warn(format!(
                "Imports formatters from {}, which no longer exists",
                ctx.tilde(&stale.path)
            ))
            .fix(install(
                "Install Salvage's formatters (and delete the stale line from your init file):",
            ));
    }
    check
        .warn("Doesn't load any Unreal formatters, so FString, TSharedPtr, TObjectPtr and friends show up as raw memory")
        .fix(install("Install Salvage's Unreal formatters (backs up your file, adds one marked line):"))
}

/// A `command script import` line that loads Unreal formatters.
struct Import {
    path: PathBuf,
    /// Salvage's own formatters, rather than the engine's.
    salvage: bool,
}

fn formatter_import(line: &str, home: &Path) -> Option<Import> {
    let line = line.trim();
    if line.starts_with('#') {
        return None;
    }
    let rest = line.strip_prefix("command script import")?.trim();
    let path = rest.trim_matches(|c| c == '"' || c == '\'');
    let salvage = path.contains(crate::lldb::SCRIPT_NAME);
    if !salvage && !path.contains("UEDataFormatters") {
        return None;
    }
    let path = match path.strip_prefix("~/") {
        Some(tail) => home.join(tail),
        None => PathBuf::from(path),
    };
    Some(Import { path, salvage })
}
