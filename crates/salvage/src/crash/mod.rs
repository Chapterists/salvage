//! `salvage crash`: offline symbolication for Unreal crashes on a Mac.
//!
//! Reads an Unreal crash folder, a macOS .ips report or a text crash report;
//! matches each image to a dSYM or binary by UUID; asks atos for function,
//! file and line; and gives each crash a stable signature so repeats group
//! together. Nothing is uploaded: every step reads files on this machine.

pub mod bucket;
pub mod report;
pub mod symbols;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::probe::Probe;
pub use report::Report;
use symbols::{Resolved, SearchPaths, Symbols};

#[derive(Clone, Debug, Serialize)]
pub struct ImageResult {
    pub name: String,
    pub path: Option<PathBuf>,
    pub uuid: Option<String>,
    pub symbols: Symbols,
}

#[derive(Clone, Debug, Serialize)]
pub struct FrameOut {
    pub index: usize,
    pub module: String,
    pub address: String,
    pub function: Option<String>,
    pub file: Option<String>,
    pub line: Option<u32>,
    /// Where the function name came from: "atos", "unreal-atos" or "report".
    pub from: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ThreadOut {
    pub index: Option<usize>,
    pub name: Option<String>,
    pub crashed: bool,
    pub frames: Vec<FrameOut>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Analysis {
    pub source: report::Source,
    pub path: PathBuf,
    pub process: Option<String>,
    pub time: Option<String>,
    pub exception: Option<String>,
    pub unreal: Option<report::UnrealContext>,
    pub signature_id: String,
    pub signature: Vec<String>,
    pub threads: Vec<ThreadOut>,
    /// Only the images the shown frames use.
    pub images: Vec<ImageResult>,
}

/// Whether an image is part of an Unreal Engine install (not your project).
pub fn is_engine(image: &report::Image) -> bool {
    image
        .path
        .as_ref()
        .is_some_and(|p| p.to_string_lossy().contains("/Engine/"))
}

/// Folders above the report's own images that hold a .uproject: the
/// project the crash came from, searched for binaries and plugins.
pub fn project_roots(report: &Report) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    for image in &report.images {
        let Some(path) = &image.path else { continue };
        if let Some(dir) = path
            .ancestors()
            .find(|a| crate::project::Project::find_in(a).is_some())
            && !roots.iter().any(|r| r == dir)
        {
            roots.push(dir.to_path_buf());
        }
    }
    roots
}

/// Symbolicates the crashing thread (or every thread) of a report.
pub fn analyze(
    probe: &dyn Probe,
    report: &Report,
    extra_roots: &[PathBuf],
    all_threads: bool,
    signature_frames: usize,
) -> Analysis {
    let mut roots = project_roots(report);
    roots.extend(extra_roots.iter().cloned());
    let search = SearchPaths { roots };
    let threads: Vec<&report::Thread> = if all_threads {
        report.threads.iter().collect()
    } else {
        report.crashed_thread().into_iter().collect()
    };

    // Which images the shown frames use, and the addresses to look up in each.
    let mut lookups: HashMap<usize, Vec<(u64, bool)>> = HashMap::new();
    for thread in &threads {
        for (position, frame) in thread.frames.iter().enumerate() {
            if let Some(image) = frame.image {
                lookups
                    .entry(image)
                    .or_default()
                    .push((frame.address, position > 0));
            }
        }
    }
    let this_os = probe
        .run("sw_vers", &["-buildVersion"])
        .filter(|o| o.success)
        .map(|o| o.first_line());
    let same_os = report.os_build.is_some() && report.os_build == this_os;
    let mut found: HashMap<usize, Symbols> = HashMap::new();
    let mut resolved: HashMap<(usize, u64), Resolved> = HashMap::new();
    let mut order: Vec<usize> = lookups.keys().copied().collect();
    order.sort();
    for index in &order {
        let image = &report.images[*index];
        let located = symbols::locate(probe, image, &search, same_os);
        for (address, r) in symbols::symbolicate(probe, &located, image, &lookups[index]) {
            resolved.insert((*index, address), r);
        }
        found.insert(*index, located);
    }

    let unreal_tool = |s: &Symbols| matches!(s, Symbols::UnrealSymbols { .. });
    let threads_out: Vec<ThreadOut> = threads
        .iter()
        .map(|thread| ThreadOut {
            index: (thread.index != usize::MAX).then_some(thread.index),
            name: thread.name.clone(),
            crashed: thread.crashed,
            frames: thread
                .frames
                .iter()
                .enumerate()
                .map(|(position, frame)| {
                    let r = frame.image.and_then(|i| resolved.get(&(i, frame.address)));
                    let from = match (r, frame.image.and_then(|i| found.get(&i))) {
                        (Some(_), Some(s)) if unreal_tool(s) => Some("unreal-atos".to_string()),
                        (Some(_), _) => Some("atos".to_string()),
                        (None, _) if frame.report_symbol.is_some() => Some("report".to_string()),
                        _ => None,
                    };
                    FrameOut {
                        index: position,
                        module: frame.module.clone(),
                        address: format!("{:#x}", frame.address),
                        function: r
                            .map(|r| r.function.clone())
                            .or_else(|| frame.report_symbol.clone()),
                        file: r.and_then(|r| r.file.clone()),
                        line: r.and_then(|r| r.line),
                        from,
                    }
                })
                .collect(),
        })
        .collect();

    // The signature comes from the crashing thread, whatever is shown.
    let crashed = report.crashed_thread();
    let sig_frames: Vec<bucket::SignatureFrame> = crashed
        .map(|thread| {
            thread
                .frames
                .iter()
                .map(|frame| {
                    let image = frame.image.and_then(|i| report.images.get(i));
                    let r = frame.image.and_then(|i| resolved.get(&(i, frame.address)));
                    let function = r
                        .map(|r| r.function.clone())
                        .or_else(|| frame.report_symbol.clone())
                        .unwrap_or_else(|| {
                            format!("+{:#x}", frame.address - image.map(|i| i.base).unwrap_or(0))
                        });
                    bucket::SignatureFrame {
                        module: bucket::module_label(&frame.module),
                        function,
                        own: image.is_some_and(|i| !is_engine(i) && !symbols::is_system(i)),
                        system: image.is_some_and(symbols::is_system)
                            || frame.module.starts_with("libsystem_"),
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    let signature = bucket::signature(&sig_frames, signature_frames);

    Analysis {
        source: report.source,
        path: report.path.clone(),
        process: report.process.clone(),
        time: report.time.as_deref().map(readable_time),
        exception: report.exception.clone(),
        unreal: report.unreal.clone(),
        signature_id: bucket::id(&signature),
        signature,
        threads: threads_out,
        images: order
            .iter()
            .map(|i| {
                let image = &report.images[*i];
                ImageResult {
                    name: image.name.clone(),
                    path: image.path.clone(),
                    uuid: image.uuid.clone(),
                    symbols: found.remove(i).unwrap_or(Symbols::Missing),
                }
            })
            .collect(),
    }
}

/// Unreal writes TimeOfCrash as .NET ticks (100 ns since year 1, UTC).
/// Those become `2026-11-07 00:53:11 UTC`; other times pass through, trimmed.
pub fn readable_time(text: &str) -> String {
    if let Ok(ticks) = text.trim().parse::<u64>()
        && ticks > 621_355_968_000_000_000
    {
        let secs = (ticks - 621_355_968_000_000_000) / 10_000_000;
        let (days, rest) = (secs / 86_400, secs % 86_400);
        let (y, m, d) = civil_from_days(days as i64);
        return format!(
            "{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02} UTC",
            rest / 3600,
            rest % 3600 / 60,
            rest % 60
        );
    }
    let text = text.trim();
    // ".ips": 2026-11-06 19:12:38.3995 -0500 -> drop the fraction.
    match (text.find('.'), text.len() > 19) {
        (Some(19), true) => format!(
            "{}{}",
            &text[..19],
            text[19..].find(' ').map(|i| &text[19 + i..]).unwrap_or("")
        ),
        _ => text.to_string(),
    }
}

/// Days since 1970-01-01 to (year, month, day), from Howard Hinnant's algorithm.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// Crash reports for a project, newest first: its own Saved/Crashes, the
/// engine's Saved/Crashes folders for crashes named after it, and macOS
/// .ips reports for a process named after it or living in its folder.
pub fn find_for_project(
    project_dir: &Path,
    project_name: &str,
    home: Option<&Path>,
) -> Vec<PathBuf> {
    let mut found: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();
    let mut add = |path: PathBuf| {
        let time = std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .unwrap_or(std::time::UNIX_EPOCH);
        if !found.iter().any(|(_, p)| *p == path) {
            found.push((time, path));
        }
    };
    let folders = |dir: &Path| -> Vec<PathBuf> {
        std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| {
                p.join("CrashContext.runtime-xml").is_file() || p.join("minidump.dmp").is_file()
            })
            .collect()
    };
    for folder in folders(&project_dir.join("Saved/Crashes")) {
        add(folder);
    }
    if let Some(home) = home {
        let engines = home.join("Library/Application Support/Epic/UnrealEngine");
        for version in std::fs::read_dir(&engines)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
        {
            for folder in folders(&version.path().join("Saved/Crashes")) {
                let named = folder.file_name().is_some_and(|n| {
                    n.to_string_lossy()
                        .contains(&format!("-{project_name}-pid-"))
                });
                let game = std::fs::read_to_string(folder.join("CrashContext.runtime-xml"))
                    .ok()
                    .and_then(|x| report::parse_context(&x).game_name)
                    .is_some_and(|g| g == format!("UE-{project_name}") || g == project_name);
                if named || game {
                    add(folder);
                }
            }
        }
        let reports = home.join("Library/Logs/DiagnosticReports");
        for entry in std::fs::read_dir(&reports)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
        {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "ips") {
                let name = entry.file_name().to_string_lossy().into_owned();
                let mine = name.starts_with(&format!("{project_name}-"))
                    || std::fs::read_to_string(&path)
                        .is_ok_and(|t| t.contains(&project_dir.to_string_lossy().to_string()));
                if mine {
                    add(path);
                }
            }
        }
    }
    found.sort_by_key(|(time, _)| std::cmp::Reverse(*time));
    found.into_iter().map(|(_, p)| p).collect()
}

#[derive(Clone, Debug, Serialize)]
pub struct Group {
    pub signature_id: String,
    pub signature: Vec<String>,
    pub count: usize,
    pub first: Option<String>,
    pub last: Option<String>,
    pub kinds: Vec<String>,
    pub reports: Vec<PathBuf>,
}

/// Groups analyses by signature, biggest group first.
pub fn group(analyses: &[Analysis]) -> Vec<Group> {
    let mut groups: Vec<Group> = Vec::new();
    for a in analyses {
        let kind = a
            .unreal
            .as_ref()
            .and_then(|u| u.crash_type.clone())
            .or_else(|| a.exception.clone())
            .unwrap_or_else(|| "crash".into());
        let group = match groups
            .iter_mut()
            .position(|g| g.signature_id == a.signature_id)
        {
            Some(i) => &mut groups[i],
            None => {
                groups.push(Group {
                    signature_id: a.signature_id.clone(),
                    signature: a.signature.clone(),
                    count: 0,
                    first: None,
                    last: None,
                    kinds: Vec::new(),
                    reports: Vec::new(),
                });
                groups.last_mut().expect("just pushed")
            }
        };
        group.count += 1;
        group.reports.push(a.path.clone());
        if !group.kinds.contains(&kind) {
            group.kinds.push(kind);
        }
        if let Some(time) = &a.time {
            if group.first.as_ref().is_none_or(|f| time < f) {
                group.first = Some(time.clone());
            }
            if group.last.as_ref().is_none_or(|l| time > l) {
                group.last = Some(time.clone());
            }
        }
    }
    groups.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| b.last.cmp(&a.last)));
    groups
}

#[cfg(test)]
mod tests;
