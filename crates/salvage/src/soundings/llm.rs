//! Parses `-llmcsv` output. The first line names one column per LLM tag (each
//! followed by a comma, then padded with spaces); every later line is one
//! sample, in MB. Tags that appear mid-run make later rows longer than earlier
//! ones.

use std::path::Path;

use super::{Kind, Pool, Sounding, View};

const TOTAL: &str = "Total";
const TRACKED_TOTAL: &str = "TrackedTotal";
const UNTRACKED: &str = "Untracked";
/// Free memory the OS reported, not memory in use.
const OS_AVAILABLE: &str = "OSAvailable";

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
        // Tags can dip a little below zero when memory is freed under a
        // different tag than it was allocated with; count those as zero.
        .filter_map(|row| {
            row.iter()
                .map(|v| megabytes(v))
                .collect::<Option<Vec<u64>>>()
        })
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

    // The file name says which tracker wrote it. Both can have a Total column
    // (on macOS the default tracker's is address space too), so the header
    // only decides for files that were renamed.
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let platform = if name.starts_with("LLMPlatform") {
        true
    } else if name.starts_with("LLM_") {
        false
    } else {
        header.iter().any(|c| c == TOTAL)
    };
    let column = |name: &str| header.iter().position(|c| c == name);
    let mut labels = vec![format!("{} samples", rows.len())];
    // On some platforms (macOS among them) the platform tracker's Total is the
    // process's address space, hundreds of GB, not memory in use. Then the
    // tracked total is the meaningful number.
    let address_space = match (column(TOTAL), column(TRACKED_TOTAL)) {
        (Some(total), Some(tracked)) => peak(total) > peak(tracked).saturating_mul(4),
        _ => false,
    };
    if address_space {
        labels.push("Total is address space here, so the draft uses TrackedTotal".to_string());
        if !platform {
            labels.push(
                "for the whole process, read the LLMPlatform CSV from the same run".to_string(),
            );
        }
    }
    let footprint_column = if address_space {
        column(TRACKED_TOTAL)
    } else {
        column(TOTAL).or_else(|| column(TRACKED_TOTAL))
    };
    let not_pools: &[&str] = if address_space {
        &[TOTAL, TRACKED_TOTAL, UNTRACKED, OS_AVAILABLE]
    } else {
        &[TOTAL, TRACKED_TOTAL, OS_AVAILABLE]
    };

    let pools = header
        .iter()
        .enumerate()
        // Child tags ("Meshes/StaticMesh") are part of their parent; listing both counts twice.
        .filter(|(_, name)| !not_pools.contains(&name.as_str()) && !name.contains('/'))
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
        labels,
        footprint: footprint_column.map(|i| at(last, i)),
        peak: footprint_column.map(peak),
        machine_memory: None,
        pools,
    })
}

/// A sample in MB as bytes, with negative values counted as zero.
fn megabytes(text: &str) -> Option<u64> {
    let value: f64 = text.trim().parse().ok()?;
    value
        .is_finite()
        .then(|| (value.max(0.0) * crate::units::MIB as f64).round() as u64)
}

fn columns(line: &str) -> Vec<String> {
    line.split(',')
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .map(str::to_string)
        .collect()
}
