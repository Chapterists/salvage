//! Finding each image's symbols by UUID, and asking atos (or UnrealAtoS) for
//! function, file and line. Everything happens on this machine.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

use super::report::{Image, normalize_uuid};
use crate::probe::Probe;

/// How an image's symbols were found, or why they weren't.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum Symbols {
    /// A dSYM with the same UUID: functions, files and lines.
    Dsym { path: PathBuf },
    /// The binary itself, same UUID: functions, plus files and lines when the
    /// build's object files are still there.
    Binary { path: PathBuf },
    /// An Unreal .udebugsymbols file next to the binary, read with UnrealAtoS.
    UnrealSymbols { path: PathBuf, binary: PathBuf },
    /// Files with this name exist, but they're from a different build.
    Mismatch { found: Vec<(PathBuf, String)> },
    /// No dSYM or binary with this name or UUID on this machine.
    Missing,
    /// A macOS library read from this machine's shared cache: the crash
    /// happened on this same macOS build, so the cache matches.
    SharedCache { path: PathBuf },
    /// A macOS library that can't be matched (another macOS build), so only
    /// the names the report itself gives are used.
    System,
    /// The report gives no UUID for this image, so nothing can be matched.
    NoUuid,
}

impl Symbols {
    pub fn file(&self) -> Option<&Path> {
        match self {
            Symbols::Dsym { path } | Symbols::Binary { path } | Symbols::SharedCache { path } => {
                Some(path)
            }
            Symbols::UnrealSymbols { binary, .. } => Some(binary),
            _ => None,
        }
    }

    pub fn describe(&self) -> String {
        match self {
            Symbols::Dsym { path } => format!("dSYM {}", path.display()),
            Symbols::Binary { path } => format!("binary {}", path.display()),
            Symbols::UnrealSymbols { path, .. } => format!("Unreal symbols {}", path.display()),
            Symbols::Mismatch { found } => format!(
                "no match: {} from a different build (UUID {})",
                found
                    .iter()
                    .map(|(p, _)| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", "),
                found
                    .iter()
                    .map(|(_, u)| u.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Symbols::Missing => "no dSYM or binary with this UUID on this machine".to_string(),
            Symbols::SharedCache { .. } => {
                "macOS library, from this Mac's shared cache (same macOS build)".to_string()
            }
            Symbols::System => {
                "macOS library from another macOS build (names from the report, if any)".to_string()
            }
            Symbols::NoUuid => "the report gives no UUID for it".to_string(),
        }
    }
}

pub fn is_system(image: &Image) -> bool {
    image.path.as_ref().is_some_and(|p| {
        let p = p.to_string_lossy();
        p.starts_with("/System/") || p.starts_with("/usr/lib/") || p.starts_with("/Library/Apple/")
    })
}

/// UUIDs (and architectures) of a Mach-O file or dSYM bundle, from `dwarfdump --uuid`.
pub fn uuids_of(probe: &dyn Probe, file: &Path) -> Vec<(String, String)> {
    let Some(path) = file.to_str() else {
        return Vec::new();
    };
    let Some(out) = probe.run("dwarfdump", &["--uuid", path]) else {
        return Vec::new();
    };
    parse_dwarfdump_uuids(&out.stdout)
}

/// `UUID: 2423AB55-DD62-3A34-B8C8-1D496C0C7BE4 (arm64) /path` per line.
pub fn parse_dwarfdump_uuids(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| {
            let rest = line.trim().strip_prefix("UUID: ")?;
            let (uuid, rest) = rest.split_once(' ')?;
            let arch = rest
                .trim()
                .strip_prefix('(')?
                .split(')')
                .next()?
                .to_string();
            Some((normalize_uuid(uuid)?, arch))
        })
        .collect()
}

/// Where to look for an image's dSYM or binary, besides the path in the report.
#[derive(Clone, Debug, Default)]
pub struct SearchPaths {
    /// Folders searched (a few levels deep) for `<name>` and `<name>.dSYM`.
    pub roots: Vec<PathBuf>,
}

fn find_named(root: &Path, names: &[String], depth: usize, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let file = entry.file_name().to_string_lossy().into_owned();
        if names.contains(&file) {
            out.push(path.clone());
        }
        let is_dir = entry.file_type().is_ok_and(|t| t.is_dir());
        if is_dir
            && depth > 0
            && !file.ends_with(".dSYM")
            && !file.starts_with('.')
            && file != "Intermediate"
            && file != "Content"
        {
            find_named(&path, names, depth - 1, out);
        }
    }
}

