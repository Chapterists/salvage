//! Reads a plugin folder and works out whether it runs on a Mac and, if not,
//! what stands in the way: allow lists, missing Mac binaries, Windows-only
//! libraries, Windows API calls in the source, or no source at all.

use std::path::{Path, PathBuf};

use serde::Serialize;

use super::spans;
use super::winapi;

/// Lists that decide which platforms a module (or the whole plugin) is for.
pub const ALLOW_KEYS: [&str; 2] = ["PlatformAllowList", "WhitelistPlatforms"];
pub const DENY_KEYS: [&str; 2] = ["PlatformDenyList", "BlacklistPlatforms"];
pub const SUPPORTED_KEY: &str = "SupportedTargetPlatforms";

/// Module types the editor never loads, so they need no editor binary.
const NOT_IN_EDITOR: [&str; 3] = ["Program", "CookedOnly", "ServerOnly"];

/// Source files worth reading for Windows API use.
const SOURCE_EXTENSIONS: [&str; 10] = ["h", "hpp", "hh", "c", "cc", "cpp", "cxx", "inl", "m", "mm"];
/// Files bigger than this are generated or vendored blobs; skip them.
const MAX_SOURCE_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Verdict {
    /// Every module the editor loads has a Mac binary.
    Works,
    /// No code at all, only content.
    ContentOnly,
    /// Some modules work on Mac; the others are listed for Windows only.
    Partial,
    /// Source is there and nothing blocks Mac: build it.
    Rebuild,
    /// The Mac binaries are there, but a list keeps Mac out: patch it.
    PatchOnly,
    /// Only an allow list keeps it off Mac: patch it, then build it.
    PatchAndRebuild,
    /// The source needs changes before it can build for Mac.
    NeedsPorting,
    /// Windows binaries and no source: only the vendor can fix it.
    NoSource,
}

impl Verdict {
    pub fn describe(&self) -> &'static str {
        match self {
            Verdict::Works => "works on Mac",
            Verdict::ContentOnly => "content only, works on Mac",
            Verdict::Partial => "works on Mac without its Windows-only modules",
            Verdict::Rebuild => "rebuild for Mac",
            Verdict::PatchOnly => "allow list only: patch (Mac binaries are there)",
            Verdict::PatchAndRebuild => "allow list only: patch, then rebuild",
            Verdict::NeedsPorting => "needs porting",
            Verdict::NoSource => "no source: ask the vendor",
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Module {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub allowed_on_mac: bool,
    /// The editor loads it (so it needs a Mac editor binary).
    pub in_editor: bool,
    /// The Mac editor binary, relative to the plugin, when there is one.
    pub mac_binary: Option<String>,
    /// Its `<Name>.Build.cs` is in the plugin.
    pub source: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Finding {
    /// A list that keeps Mac out. `module` is None for the plugin-wide list.
    AllowList {
        module: Option<String>,
        key: String,
        platforms: Vec<String>,
    },
    /// Modules the editor would load on Mac but has no Mac binary for.
    NoMacBinaries { modules: Vec<String> },
    /// No Source folder with build rules.
    NoSource,
    /// A module named in the .uplugin with no build rules in the plugin.
    ModuleSourceMissing { module: String },
    /// Prebuilt libraries for Windows (.lib, .dll) and none for Mac.
    WindowsOnlyLibrary { folder: String, files: Vec<String> },
    /// Build rules with Windows-specific settings and no Mac branch.
    WindowsOnlyBuildRules { file: String, line: usize },
    /// Windows API use outside any Windows-only block: won't compile on Mac.
    WindowsApi {
        file: String,
        line: usize,
        what: String,
    },
    /// A `#if PLATFORM_WINDOWS` block with no `#else`: compiles on Mac, but
    /// whatever it does is missing there.
    WindowsOnlyBlock { file: String, line: usize },
}

impl Finding {
    /// Findings that stop a Mac build until someone changes the source.
    pub fn blocks_build(&self) -> bool {
        matches!(
            self,
            Finding::WindowsOnlyLibrary { .. }
                | Finding::WindowsOnlyBuildRules { .. }
                | Finding::WindowsApi { .. }
                | Finding::ModuleSourceMissing { .. }
        )
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Plugin {
    pub name: String,
    pub dir: PathBuf,
    pub file: PathBuf,
    pub verdict: Verdict,
    pub modules: Vec<Module>,
    pub findings: Vec<Finding>,
}

/// Finds every plugin under `root`: a folder with a `.uplugin` is a plugin,
/// and Salvage doesn't look for more plugins inside it.
pub fn find_plugins(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    if let Some(file) = uplugin_in(root) {
        found.push(file);
        return found;
    }
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut subdirs: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .map(|e| e.path())
            .filter(|p| !skipped_dir(p))
            .collect();
        subdirs.sort();
        for sub in subdirs.into_iter().rev() {
            match uplugin_in(&sub) {
                Some(file) => found.push(file),
                None => stack.push(sub),
            }
        }
    }
    found.sort();
    found
}

fn skipped_dir(path: &Path) -> bool {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy())
        .unwrap_or_default();
    name.starts_with('.')
        || matches!(
            name.as_ref(),
            "Binaries" | "Intermediate" | "Content" | "Saved" | "DerivedDataCache" | "Resources"
        )
}

/// The `.uplugin` directly inside `dir`, if any.
pub fn uplugin_in(dir: &Path) -> Option<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("uplugin"))
        })
        .collect();
    files.sort();
    files.into_iter().next()
}

