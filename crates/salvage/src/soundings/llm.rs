//! Parses `-llmcsv` output. The first line names one column per LLM tag (each
//! followed by a comma, then padded with spaces); every later line is one
//! sample, in MB. Tags that appear mid-run make later rows longer than earlier
//! ones.

use std::path::Path;

use super::{Kind, Pool, Sounding, View, mb};

const TOTAL: &str = "Total";
const TRACKED_TOTAL: &str = "TrackedTotal";

pub fn looks_like(text: &str) -> bool {
    let header = columns(text.lines().next().unwrap_or(""));
    header.iter().any(|c| c == TRACKED_TOTAL || c == TOTAL)
}

pub fn parse(path: &Path, text: &str) -> Result<Sounding, String> {
    let mut lines = text.lines();
    let header = columns(lines.next().unwrap_or(""));
    let rows: Vec<Vec<u64>> = lines
        .map(columns)
        .filter(|row| !row.is_empty())
        .filter_map(|row| row.iter().map(|v| mb(v)).collect::<Option<Vec<u64>>>())
        .collect();
    if rows.is_empty() {
        return Err(format!(
            "{} has LLM columns but no samples yet",
            path.display()
        ));
    }
    let last = rows.last().expect("checked non-empty");
    let at = |row: &Vec<u64>, i: usize| row.get(i).copied().unwrap_or(0);
    let peak = |i: usize| rows.iter().map(|row| at(row, i)).max().unwrap_or(0);

    let platform = header.iter().any(|c| c == TOTAL)
        || path
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with("LLMPlatform"));
    let footprint_column = header
        .iter()
        .position(|c| c == TOTAL)
        .or_else(|| header.iter().position(|c| c == TRACKED_TOTAL));

    let pools = header
        .iter()
        .enumerate()
        .filter(|(_, name)| *name != TOTAL && *name != TRACKED_TOTAL)
        .map(|(i, name)| Pool {
            view: View::Llm,
            name: name.clone(),
            bytes: at(last, i),
            peak: Some(peak(i)),
            count: None,
        })
        .collect();

    Ok(Sounding {
        file: path.to_path_buf(),
        kind: if platform {
            Kind::LlmPlatform
        } else {
            Kind::Llm
        },
        labels: vec![format!("{} samples", rows.len())],
        footprint: footprint_column.map(|i| at(last, i)),
        peak: footprint_column.map(peak),
        machine_memory: None,
        pools,
    })
}

fn columns(line: &str) -> Vec<String> {
    line.split(',')
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .map(str::to_string)
        .collect()
}