/// The folder before the first `*` in a redacted path, if it has one and
/// isn't the root.
pub fn redacted_prefix(path: &Path) -> Option<PathBuf> {
    let text = path.to_string_lossy();
    let star = text.find("/*")?;
    let prefix = &text[..star];
    (prefix.matches('/').count() >= 2).then(|| PathBuf::from(prefix))
}

/// Finds symbols for one image: dSYMs Spotlight knows by UUID, then dSYMs and
/// binaries by name next to the report's path and under the search roots.
pub fn locate(probe: &dyn Probe, image: &Image, search: &SearchPaths, same_os: bool) -> Symbols {
    let Some(uuid) = image.uuid.as_deref() else {
        return if is_system(image) {
            Symbols::System
        } else {
            Symbols::NoUuid
        };
    };
    if is_system(image) {
        // atos reads macOS libraries from the shared cache, but only this
        // machine's: they match when the crash came from the same macOS build.
        if let Some(path) = &image.path {
            let found = uuids_of(probe, path);
            if found.iter().any(|(u, _)| u == uuid) {
                return Symbols::Binary { path: path.clone() };
            }
            // Libraries that only live in the shared cache can't be read for
            // a UUID; the cache is right exactly when the macOS build is.
            if found.is_empty() && same_os {
                return Symbols::SharedCache { path: path.clone() };
            }
        }
        return Symbols::System;
    }
    let matches = |file: &Path| uuids_of(probe, file).iter().any(|(u, _)| u == uuid);

    // Spotlight indexes dSYM bundles by UUID.
    if let Some(out) = probe.run(
        "mdfind",
        &[&format!("com_apple_xcode_dsym_uuids == {uuid}")],
    ) {
        for line in out.stdout.lines().map(str::trim).filter(|l| !l.is_empty()) {
            let path = PathBuf::from(line);
            if matches(&path) {
                return Symbols::Dsym { path };
            }
        }
    }
    let name = image
        .path
        .as_ref()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| image.name.clone());
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(path) = &image.path {
        candidates.push(PathBuf::from(format!("{}.dSYM", path.display())));
        candidates.push(path.clone());
    }
    let names = vec![format!("{name}.dSYM"), name.clone()];
    for root in &search.roots {
        find_named(root, &names, 4, &mut candidates);
    }
    // macOS redacts part of the path in .ips reports (`/private/tmp/*/Game`);
    // look under the part it kept.
    if let Some(prefix) = image.path.as_ref().and_then(|p| redacted_prefix(p)) {
        find_named(&prefix, &names, 8, &mut candidates);
    }
    // dSYMs first: they carry files and lines.
    candidates.sort_by_key(|p| !p.to_string_lossy().ends_with(".dSYM"));
    candidates.dedup();
    let mut mismatched = Vec::new();
    for path in candidates.into_iter().filter(|p| p.exists()) {
        let found = uuids_of(probe, &path);
        if found.iter().any(|(u, _)| u == uuid) {
            if path.to_string_lossy().ends_with(".dSYM") {
                return Symbols::Dsym { path };
            }
            let unreal = PathBuf::from(format!(
                "{}.udebugsymbols",
                path.with_extension("").display()
            ));
            if unreal.is_file() {
                return Symbols::UnrealSymbols {
                    path: unreal,
                    binary: path,
                };
            }
            return Symbols::Binary { path };
        }
        for (u, _) in found {
            if !mismatched
                .iter()
                .any(|(p, x): &(PathBuf, String)| *p == path && *x == u)
            {
                mismatched.push((path.clone(), u));
            }
        }
    }
    if mismatched.is_empty() {
        Symbols::Missing
    } else {
        Symbols::Mismatch { found: mismatched }
    }
}