/// Whether a list of platforms (from an allow or deny list) names Mac.
fn names_mac(platforms: &[&str]) -> bool {
    platforms.iter().any(|p| p.eq_ignore_ascii_case("Mac"))
}

/// Which list, if any, keeps Mac out of this object (a module or the plugin).
fn mac_exclusion(object: &spans::Spanned, supported_too: bool) -> Option<(String, Vec<String>)> {
    for key in ALLOW_KEYS {
        if let Some(list) = object.get(key) {
            let platforms = list.strings();
            if !platforms.is_empty() && !names_mac(&platforms) {
                return Some((
                    key.to_string(),
                    platforms.iter().map(|s| s.to_string()).collect(),
                ));
            }
        }
    }
    for key in DENY_KEYS {
        if let Some(list) = object.get(key) {
            let platforms = list.strings();
            if names_mac(&platforms) {
                return Some((
                    key.to_string(),
                    platforms.iter().map(|s| s.to_string()).collect(),
                ));
            }
        }
    }
    if supported_too && let Some(list) = object.get(SUPPORTED_KEY) {
        let platforms = list.strings();
        if !platforms.is_empty() && !names_mac(&platforms) {
            return Some((
                SUPPORTED_KEY.to_string(),
                platforms.iter().map(|s| s.to_string()).collect(),
            ));
        }
    }
    None
}

