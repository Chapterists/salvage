//! `salvage xcode`: every installed Xcode, judged against the engine you build
//! with, and how to pin the right one for that engine alone.
//!
//! Two sources decide what's good: the engine's own Apple SDK file (the range
//! its build check accepts, read from the user's install) and Epic's macOS
//! requirements page (narrower, with versions it says don't work; see
//! `epic`). Installing and switching Xcode is xcodes' job: Salvage prints the
//! `xcodes install` command and never installs Xcode, runs sudo or changes
//! xcode-select. Pinning is per build, with DEVELOPER_DIR.

pub mod epic;

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::engine::{AppleSdk, Engine};
use crate::probe::Probe;
use crate::version::Version;
pub use epic::EpicRow;

/// The tools the engine's Metal shader compiler runs through xcrun.
pub const METAL_TOOLS: [&str; 4] = ["metal", "metallib", "metal-ar", "metal-pack"];

pub const XCODES_URL: &str = "https://github.com/XcodesOrg/xcodes";
pub const XCODES_APP_URL: &str = "https://github.com/XcodesOrg/XcodesApp";

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Verdict {
    /// The version Epic recommends for this engine.
    Recommended,
    /// Inside the engine's range and Epic's documented minimum.
    Supported,
    /// Inside the engine's range, but newer than anything Epic recommends.
    Newer,
    /// Inside the engine's range, but older than Epic's documented minimum.
    BelowEpicMinimum,
    /// Inside the engine's range, but Epic says it doesn't work.
    NotCompatible,
    /// Outside the engine's range: its build check rejects it.
    OutsideRange,
    /// The version couldn't be read.
    Unknown,
}