/// A symbolicated frame.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Resolved {
    pub function: String,
    pub file: Option<String>,
    pub line: Option<u32>,
}

/// One line of atos output: `Func (in Module) (File.cpp:12)`,
/// `Func (in Module) + 40`, or `0x1234 (in Module)` when it found nothing.
pub fn parse_atos_line(line: &str) -> Option<Resolved> {
    let line = line.trim();
    if line.is_empty() || line.starts_with("0x") {
        return None;
    }
    let (function, rest) = match line.find(" (in ") {
        Some(at) => (&line[..at], &line[at..]),
        None => (line, ""),
    };
    let (file, number) = match (rest.rfind('('), rest.ends_with(')')) {
        (Some(open), true) if !rest[open..].starts_with("(in ") => {
            let inner = &rest[open + 1..rest.len() - 1];
            match inner.rsplit_once(':') {
                Some((file, line)) => (Some(file.to_string()), line.parse::<u32>().ok()),
                None => (None, None),
            }
        }
        _ => (None, None),
    };
    Some(Resolved {
        function: function.to_string(),
        file,
        line: number,
    })
}

/// Symbolicates addresses in one image. Return addresses (every frame but a
/// thread's first) point after their call, so they're looked up one byte back.
pub fn symbolicate(
    probe: &dyn Probe,
    symbols: &Symbols,
    image: &Image,
    lookups: &[(u64, bool)],
) -> HashMap<u64, Resolved> {
    let mut resolved = HashMap::new();
    let Some(file) = symbols.file().and_then(Path::to_str) else {
        return resolved;
    };
    let arch = image.arch.clone().unwrap_or_else(|| "arm64".to_string());
    let base = format!("{:#x}", image.base);
    let targets: Vec<String> = lookups
        .iter()
        .map(|(address, is_return)| {
            format!("{:#x}", if *is_return { address - 1 } else { *address })
        })
        .collect();
    if let Symbols::UnrealSymbols { path, .. } = symbols {
        // UnrealAtoS answers one address at a time.
        let tool = unreal_atos(file);
        for ((address, _), target) in lookups.iter().zip(&targets) {
            let symbols_path = path.to_string_lossy().into_owned();
            if let Some(tool) = &tool
                && let Some(out) =
                    probe.run(tool, &[file, "-d", &symbols_path, "-l", &base, target])
                && let Some(r) = out.stdout.lines().find_map(parse_atos_line)
            {
                resolved.insert(*address, r);
            }
        }
        return resolved;
    }
    for chunk in lookups.iter().zip(&targets).collect::<Vec<_>>().chunks(200) {
        let mut args: Vec<&str> = vec!["-o", file, "-arch", &arch, "-l", &base];
        args.extend(chunk.iter().map(|(_, t)| t.as_str()));
        let Some(out) = probe.run("atos", &args) else {
            continue;
        };
        for (((address, _), _), line) in chunk.iter().zip(out.stdout.lines()) {
            if let Some(r) = parse_atos_line(line) {
                resolved.insert(*address, r);
            }
        }
    }
    resolved
}

/// The engine's UnrealAtoS next to a binary inside an engine install, if any.
fn unreal_atos(binary: &str) -> Option<String> {
    Path::new(binary)
        .ancestors()
        .find(|a| a.join("Engine/Binaries/Mac/UnrealAtoS").is_file())
        .map(|a| {
            a.join("Engine/Binaries/Mac/UnrealAtoS")
                .to_string_lossy()
                .into_owned()
        })
}