/// Scans one plugin. `deep` reads sources and libraries even when the plugin
/// already works (otherwise that's skipped, which keeps engine scans fast).
pub fn scan(file: &Path, deep: bool) -> Result<Plugin, String> {
    let text =
        std::fs::read_to_string(file).map_err(|e| format!("can't read {}: {e}", file.display()))?;
    let json =
        spans::parse(&text).map_err(|e| format!("{} isn't valid JSON: {e}", file.display()))?;
    let dir = file.parent().map(Path::to_path_buf).unwrap_or_default();
    let name = file
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();

    let mut findings = Vec::new();
    if let Some((key, platforms)) = mac_exclusion(&json, true) {
        findings.push(Finding::AllowList {
            module: None,
            key,
            platforms,
        });
    }

    let build_rules = build_rules_in(&dir.join("Source"));
    let binaries = mac_binaries(&dir);
    let mut modules = Vec::new();
    if let Some(list) = json.get("Modules") {
        for item in list.items() {
            let Some(module_name) = item.get("Name").and_then(|n| n.as_str()) else {
                continue;
            };
            let kind = item
                .get("Type")
                .and_then(|t| t.as_str())
                .unwrap_or("Runtime")
                .to_string();
            let exclusion = mac_exclusion(item, false);
            let editor_targets = item
                .get("TargetAllowList")
                .map(|l| l.strings().iter().any(|t| t.eq_ignore_ascii_case("Editor")));
            let in_editor =
                !NOT_IN_EDITOR.contains(&kind.as_str()) && editor_targets != Some(false);
            if let Some((key, platforms)) = &exclusion {
                findings.push(Finding::AllowList {
                    module: Some(module_name.to_string()),
                    key: key.clone(),
                    platforms: platforms.clone(),
                });
            }
            let source = build_rules.iter().any(|p| {
                build_rules_module(p).is_some_and(|m| m.eq_ignore_ascii_case(module_name))
            });
            modules.push(Module {
                name: module_name.to_string(),
                kind,
                allowed_on_mac: exclusion.is_none(),
                in_editor,
                mac_binary: binaries.get(module_name).cloned(),
                source,
            });
        }
    }

    let editor_modules: Vec<&Module> = modules.iter().filter(|m| m.in_editor).collect();
    let plugin_excluded = findings
        .iter()
        .any(|f| matches!(f, Finding::AllowList { module: None, .. }));
    let working = editor_modules
        .iter()
        .filter(|m| m.allowed_on_mac && m.mac_binary.is_some())
        .count();
    let missing: Vec<String> = editor_modules
        .iter()
        .filter(|m| m.mac_binary.is_none())
        .map(|m| m.name.clone())
        .collect();
    let has_source = !build_rules.is_empty();

    let quick = if editor_modules.is_empty() && !plugin_excluded {
        Some(if modules.is_empty() {
            Verdict::ContentOnly
        } else {
            Verdict::Works
        })
    } else if !plugin_excluded && working == editor_modules.len() {
        Some(Verdict::Works)
    } else if !plugin_excluded
        && working > 0
        && editor_modules
            .iter()
            .all(|m| m.mac_binary.is_some() || !m.allowed_on_mac)
    {
        Some(Verdict::Partial)
    } else {
        None
    };

    if !missing.is_empty() && quick.is_none() {
        findings.push(Finding::NoMacBinaries { modules: missing });
    }
    if quick.is_none() || deep {
        if !has_source {
            findings.push(Finding::NoSource);
        } else {
            for module in modules.iter().filter(|m| !m.source && m.in_editor) {
                findings.push(Finding::ModuleSourceMissing {
                    module: module.name.clone(),
                });
            }
            findings.extend(windows_only_libraries(&dir));
            for rules in &build_rules {
                if let Some(line) = windows_only_build_rules(rules) {
                    findings.push(Finding::WindowsOnlyBuildRules {
                        file: relative(&dir, rules),
                        line,
                    });
                }
            }
            for source in source_files(&dir.join("Source")) {
                let Ok(text) = std::fs::read(&source) else {
                    continue;
                };
                let text = String::from_utf8_lossy(&text);
                for hit in winapi::scan(&text) {
                    let file = relative(&dir, &source);
                    findings.push(match hit {
                        winapi::Hit::Api { line, what } => Finding::WindowsApi { file, line, what },
                        winapi::Hit::OnlyBlock { line } => Finding::WindowsOnlyBlock { file, line },
                    });
                }
            }
        }
    }

    let verdict = match quick {
        Some(v) => v,
        None if !has_source => Verdict::NoSource,
        None if findings.iter().any(Finding::blocks_build) => Verdict::NeedsPorting,
        None if findings
            .iter()
            .any(|f| matches!(f, Finding::AllowList { .. })) =>
        {
            if findings
                .iter()
                .any(|f| matches!(f, Finding::NoMacBinaries { .. }))
            {
                Verdict::PatchAndRebuild
            } else {
                Verdict::PatchOnly
            }
        }
        None => Verdict::Rebuild,
    };
    Ok(Plugin {
        name,
        dir,
        file: file.to_path_buf(),
        verdict,
        modules,
        findings,
    })
}

