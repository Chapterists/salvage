//! Checks a plugin package that BuildPlugin wrote: every module the editor
//! should load on Mac has a Mac binary that really is an Unreal module.
//!
//! The trap this catches: a plugin whose allow lists leave Mac out "builds"
//! successfully on a Mac in about a second, compiling nothing and writing no
//! binaries at all.

use std::path::{Path, PathBuf};

use serde::Serialize;

use super::scan;

/// Every Unreal module binary exports this symbol.
const MODULE_MARKER: &[u8] = b"_ThisIsAnUnrealEngineModule";

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ModuleCheck {
    pub name: String,
    /// The Mac editor binary, relative to the package.
    pub binary: Option<String>,
    /// CPU architectures in it: "arm64", "x86_64".
    pub architectures: Vec<String>,
    /// It exports Unreal's module marker.
    pub is_module: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PackageCheck {
    pub package: PathBuf,
    pub modules: Vec<ModuleCheck>,
    /// Compile actions in the build log, when a log was given.
    pub compiled: Option<usize>,
    /// Empty when the package is good.
    pub problems: Vec<String>,
}

impl PackageCheck {
    pub fn ok(&self) -> bool {
        self.problems.is_empty()
    }
}

pub fn check(package: &Path, log: Option<&str>) -> Result<PackageCheck, String> {
    let uplugin =
        scan::uplugin_in(package).ok_or_else(|| format!("no .uplugin in {}", package.display()))?;
    let plugin = scan::scan(&uplugin, false)?;
    let mut problems = Vec::new();
    let compiled = log.map(compile_actions);

    let expected: Vec<&scan::Module> = plugin.modules.iter().filter(|m| m.in_editor).collect();
    let on_mac: Vec<&&scan::Module> = expected.iter().filter(|m| m.allowed_on_mac).collect();
    let plugin_excluded = plugin
        .findings
        .iter()
        .any(|f| matches!(f, scan::Finding::AllowList { module: None, .. }));
    if !expected.is_empty() && (on_mac.is_empty() || plugin_excluded) {
        problems.push(
            "the plugin's allow lists leave Mac out, so nothing was built for Mac \
             (run salvage refit patch first)"
                .to_string(),
        );
    }
    if let Some(log) = log
        && built_for_no_target(log)
    {
        problems.push("BuildPlugin built for no target platform".to_string());
    }
    if compiled == Some(0) && !on_mac.is_empty() {
        problems.push("the build log shows no files compiled".to_string());
    }

    let mut modules = Vec::new();
    for module in &on_mac {
        let binary = module.mac_binary.clone();
        let bytes = binary
            .as_ref()
            .and_then(|b| std::fs::read(package.join(b)).ok())
            .unwrap_or_default();
        let architectures = macho_architectures(&bytes);
        let is_module = !architectures.is_empty() && contains(&bytes, MODULE_MARKER);
        match (&binary, is_module) {
            (None, _) => problems.push(format!("{}: no Mac binary", module.name)),
            (Some(b), false) => problems.push(format!(
                "{}: {b} isn't an Unreal module binary",
                module.name
            )),
            _ => {}
        }
        modules.push(ModuleCheck {
            name: module.name.clone(),
            binary,
            architectures,
            is_module,
        });
    }
    problems.dedup();
    Ok(PackageCheck {
        package: package.to_path_buf(),
        modules,
        compiled,
        problems,
    })
}

/// `[12/74] Compile [Apple] Foo.cpp` lines in a UnrealBuildTool log.
pub fn compile_actions(log: &str) -> usize {
    log.lines()
        .filter(|l| {
            let l = l.trim_start();
            l.starts_with('[')
                && l.split_once("] ")
                    .is_some_and(|(_, rest)| rest.starts_with("Compile "))
        })
        .count()
}

/// BuildPlugin says which target platforms it builds for. When the allow
/// lists leave Mac out, that list comes out empty and it builds nothing.
fn built_for_no_target(log: &str) -> bool {
    let targets: Vec<&str> = log
        .lines()
        .filter_map(|l| {
            l.trim()
                .strip_prefix("Building plugin for target platforms:")
        })
        .map(str::trim)
        .collect();
    !targets.is_empty() && targets.iter().all(|t| t.is_empty())
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// The architectures in a Mach-O file (thin or universal); empty if it isn't one.
pub fn macho_architectures(bytes: &[u8]) -> Vec<String> {
    let be = |at: usize| -> Option<u32> {
        bytes
            .get(at..at + 4)
            .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    };
    let le = |at: usize| -> Option<u32> {
        bytes
            .get(at..at + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    };
    let name = |cpu: u32| match cpu {
        0x0100_000c => "arm64".to_string(),
        0x0100_0007 => "x86_64".to_string(),
        other => format!("cpu {other:#x}"),
    };
    match be(0) {
        // Universal: big-endian header, then (cputype, subtype, offset, size, align) each.
        Some(0xcafe_babe) | Some(0xcafe_babf) => {
            let wide = be(0) == Some(0xcafe_babf);
            let count = be(4).unwrap_or(0).min(16) as usize;
            let stride = if wide { 32 } else { 20 };
            (0..count)
                .filter_map(|i| be(8 + i * stride).map(name))
                .collect()
        }
        _ => match le(0) {
            Some(0xfeed_facf) => le(4).map(name).into_iter().collect(),
            _ => Vec::new(),
        },
    }
}

/// The RunUAT command that builds a plugin package for Mac.
pub fn build_command(engine_root: &Path, uplugin: &Path, out: &Path) -> (PathBuf, Vec<String>) {
    let uat = engine_root.join("Engine/Build/BatchFiles/RunUAT.sh");
    let args = vec![
        "BuildPlugin".to_string(),
        format!("-Plugin={}", uplugin.display()),
        format!("-Package={}", out.display()),
        "-TargetPlatforms=Mac".to_string(),
        "-unattended".to_string(),
    ];
    (uat, args)
}
