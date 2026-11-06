//! Parses `memreport` output: the sections it writes between
//! `MemReport: Begin command "..."` and `MemReport: End command "..."`.

use std::path::Path;

use super::{Kind, Pool, Sounding, View, mb};
use crate::units::KIB;

const BEGIN: &str = "MemReport: Begin command \"";

pub fn looks_like(text: &str) -> bool {
    text.contains(BEGIN)
}

pub fn parse(path: &Path, text: &str) -> Result<Sounding, String> {
    let mut sounding = Sounding {
        file: path.to_path_buf(),
        kind: Kind::Memreport,
        labels: Vec::new(),
        footprint: None,
        peak: None,
        machine_memory: None,
        pools: Vec::new(),
    };
    let mut command: Option<String> = None;
    let mut platform: Option<String> = None;
    let mut config: Option<String> = None;
    let mut profile: Option<String> = None;

    for raw in text.lines() {
        let line = raw.trim();
        if let Some(rest) = line.strip_prefix(BEGIN) {
            command = Some(rest.trim_end_matches('"').to_ascii_lowercase());
            continue;
        }
        if line.starts_with("MemReport: End command") {
            command = None;
            continue;
        }
        let Some(cmd) = command.as_deref() else {
            // Header lines before the first command.
            if let Some(v) = line.strip_prefix("Config:") {
                config = Some(v.trim().to_string());
            } else if let Some(v) = line.strip_prefix("Device Profile:") {
                profile = Some(v.trim().to_string());
            }
            continue;
        };

        if cmd.starts_with("mem fromreport") {
            if let Some(v) = line.strip_prefix("Platform Memory Stats for ") {
                platform = Some(v.trim().to_string());
            } else if let Some(v) = line.strip_prefix("Process Physical Memory:") {
                let (used, peak) = used_and_peak(v);
                sounding.footprint = used;
                sounding.peak = peak;
            } else if let Some(v) = line.strip_prefix("Physical Memory:") {
                sounding.machine_memory = v
                    .split(',')
                    .find(|p| p.contains("total"))
                    .and_then(|p| mb(p.replace("total", "").trim()));
            }
        } else if let Some(name) = cmd.strip_prefix("rhi.dumpresourcememory summary name=") {
            if let Some(bytes) = shown_size(line) {
                sounding
                    .pools
                    .push(Pool::new(View::Gpu, display_name(name, line), bytes));
            }
        } else if let Some((name, bytes)) = headline_total(cmd, line) {
            // Zero means the platform doesn't track it (Metal reports no RHI
            // resource info), and the first value of a name is the full list's.
            if bytes > 0
                && !sounding
                    .pools
                    .iter()
                    .any(|p| p.view == View::Totals && p.name == name)
            {
                sounding.pools.push(Pool::new(View::Totals, name, bytes));
            }
        } else if is_full_object_list(cmd) {
            if let Some(pool) = object_class(line) {
                sounding.pools.push(pool);
            } else if let Some(bytes) = objects_total(line).filter(|b| *b > 0) {
                sounding.pools.push(Pool::new(
                    View::Totals,
                    "Objects and their resources",
                    bytes,
                ));
            }
        }
    }

    sounding.labels = [platform, config, profile.filter(|p| !p.is_empty())]
        .into_iter()
        .flatten()
        .fold(Vec::new(), |mut labels, label| {
            if !labels.contains(&label) {
                labels.push(label);
            }
            labels
        });
    if sounding.footprint.is_none() && sounding.pools.is_empty() {
        return Err(format!(
            "{} has memreport sections but none Salvage can read",
            path.display()
        ));
    }
    Ok(sounding)
}

/// `obj list -resourcesizesort` over every class. Memreports also list single
/// classes (`obj list class=StaticMesh ...`), which would count objects twice.
fn is_full_object_list(cmd: &str) -> bool {
    cmd.starts_with("obj list") && cmd.contains("-resourcesizesort") && !cmd.contains("class=")
}

/// The one-line totals some commands print. Only the unfiltered commands
/// count: `listtextures nonvt` and friends total a subset.
fn headline_total(cmd: &str, line: &str) -> Option<(&'static str, u64)> {
    let command = cmd.split_whitespace().next().unwrap_or("");
    let filtered = cmd.split_whitespace().count() > 1;
    if command == "rhi.dumpresourcememory" && !filtered {
        let size = line.strip_prefix("Total tracked resource size:")?;
        Some(("RHI tracked resources", mb(size)?))
    } else if command == "listtextures" && !filtered {
        let size = line.strip_prefix("Total size: InMem=")?;
        Some(("Textures in memory", mb(size.split_whitespace().next()?)?))
    } else if command == "r.dumprendertargetpoolmemory" {
        let (size, _) = line.split_once("MB total,")?;
        Some(("Render target pool", mb(size)?))
    } else {
        None
    }
}

/// `" 9612.40 MB used, 10350.75 MB peak"` → (used, peak).
fn used_and_peak(text: &str) -> (Option<u64>, Option<u64>) {
    let mut used = None;
    let mut peak = None;
    for part in text.split(',') {
        let part = part.trim();
        if let Some(v) = part.strip_suffix("used") {
            used = mb(v);
        } else if let Some(v) = part.strip_suffix("peak") {
            peak = mb(v);
        }
    }
    (used, peak)
}

/// `"Shown 61 entries with name Lumen. Size: 742.30/3640.50 MB (...)"` → 742.30 MB.
fn shown_size(line: &str) -> Option<u64> {
    if !line.starts_with("Shown ") {
        return None;
    }
    let size = line.split_once("Size:")?.1;
    mb(size.split('/').next()?)
}

/// Keeps the capitalization the capture used for a system's name.
fn display_name(lowercase: &str, line: &str) -> String {
    line.split_once("with name ")
        .and_then(|(_, rest)| rest.split_once(". Size:"))
        .map(|(name, _)| name.trim().to_string())
        .filter(|name| name.eq_ignore_ascii_case(lowercase))
        .unwrap_or_else(|| lowercase.to_string())
}

/// A class row from `obj list -resourcesizesort`, in either its column or CSV form:
/// class, count, NumKB, MaxKB, ResExcKB, ResExcDedSysKB, ResExcDedVidKB, ResExcUnkKB.
fn object_class(line: &str) -> Option<Pool> {
    let fields: Vec<&str> = match line.strip_prefix(',') {
        Some(csv) => csv.split(',').map(str::trim).collect(),
        None => line.split_whitespace().collect(),
    };
    if fields.len() < 8 {
        return None;
    }
    let (name, numbers) = fields.split_at(fields.len() - 7);
    let numbers: Vec<f64> = numbers
        .iter()
        .map(|n| n.parse().ok())
        .collect::<Option<_>>()?;
    let name = name.join(" ");
    if name.is_empty() || name == "Class" {
        return None;
    }
    let kb = numbers[1] + numbers[3]; // the objects themselves plus their resources
    Some(Pool {
        view: View::Objects,
        name,
        bytes: (kb * KIB as f64).round() as u64,
        peak: None,
        count: Some(numbers[0] as u64),
    })
}

/// `"183402 Objects (Total: 201.442M / Max: ... / Res: 3735.684M | ...)"` → total + res.
fn objects_total(line: &str) -> Option<u64> {
    let rest = line.split_once(" Objects (Total:")?.1;
    let total = mb(rest.split('/').next()?)?;
    let res = rest.split_once("Res:")?.1;
    let res = mb(res.split(['|', '/']).next()?)?;
    Some(total + res)
}