pub fn relative(base: &Path, path: &Path) -> String {
    path.strip_prefix(base)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Module name to Mac editor binary (relative to the plugin), from
/// `Binaries/Mac/UnrealEditor.modules`, or failing that from file names.
fn mac_binaries(dir: &Path) -> std::collections::HashMap<String, String> {
    let mac = dir.join("Binaries/Mac");
    let mut found = std::collections::HashMap::new();
    if let Ok(text) = std::fs::read_to_string(mac.join("UnrealEditor.modules"))
        && let Ok(json) = crate::jsonc::parse(&text)
        && let Some(map) = json.get("Modules").and_then(|m| m.as_object())
    {
        for (module, file) in map {
            if let Some(file) = file.as_str()
                && mac.join(file).is_file()
            {
                found.insert(module.clone(), format!("Binaries/Mac/{file}"));
            }
        }
        return found;
    }
    for entry in std::fs::read_dir(&mac)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
    {
        let file = entry.file_name().to_string_lossy().into_owned();
        let module = file
            .strip_suffix(".dylib")
            .map(|f| f.strip_prefix("lib").unwrap_or(f))
            .and_then(|f| f.strip_prefix("UnrealEditor-"));
        if let Some(module) = module {
            // UnrealEditor-Module-Mac-DebugGame.dylib and the like: first part only.
            let module = module.split('-').next().unwrap_or(module);
            found.insert(module.to_string(), format!("Binaries/Mac/{file}"));
        }
    }
    found
}

/// Every `*.Build.cs` under `source`.
pub fn build_rules_in(source: &Path) -> Vec<PathBuf> {
    walk(source)
        .into_iter()
        .filter(|p| {
            p.to_string_lossy()
                .to_ascii_lowercase()
                .ends_with(".build.cs")
        })
        .collect()
}

/// `Foo` for `.../Foo.Build.cs`.
pub fn build_rules_module(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_string_lossy().into_owned();
    let lower = name.to_ascii_lowercase();
    lower
        .strip_suffix(".build.cs")
        .map(|stem| name[..stem.len()].to_string())
}

/// Files under `dir`, skipping build output.
fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
        {
            let path = entry.path();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if !name.starts_with('.') && name != "Intermediate" {
                    stack.push(path);
                }
            } else {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

fn source_files(source: &Path) -> Vec<PathBuf> {
    walk(source)
        .into_iter()
        .filter(|p| {
            p.extension()
                .is_some_and(|e| SOURCE_EXTENSIONS.iter().any(|x| e.eq_ignore_ascii_case(x)))
        })
        .filter(|p| p.metadata().is_ok_and(|m| m.len() <= MAX_SOURCE_BYTES))
        .collect()
}

const WINDOWS_LIBRARIES: [&str; 2] = ["lib", "dll"];
const MAC_LIBRARIES: [&str; 3] = ["a", "dylib", "tbd"];

/// Prebuilt library folders with Windows libraries and no Mac ones. Folders
/// are grouped by name, so `Source/ThirdParty/Foo` and `Binaries/ThirdParty/Foo`
/// count as one library.
fn windows_only_libraries(dir: &Path) -> Vec<Finding> {
    use std::collections::BTreeMap;
    // Library name -> (Windows files, has Mac files, folder shown).
    let mut groups: BTreeMap<String, (Vec<String>, bool, String)> = BTreeMap::new();
    let roots = [
        dir.join("Source"),
        dir.join("Binaries/ThirdParty"),
        dir.join("ThirdParty"),
    ];
    for root in roots.iter().filter(|r| r.is_dir()) {
        for file in walk(root) {
            let ext = file
                .extension()
                .map(|e| e.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_default();
            let in_framework = file.ancestors().any(|a| {
                a.extension()
                    .is_some_and(|e| e == "framework" || e == "xcframework")
            });
            let windows = WINDOWS_LIBRARIES.contains(&ext.as_str());
            let mac = MAC_LIBRARIES.contains(&ext.as_str()) || in_framework;
            if !windows && !mac {
                continue;
            }
            let Some(folder) = library_folder(root, &file) else {
                continue;
            };
            let key = folder
                .file_name()
                .map(|n| n.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_default();
            let entry = groups
                .entry(key)
                .or_insert_with(|| (Vec::new(), false, relative(dir, &folder)));
            if windows {
                entry.0.push(relative(dir, &file));
            }
            entry.1 |= mac;
        }
    }
    groups
        .into_values()
        .filter(|(windows, mac, _)| !windows.is_empty() && !mac)
        .map(|(files, _, folder)| Finding::WindowsOnlyLibrary { folder, files })
        .collect()
}

/// The folder a library belongs to: its module folder (the nearest one with
/// build rules) under Source, or the first folder under a ThirdParty root.
fn library_folder(root: &Path, file: &Path) -> Option<PathBuf> {
    if root.ends_with("Source") {
        file.ancestors()
            .skip(1)
            .take_while(|a| a.starts_with(root) && *a != root)
            .find(|a| !build_rules_here(a).is_empty())
            .map(Path::to_path_buf)
    } else {
        let rest = file.strip_prefix(root).ok()?;
        let first = rest.components().next()?;
        Some(root.join(first))
    }
}

fn build_rules_here(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.to_string_lossy()
                .to_ascii_lowercase()
                .ends_with(".build.cs")
        })
        .collect()
}

/// The line of the first Windows platform check in build rules that never
/// mention Mac, if that's what these rules are.
fn windows_only_build_rules(path: &Path) -> Option<usize> {
    let text = std::fs::read_to_string(path).ok()?;
    let code = winapi::strip_comments(&text);
    let windows_markers = [
        "UnrealTargetPlatform.Win64",
        "UnrealTargetPlatform.Win32",
        "UnrealPlatformGroup.Windows",
        "UnrealPlatformGroup.Microsoft",
    ];
    let mac_markers = [
        "UnrealTargetPlatform.Mac",
        "UnrealPlatformGroup.Apple",
        "\"Mac\"",
        "Mac/",
    ];
    if mac_markers.iter().any(|m| code.contains(m)) {
        return None;
    }
    // Windows-only settings are common and harmless (a define, a system
    // library); it's Windows libraries or refusing other platforms that block.
    let blocking = [".lib\"", ".dll\"", "PublicDelayLoadDLLs", "BuildException"];
    if !blocking
        .iter()
        .any(|m| code.contains(m) || text.contains(m))
    {
        return None;
    }
    code.lines()
        .position(|l| windows_markers.iter().any(|m| l.contains(m)))
        .map(|i| i + 1)
}
