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

/// What writing or removing Salvage's block does to one file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    /// The file doesn't exist yet and would hold just the block.
    Create,
    /// The file would change: a block added, replaced or taken out.
    Update,
    /// The file already holds exactly this.
    Unchanged,
    /// The file held nothing but the block, so it goes.
    Delete,
}

fn read(file: &Path) -> Result<Option<String>, String> {
    match std::fs::read_to_string(resolve(file)) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("can't read {}: {e}", file.display())),
    }
}

/// The text of a file once `body` is between the markers: an existing block
/// is replaced in place, otherwise one is appended.
fn with_block(existing: Option<&str>, body: &str) -> String {
    let block = format!("{BEGIN}\n{body}{END}\n");
    match existing.map(|t| (t, split(t))) {
        Some((_, Some((before, _, after)))) => format!("{before}{block}{after}"),
        Some(("", None)) | None => block,
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
    }
}

/// The text of a file with Salvage's block taken out, or None if it has none.
fn without_block(text: &str) -> Option<String> {
    let (before, _, after) = split(text)?;
    // Drop the blank line upsert put before the block.
    let before = before
        .strip_suffix("\n\n")
        .map(|b| format!("{b}\n"))
        .unwrap_or_else(|| before.to_string());
    Some(format!("{before}{after}"))
}

/// What `upsert_block` would do to `file`, without touching it.
pub fn preview_upsert(file: &Path, body: &str) -> Result<Change, String> {
    Ok(match read(file)? {
        None => Change::Create,
        Some(text) if text == with_block(Some(&text), body) => Change::Unchanged,
        Some(_) => Change::Update,
    })
}

/// What `remove_block` would do to `file`, without touching it. None when it
/// has no block.
pub fn preview_remove(file: &Path) -> Result<Option<Change>, String> {
    let Some(text) = read(file)? else {
        return Ok(None);
    };
    Ok(without_block(&text).map(|updated| {
        if updated.trim().is_empty() {
            Change::Delete
        } else {
            Change::Update
        }
    }))
}

/// Puts `body` between the markers in `file`: replaces an existing block in
/// place, or appends one. Backs up a file before changing it.
pub fn upsert_block(file: &Path, body: &str, stamp: u64) -> Result<Written, String> {
    let target = resolve(file);
    let existing = read(file)?;
    let updated = with_block(existing.as_deref(), body);
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
    let Some(text) = read(file)? else {
        return Ok(None);
    };
    let Some(updated) = without_block(&text) else {
        return Ok(None);
    };
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
