//! Salvage's marked blocks inside a user's ini files.

use std::path::Path;

use crate::fsutil::{Written, back_up, resolve, write_atomically};

pub const BEGIN: &str = "; >>> salvage trim: begin. Generated; remove with `salvage trim --undo`.";
pub const END: &str = "; <<< salvage trim: end";

/// Splits `text` into (before, block, after) around Salvage's block, if any.
fn split(text: &str) -> Option<(&str, &str, &str)> {
    let start = text.find(BEGIN)?;
    let end_marker = text[start..].find(END)? + start;
    let end = text[end_marker..]
        .find('\n')
        .map(|i| end_marker + i + 1)
        .unwrap_or(text.len());
    Some((&text[..start], &text[start..end], &text[end..]))
}

/// Puts `body` between the markers in `file`: replaces an existing block in
/// place, or appends one. Backs up a file before changing it.
pub fn upsert_block(file: &Path, body: &str, stamp: u64) -> Result<Written, String> {
    let block = format!("{BEGIN}\n{body}{END}\n");
    let target = resolve(file);
    let existing = match std::fs::read_to_string(&target) {
        Ok(text) => Some(text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(format!("can't read {}: {e}", file.display())),
    };
    let updated = match existing.as_deref().map(|t| (t, split(t))) {
        Some((_, Some((before, _, after)))) => format!("{before}{block}{after}"),
        Some(("", None)) => block.clone(),
        Some((text, None)) => {
            let separator = if text.ends_with("\n\n") {
                ""
            } else if text.ends_with('\n') {
                "\n"
            } else {
                "\n\n"
            };
            format!("{text}{separator}{block}")
        }
        None => block.clone(),
    };
    match existing {
        Some(text) if text == updated => Ok(Written::Unchanged),
        Some(_) => {
            let backup = back_up(file, &target, stamp)?;
            write_atomically(&target, &updated, Some(&target))
                .map_err(|e| format!("can't write {}: {e}", file.display()))?;
            Ok(Written::Replaced { backup })
        }
        None => {
            if let Some(dir) = file.parent() {
                std::fs::create_dir_all(dir)
                    .map_err(|e| format!("can't create {}: {e}", dir.display()))?;
            }
            write_atomically(file, &updated, None)
                .map_err(|e| format!("can't write {}: {e}", file.display()))?;
            Ok(Written::Created)
        }
    }
}

/// Takes Salvage's block out of `file`. Returns None when there was none. A
/// file that held nothing else is deleted, along with its folder if that's
/// now empty.
pub fn remove_block(file: &Path, stamp: u64) -> Result<Option<Written>, String> {
    let target = resolve(file);
    let Ok(text) = std::fs::read_to_string(&target) else {
        return Ok(None);
    };
    let Some((before, _, after)) = split(&text) else {
        return Ok(None);
    };
    // Drop the blank line upsert put before the block.
    let before = before
        .strip_suffix("\n\n")
        .map(|b| format!("{b}\n"))
        .unwrap_or_else(|| before.to_string());
    let updated = format!("{before}{after}");
    if updated.trim().is_empty() {
        // Nothing but our block: no one else's content to keep, so no backup.
        std::fs::remove_file(&target)
            .map_err(|e| format!("can't remove {}: {e}", file.display()))?;
        // Tidy up a platform folder (Config/Mac...) that's now empty.
        if let Some(dir) = file.parent() {
            let _ = std::fs::remove_dir(dir);
        }
        return Ok(Some(Written::Removed));
    }
    let backup = back_up(file, &target, stamp)?;
    write_atomically(&target, &updated, Some(&target))
        .map_err(|e| format!("can't write {}: {e}", file.display()))?;
    Ok(Some(Written::Replaced { backup }))
}
