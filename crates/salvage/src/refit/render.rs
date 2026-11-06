//! Human-readable output for `salvage refit`.

use std::collections::BTreeMap;
use std::path::Path;

use super::package::PackageCheck;
use super::scan::{Finding, Plugin, Verdict};
use crate::ui::{BAD, BOLD, DIM, GOOD, WARN, paint};

const ORDER: [Verdict; 7] = [
    Verdict::Works,
    Verdict::ContentOnly,
    Verdict::Partial,
    Verdict::Rebuild,
    Verdict::PatchAndRebuild,
    Verdict::NeedsPorting,
    Verdict::NoSource,
];

fn mark(verdict: &Verdict) -> String {
    match verdict {
        Verdict::Works | Verdict::ContentOnly => paint(GOOD, "✓"),
        Verdict::Partial => paint(GOOD, "◐"),
        Verdict::Rebuild | Verdict::PatchAndRebuild => paint(WARN, "~"),
        Verdict::NeedsPorting | Verdict::NoSource => paint(BAD, "✗"),
    }
}

/// The summary, one line per plugin that doesn't simply work, and with
/// `details` what stands in each one's way.
pub fn scan(plugins: &[Plugin], roots: &[&Path], details: bool, all: bool) -> String {
    let mut out = String::new();
    let where_ = roots
        .iter()
        .map(|r| r.display().to_string())
        .collect::<Vec<_>>()
        .join(", ");
    out.push_str(&format!(
        "\n  {}  {} plugin{} in {where_}\n\n",
        paint(BOLD, "salvage refit scan"),
        plugins.len(),
        if plugins.len() == 1 { "" } else { "s" }
    ));
    for verdict in ORDER {
        let count = plugins.iter().filter(|p| p.verdict == verdict).count();
        if count > 0 {
            out.push_str(&format!(
                "    {} {count:>4}  {}\n",
                mark(&verdict),
                verdict.describe()
            ));
        }
    }
    let shown: Vec<&Plugin> = plugins
        .iter()
        .filter(|p| {
            all || !matches!(
                p.verdict,
                Verdict::Works | Verdict::ContentOnly | Verdict::Partial
            )
        })
        .collect();
    if shown.is_empty() {
        out.push('\n');
        return out;
    }
    out.push('\n');
    let width = shown
        .iter()
        .map(|p| p.name.len())
        .max()
        .unwrap_or(0)
        .min(36);
    for plugin in &shown {
        let root = roots.iter().find(|r| plugin.dir.starts_with(r));
        let place = root
            .and_then(|r| plugin.dir.strip_prefix(r).ok())
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| plugin.dir.display().to_string());
        out.push_str(&format!(
            "  {} {:width$}  {}  {}\n",
            mark(&plugin.verdict),
            plugin.name,
            plugin.verdict.describe(),
            paint(DIM, &place),
        ));
        if details {
            out.push_str(&findings(plugin));
        }
    }
    if !details {
        out.push_str(&format!(
            "\n  {}\n",
            paint(
                DIM,
                "Scan one plugin's folder, or add --details, to see what stands in each one's way."
            )
        ));
    }
    out.push('\n');
    out
}

/// What stands in a plugin's way, grouped, and what to do next.
pub fn findings(plugin: &Plugin) -> String {
    let mut out = String::new();
    let mut line = |text: String| out.push_str(&format!("      {text}\n"));
    let mut api_by_file: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    let mut blocks: Vec<String> = Vec::new();
    for finding in &plugin.findings {
        match finding {
            Finding::AllowList {
                module,
                key,
                platforms,
            } => line(format!(
                "{}: {key} is {}",
                module.as_deref().unwrap_or("the plugin"),
                if platforms.is_empty() {
                    "empty".to_string()
                } else {
                    platforms.join(", ")
                }
            )),
            Finding::NoMacBinaries { modules } => {
                line(format!("No Mac binaries for {}", modules.join(", ")))
            }
            Finding::NoSource => line("No Source folder: it can't be rebuilt".to_string()),
            Finding::ModuleSourceMissing { module } => line(format!(
                "{module}: listed, but its build rules aren't in the plugin"
            )),
            Finding::WindowsOnlyLibrary { folder, files } => line(format!(
                "{folder}: prebuilt for Windows only ({}), no .a, .dylib or .framework",
                summarize(files, 3)
            )),
            Finding::WindowsOnlyBuildRules { file, line: at } => line(format!(
                "{file}:{at}: build rules link Windows libraries or refuse other platforms, with no Mac branch"
            )),
            Finding::WindowsApi {
                file,
                line: at,
                what,
            } => api_by_file
                .entry(file)
                .or_default()
                .push(format!("{at} {what}")),
            Finding::WindowsOnlyBlock { file, line: at } => blocks.push(format!("{file}:{at}")),
        }
    }
    if !api_by_file.is_empty() {
        let total: usize = api_by_file.values().map(Vec::len).sum();
        line(format!(
            "Windows API outside any Windows-only block: {total} in {} file{}",
            api_by_file.len(),
            if api_by_file.len() == 1 { "" } else { "s" }
        ));
        for (file, hits) in api_by_file.iter().take(8) {
            line(format!("  {file}: {}", summarize(hits, 4)));
        }
        if api_by_file.len() > 8 {
            line(format!("  and {} more files", api_by_file.len() - 8));
        }
    }
    if !blocks.is_empty() {
        line(format!(
            "Windows-only #if blocks with no #else (they compile on Mac, but do nothing there): {}",
            summarize(&blocks, 4)
        ));
    }
    let next = match plugin.verdict {
        Verdict::PatchAndRebuild => Some("Next: salvage refit patch, then salvage refit build."),
        Verdict::Rebuild => Some("Next: salvage refit build."),
        Verdict::NeedsPorting => {
            Some("Next: port what's listed above, then salvage refit patch and build.")
        }
        Verdict::NoSource => Some("Next: ask the vendor for a Mac build or the source."),
        _ => None,
    };
    if let Some(next) = next {
        line(paint(DIM, next));
    }
    out
}

fn summarize(items: &[String], show: usize) -> String {
    let mut text = items
        .iter()
        .take(show)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    if items.len() > show {
        text.push_str(&format!(", and {} more", items.len() - show));
    }
    text
}

pub fn package(check: &PackageCheck) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "\n  {}  {}\n",
        paint(BOLD, "salvage refit check"),
        check.package.display()
    ));
    if let Some(compiled) = check.compiled {
        out.push_str(&format!("  {compiled} files compiled\n"));
    }
    for module in &check.modules {
        let mark = if module.is_module {
            paint(GOOD, "✓")
        } else {
            paint(BAD, "✗")
        };
        let detail = match &module.binary {
            Some(binary) if module.is_module => {
                format!("{binary} ({})", module.architectures.join(", "))
            }
            Some(binary) => format!("{binary} isn't an Unreal module binary"),
            None => "no Mac binary".to_string(),
        };
        out.push_str(&format!(
            "  {mark} {}  {}\n",
            module.name,
            paint(DIM, &detail)
        ));
    }
    if check.ok() {
        out.push_str(&format!(
            "\n  {}\n\n",
            paint(GOOD, "Mac binaries for every module.")
        ));
    } else {
        for problem in &check.problems {
            out.push_str(&format!("  {} {problem}\n", paint(BAD, "✗")));
        }
        out.push('\n');
    }
    out
}
