//! `salvage clangd`: a fast IDE setup for Unreal C++ without Xcode's indexer.
//!
//! Runs UnrealBuildTool's GenerateClangDatabase mode for a project's target,
//! which writes `compile_commands.json` into the project, then adds a
//! `.clangd` tuned for Unreal code and, if asked, editor settings that start
//! clangd with sensible flags. `--refresh` skips the (slow) UnrealBuildTool
//! run when nothing that shapes the database has changed.

mod fingerprint;
mod generated;

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::engine::Engine;
use crate::probe::Os;
use crate::project::Project;

pub use fingerprint::{Fingerprint, STAMP_FILE};
pub use generated::{GeneratedHeaders, check_generated_headers};

pub const DATABASE: &str = "compile_commands.json";
pub const CLANGD_FILE: &str = ".clangd";
pub const NEOVIM_FILE: &str = ".nvim.lua";
pub const VSCODE_FILE: &str = ".vscode/settings.json";

/// Folders that never hold source we want to scan.
pub(crate) const SKIP_DIRS: [&str; 8] = [
    "Intermediate",
    "Binaries",
    "Saved",
    "DerivedDataCache",
    "Content",
    ".git",
    ".vs",
    "node_modules",
];

/// The UnrealBuildTool platform name for the machine we're on.
pub fn host_platform(os: Os) -> Option<&'static str> {
    match os {
        Os::Macos => Some("Mac"),
        Os::Linux => Some("Linux"),
        Os::Windows => Some("Win64"),
        Os::Other => None,
    }
}

/// The build targets a project defines (`Source/<Name>.Target.cs`), sorted.
pub fn targets(project: &Project) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(project.dir.join("Source")) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .filter_map(Result::ok)
        .filter_map(|e| {
            e.file_name()
                .to_string_lossy()
                .strip_suffix(".Target.cs")
                .map(str::to_string)
        })
        .collect();
    names.sort();
    names
}

/// Picks the target to index: the one asked for, else the editor target, else the game.
pub fn choose_target(project: &Project, requested: Option<&str>) -> Result<String, String> {
    let available = targets(project);
    if available.is_empty() {
        return Err(format!(
            "{} has no C++ targets (no Source/*.Target.cs). If it's Blueprint-only, add a C++ class from the editor first.",
            project.name
        ));
    }
    if let Some(requested) = requested {
        return available
            .iter()
            .find(|t| t.eq_ignore_ascii_case(requested))
            .cloned()
            .ok_or_else(|| {
                format!(
                    "{} has no target named {requested}; it has {}",
                    project.name,
                    available.join(", ")
                )
            });
    }
    let editor = format!("{}Editor", project.name);
    let preferred = [editor.as_str(), project.name.as_str()];
    Ok(preferred
        .iter()
        .find_map(|want| available.iter().find(|t| t.eq_ignore_ascii_case(want)))
        .or_else(|| available.iter().find(|t| t.ends_with("Editor")))
        .unwrap_or(&available[0])
        .clone())
}

/// The UnrealBuildTool invocation that writes the database into the project.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UbtCommand {
    pub program: PathBuf,
    pub args: Vec<String>,
}

impl UbtCommand {
    pub fn new(
        engine: &Engine,
        project: &Project,
        target: &str,
        platform: &str,
        configuration: &str,
    ) -> UbtCommand {
        let script = if platform == "Win64" {
            "RunUBT.bat"
        } else {
            "RunUBT.sh"
        };
        let absolute = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
        UbtCommand {
            program: engine.dir().join("Build/BatchFiles").join(script),
            args: vec![
                target.to_string(),
                platform.to_string(),
                configuration.to_string(),
                format!("-Project={}", absolute(&project.file).display()),
                "-Mode=GenerateClangDatabase".to_string(),
                format!("-OutputDir={}", absolute(&project.dir).display()),
                format!("-OutputFilename={DATABASE}"),
            ],
        }
    }