impl Verdict {
    /// Whether builds should use it without a second thought.
    pub fn good(self) -> bool {
        matches!(
            self,
            Verdict::Recommended | Verdict::Supported | Verdict::Newer
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Judgement {
    pub verdict: Verdict,
    pub reasons: Vec<String>,
}

/// Judges one Xcode version against the engine's range and Epic's row.
pub fn judge(xcode: Option<&Version>, sdk: Option<&AppleSdk>, row: Option<&EpicRow>) -> Judgement {
    let Some(xcode) = xcode else {
        return Judgement {
            verdict: Verdict::Unknown,
            reasons: vec!["Couldn't read its version.".to_string()],
        };
    };
    let mut reasons = Vec::new();
    if let Some(sdk) = sdk {
        if *xcode < sdk.min || *xcode > sdk.max {
            reasons.push(format!(
                "The engine's build check accepts Xcode {} to {}, so it rejects {xcode}.",
                sdk.min, sdk.max
            ));
            return Judgement {
                verdict: Verdict::OutsideRange,
                reasons,
            };
        }
        if sdk.main.as_ref() == Some(xcode) {
            reasons.push(format!("Epic builds this engine with Xcode {xcode}."));
        }
    }
    let Some(row) = row else {
        if sdk.is_none() {
            reasons.push("There's no engine range or Epic guidance to judge it by.".to_string());
        }
        return Judgement {
            verdict: Verdict::Supported,
            reasons,
        };
    };
    if let Some(listed) = row.not_compatible_match(xcode) {
        reasons.push(format!(
            "Epic's macOS requirements page says Xcode {listed} is not compatible with Unreal Engine {}.",
            row.engines
        ));
        return Judgement {
            verdict: Verdict::NotCompatible,
            reasons,
        };
    }
    if *xcode < row.minimum() {
        reasons.push(format!(
            "Epic lists Xcode {} as the minimum for Unreal Engine {}.",
            row.minimum, row.engines
        ));
        return Judgement {
            verdict: Verdict::BelowEpicMinimum,
            reasons,
        };
    }
    if row.is_recommended(xcode) {
        reasons.push(format!(
            "Epic recommends {} for Unreal Engine {}.",
            row.describe_recommended(),
            row.engines
        ));
        return Judgement {
            verdict: Verdict::Recommended,
            reasons,
        };
    }
    let newer = match row.recommended {
        epic::Recommended::Exactly(v) => Version::parse(v).is_some_and(|v| *xcode > v),
        epic::Recommended::LatestOf(major) => xcode.parts().first().is_some_and(|m| *m > major),
        epic::Recommended::AtLeast(_) => false,
    };
    reasons.push(format!(
        "Epic recommends {} for Unreal Engine {}{}.",
        row.describe_recommended(),
        row.engines,
        if newer {
            "; this one is newer than that"
        } else {
            ""
        }
    ));
    Judgement {
        verdict: if newer {
            Verdict::Newer
        } else {
            Verdict::Supported
        },
        reasons,
    }
}

/// An installed Xcode.
#[derive(Clone, Debug, Serialize)]
pub struct Installed {
    /// The app bundle, e.g. /Applications/Xcode-26.1.1.app.
    pub app: PathBuf,
    /// Its developer folder: what DEVELOPER_DIR points at.
    pub developer_dir: PathBuf,
    pub version: Option<Version>,
    pub build: Option<String>,
    /// `xcrun metal --version` worked with this Xcode.
    pub metal_toolchain: bool,
    /// Metal tools the engine runs that xcrun couldn't find for this Xcode.
    pub missing_metal_tools: Vec<String>,
    pub judgement: Judgement,
}

/// The app bundle a developer folder belongs to (…/Xcode.app/Contents/Developer).
pub fn app_of(developer_dir: &Path) -> Option<PathBuf> {
    let app = developer_dir.parent()?.parent()?;
    app.extension()
        .is_some_and(|e| e == "app")
        .then(|| app.to_path_buf())
}

fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Every Xcode app found by Spotlight, in `app_dirs` (named Xcode*.app), by
/// xcodes, or selected by xcode-select or DEVELOPER_DIR. Only bundles with a
/// developer folder count.
pub fn find(probe: &dyn Probe, app_dirs: &[PathBuf]) -> Vec<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(out) = probe.run(
        "mdfind",
        &["kMDItemCFBundleIdentifier == 'com.apple.dt.Xcode'"],
    ) && out.success
    {
        candidates.extend(
            out.stdout
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(PathBuf::from),
        );
    }
    for dir in app_dirs {
        for entry in std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
        {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with("Xcode") && name.ends_with(".app") {
                candidates.push(entry.path());
            }
        }
    }
    if let Some(out) = probe.run("xcodes", &["installed"])
        && out.success
    {
        for line in out.stdout.lines() {
            if let Some(start) = line.find('/') {
                let rest = &line[start..];
                if let Some(end) = rest.find(".app") {
                    candidates.push(PathBuf::from(&rest[..end + 4]));
                }
            }
        }
    }
    if let Some(dir) = selected_developer_dir(probe) {
        candidates.extend(app_of(&dir));
    }
    if let Some(dir) = probe.env("DEVELOPER_DIR") {
        candidates.extend(app_of(Path::new(&dir)));
    }
    let mut found: Vec<PathBuf> = Vec::new();
    for app in candidates {
        if app.join("Contents/Developer").is_dir()
            && !found.iter().any(|f| canonical(f) == canonical(&app))
        {
            found.push(app);
        }
    }
    found.sort();
    found
}

/// What xcode-select points at, if it's set.
pub fn selected_developer_dir(probe: &dyn Probe) -> Option<PathBuf> {
    probe
        .run("xcode-select", &["-p"])
        .filter(|o| o.success)
        .map(|o| PathBuf::from(o.first_line()))
        .filter(|p| !p.as_os_str().is_empty())
}

pub fn inspect(
    probe: &dyn Probe,
    app: &Path,
    sdk: Option<&AppleSdk>,
    row: Option<&EpicRow>,
) -> Installed {
    let developer_dir = app.join("Contents/Developer");
    let assignment = format!("DEVELOPER_DIR={}", developer_dir.display());
    // `env DEVELOPER_DIR=<dir> <command>` runs a tool as if that Xcode were selected.
    let run = |command: &[&str]| {
        let mut args = vec![assignment.as_str()];
        args.extend_from_slice(command);
        probe.run("env", &args)
    };
    let (version, build) = match run(&["xcodebuild", "-version"]) {
        Some(out) if out.success => parse_xcodebuild_version(&out.stdout),
        _ => (None, None),
    };
    let metal_toolchain = run(&["xcrun", "metal", "--version"]).is_some_and(|o| o.success);
    let missing_metal_tools = METAL_TOOLS
        .iter()
        .filter(|tool| !run(&["xcrun", "--find", tool]).is_some_and(|o| o.success))
        .map(|tool| tool.to_string())
        .collect();
    let judgement = judge(version.as_ref(), sdk, row);
    Installed {
        app: app.to_path_buf(),
        developer_dir,
        version,
        build,
        metal_toolchain,
        missing_metal_tools,
        judgement,
    }
}

/// `Xcode 27.2` / `Build version 27B52`.
pub fn parse_xcodebuild_version(text: &str) -> (Option<Version>, Option<String>) {
    let mut lines = text.lines().map(str::trim);
    let version = lines
        .next()
        .and_then(|l| l.strip_prefix("Xcode "))
        .and_then(Version::parse);
    let build = lines
        .find_map(|l| l.strip_prefix("Build version "))
        .map(str::to_string);
    (version, build)
}

#[derive(Clone, Debug, Serialize)]
pub struct Report {
    pub engine: Option<String>,
    pub engine_root: Option<PathBuf>,
    /// The range from the engine's Apple SDK file.
    pub engine_range: Option<AppleSdk>,
    pub engine_range_error: Option<String>,
    /// Epic's documented guidance for this engine, quoted.
    pub epic: Option<EpicGuidance>,
    pub installed: Vec<Installed>,
    /// What xcode-select points at.
    pub xcode_select: Option<PathBuf>,
    /// DEVELOPER_DIR in this shell, which wins over xcode-select.
    pub developer_dir_env: Option<String>,
    /// The developer folder builds started from this shell use.
    pub active: Option<PathBuf>,
    /// The installed Xcode to pin for this engine, if any is good.
    pub pick: Option<PathBuf>,
    /// A version to install with xcodes, when nothing installed is recommended.
    pub install: Option<String>,
    pub xcodes_installed: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct EpicGuidance {
    pub engines: String,
    pub minimum: String,
    pub recommended: String,
    pub not_compatible: Vec<String>,
    pub says: String,
    pub source: String,
    pub checked: String,
}

pub fn report(probe: &dyn Probe, engine: Option<&Engine>, app_dirs: &[PathBuf]) -> Report {
    let (sdk, sdk_error) = match engine.map(Engine::apple_sdk) {
        Some(Ok(sdk)) => (Some(sdk), None),
        Some(Err(e)) => (None, Some(e)),
        None => (None, None),
    };
    let row = engine
        .and_then(|e| e.version.as_ref())
        .and_then(epic::row_for);
    let mut installed: Vec<Installed> = find(probe, app_dirs)
        .iter()
        .map(|app| inspect(probe, app, sdk.as_ref(), row))
        .collect();
    installed.sort_by(|a, b| b.version.cmp(&a.version));
    let xcode_select = selected_developer_dir(probe);
    let developer_dir_env = probe.env("DEVELOPER_DIR");
    let active = developer_dir_env
        .as_ref()
        .map(PathBuf::from)
        .or_else(|| xcode_select.clone());
    let pick = pick(&installed).map(|i| i.app.clone());
    let recommended_installed = installed
        .iter()
        .any(|i| i.judgement.verdict == Verdict::Recommended);
    let install = if recommended_installed {
        None
    } else {
        row.and_then(EpicRow::version_to_install)
            .map(str::to_string)
            .or_else(|| {
                sdk.as_ref()
                    .and_then(|s| s.main.as_ref())
                    .map(Version::to_string)
            })
            .filter(|_| engine.is_some())
    };
    Report {
        engine: engine.map(Engine::label),
        engine_root: engine.map(|e| e.root.clone()),
        engine_range: sdk,
        engine_range_error: sdk_error,
        epic: row.map(|row| EpicGuidance {
            engines: row.engines.to_string(),
            minimum: row.minimum.to_string(),
            recommended: row.describe_recommended(),
            not_compatible: row.not_compatible.iter().map(|s| s.to_string()).collect(),
            says: row.says.to_string(),
            source: row.source.to_string(),
            checked: row.checked.to_string(),
        }),
        installed,
        xcode_select,
        developer_dir_env,
        active,
        pick,
        install,
        xcodes_installed: probe.run("xcodes", &["version"]).is_some_and(|o| o.success),
    }
}

/// The best installed Xcode for the engine: recommended first, then
/// supported, then newer; the newest of those within each. Never one the
/// engine rejects or Epic warns about.
pub fn pick(installed: &[Installed]) -> Option<&Installed> {
    installed
        .iter()
        .filter(|i| i.judgement.verdict.good())
        .min_by(|a, b| {
            a.judgement
                .verdict
                .cmp(&b.judgement.verdict)
                .then_with(|| b.version.cmp(&a.version))
        })
}

/// A shell wrapper that runs one command with this Xcode pinned.
pub fn wrapper_script(developer_dir: &Path, engine: Option<&str>) -> String {
    let dir = developer_dir.display().to_string().replace('\'', "'\\''");
    format!(
        "#!/bin/sh\n\
         # {WRAPPER_MARK}\n\
         # Runs a command with Xcode pinned for {}, without changing xcode-select.\n\
         # Example: ./this-script \"<engine>/Engine/Build/BatchFiles/RunUAT.sh\" BuildCookRun ...\n\
         export DEVELOPER_DIR='{dir}'\n\
         exec \"$@\"\n",
        engine.unwrap_or("this engine")
    )
}

pub const WRAPPER_MARK: &str = "Written by salvage xcode --wrapper; safe to delete.";

#[cfg(test)]
mod tests;