    /// The command as you'd type it in a shell.
    pub fn display(&self) -> String {
        std::iter::once(self.program.display().to_string())
            .chain(self.args.iter().cloned())
            .map(|part| {
                if part.contains(' ') {
                    format!("\"{part}\"")
                } else {
                    part
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// `.clangd` for Unreal: warnings stay warnings, include checks that Unreal's
/// style trips over are off, and generated headers are left alone.
pub fn clangd_config() -> &'static str {
    r#"# Written by `salvage clangd` for Unreal Engine C++. Edit freely: salvage
# backs this file up before it ever replaces it.
CompileFlags:
  # Unreal builds with warnings as errors. While editing, keep them warnings.
  Remove: [-Werror*]
  # Flags clangd's own clang may not know from the toolchain that built the database.
  Add: [-Wno-unknown-warning-option, -Wno-unused-command-line-argument]
Diagnostics:
  # Unreal's include conventions (generated headers, module includes) make
  # these checks noisy and often wrong.
  UnusedIncludes: None
  MissingIncludes: None
Index:
  Background: Build
  # The standard library index costs memory and Unreal code rarely needs it.
  StandardLibrary: No
---
# Generated by UnrealHeaderTool; nothing to fix in here.
If:
  PathMatch: .*\.generated\.h
Diagnostics:
  Suppress: "*"
"#
}

/// Command-line flags for clangd itself, used in the editor settings.
pub fn clangd_args(os: Os) -> Vec<String> {
    let mut args = vec![
        "--background-index".to_string(),
        // Unreal headers need their includes in a particular order; let people add them.
        "--header-insertion=never".to_string(),
        "--completion-style=detailed".to_string(),
        // Keep preambles on disk instead of in memory; Unreal's are large.
        "--pch-storage=disk".to_string(),
    ];
    if os == Os::Macos {
        // Lets a non-Xcode clangd ask Xcode's compiler for its system include paths.
        args.push(
            "--query-driver=/Applications/Xcode*.app/Contents/Developer/Toolchains/**/clang*"
                .to_string(),
        );
    }
    args
}

/// VS Code settings: clangd with our flags, and Microsoft's IntelliSense off so
/// the two don't fight. Other settings are kept.
pub fn vscode_settings(existing: Option<&str>, args: &[String]) -> Result<String, String> {
    let mut settings = match existing.map(str::trim).filter(|t| !t.is_empty()) {
        Some(text) => match crate::jsonc::parse(text) {
            Ok(Value::Object(map)) => map,
            Ok(_) => {
                return Err(format!(
                    "{VSCODE_FILE} isn't a JSON object; leaving it alone"
                ));
            }
            Err(e) => {
                return Err(format!(
                    "{VSCODE_FILE} isn't valid JSON ({e}); leaving it alone"
                ));
            }
        },
        None => Map::new(),
    };
    settings.insert("clangd.arguments".into(), Value::from(args.to_vec()));
    settings.insert("C_Cpp.intelliSenseEngine".into(), Value::from("disabled"));
    let mut text =
        serde_json::to_string_pretty(&Value::Object(settings)).expect("settings serialize");
    text.push('\n');
    Ok(text)
}

/// A project-local Neovim config (read when `exrc` is on) that starts clangd with our flags.
pub fn neovim_config(args: &[String]) -> String {
    let quoted: Vec<String> = std::iter::once("clangd".to_string())
        .chain(args.iter().cloned())
        .map(|a| format!("\"{}\"", a.replace('\\', "\\\\").replace('"', "\\\"")))
        .collect();
    format!(
        r#"-- Written by `salvage clangd` for Unreal Engine C++.
-- Neovim reads this file when 'exrc' is on (Neovim 0.9+: `:set exrc`, then trust it).
local cmd = {{ {} }}
if vim.lsp.config then
  vim.lsp.config("clangd", {{ cmd = cmd }})
  vim.lsp.enable("clangd")
else
  local ok, lspconfig = pcall(require, "lspconfig")
  if ok then
    lspconfig.clangd.setup({{ cmd = cmd }})
  end
end
"#,
        quoted.join(", ")
    )
}

/// How many entries a compile database has, or None if it isn't one.
pub fn database_entries(path: &Path) -> Option<usize> {
    let text = std::fs::read_to_string(path).ok()?;
    match serde_json::from_str::<Value>(&text).ok()? {
        Value::Array(entries) => Some(entries.len()),
        _ => None,
    }
}

/// Every file under `dir` (skipping build output and content), sorted.
pub(crate) fn walk(dir: &Path, keep: &dyn Fn(&Path) -> bool) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                let name = entry.file_name();
                if !SKIP_DIRS.iter().any(|s| name == *s) {
                    pending.push(path);
                }
            } else if kind.is_file() && keep(&path) {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

/// Folders that hold a project's code: `Source/` and each plugin's `Source/`.
pub(crate) fn source_roots(project: &Project) -> Vec<PathBuf> {
    let mut roots = vec![project.dir.join("Source")];
    roots.extend(
        walk(&project.dir.join("Plugins"), &|p| {
            p.extension().is_some_and(|e| e == "uplugin")
        })
        .into_iter()
        .filter_map(|uplugin| uplugin.parent().map(|d| d.join("Source"))),
    );
    roots.into_iter().filter(|r| r.is_dir()).collect()
}

/// What to set up.
#[derive(Clone, Debug)]
pub struct Request {
    pub target: String,
    pub platform: String,
    pub configuration: String,
    pub vscode: bool,
    pub neovim: bool,
    /// Skip UnrealBuildTool when the fingerprint says nothing changed.
    pub refresh: bool,
    pub os: Os,
    /// Used to name backups.
    pub stamp: u64,
}

impl Request {
    fn build(&self) -> String {
        format!("{} {} {}", self.target, self.platform, self.configuration)
    }
}

/// What happened.
#[derive(Clone, Debug)]
pub struct Outcome {
    /// None when the database was regenerated; the reason it was skipped otherwise.
    pub skipped: Option<String>,
    /// Why it was regenerated, when `refresh` was asked for.
    pub reason: Option<String>,
    pub entries: Option<usize>,
    pub files: Vec<(PathBuf, crate::fsutil::Written)>,
    pub generated: GeneratedHeaders,
}

/// Generates the database (through `run_ubt`) and writes the config files.
pub fn setup(
    project: &Project,
    engine: &Engine,
    request: &Request,
    run_ubt: &mut dyn FnMut(&UbtCommand) -> Result<(), String>,
) -> Result<Outcome, String> {
    let database = project.dir.join(DATABASE);
    let now = Fingerprint::of(project, engine, &request.build());
    let previous = Fingerprint::load(project);
    let mut reason = None;
    let mut skipped = None;
    if request.refresh {
        reason = match (&previous, database.is_file()) {
            (_, false) => Some(format!("there's no {DATABASE} yet")),
            (None, true) => Some("there's no record of how it was made".to_string()),
            (Some(previous), true) => now.changes_since(previous),
        };
        if reason.is_none() {
            skipped = Some("nothing that shapes the database has changed".to_string());
        }
    }

    if skipped.is_none() {
        let command = UbtCommand::new(
            engine,
            project,
            &request.target,
            &request.platform,
            &request.configuration,
        );
        run_ubt(&command)?;
        if !database.is_file() {
            return Err(format!(
                "UnrealBuildTool finished but didn't write {}",
                database.display()
            ));
        }
        now.save(project)?;
    }

    let mut files = Vec::new();
    let mut write = |relative: &str, text: &str| -> Result<(), String> {
        let path = project.dir.join(relative);
        let result = crate::fsutil::write_with_backup(&path, text, request.stamp)?;
        files.push((path, result));
        Ok(())
    };
    write(CLANGD_FILE, clangd_config())?;
    let args = clangd_args(request.os);
    if request.vscode {
        let path = project.dir.join(VSCODE_FILE);
        let existing = std::fs::read_to_string(&path).ok();
        write(VSCODE_FILE, &vscode_settings(existing.as_deref(), &args)?)?;
    }
    if request.neovim {
        write(NEOVIM_FILE, &neovim_config(&args))?;
    }

    Ok(Outcome {
        skipped,
        reason,
        entries: database_entries(&database),
        files,
        generated: check_generated_headers(project),
    })
}

#[cfg(test)]
mod tests;
