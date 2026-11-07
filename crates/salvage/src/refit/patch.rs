//! Lets Mac through a plugin's allow lists: adds Mac to every allow list and
//! supported-platform list that leaves it out, and takes it off deny lists.
//!
//! The `.uplugin` is edited in place, touching only those arrays. It's backed
//! up first, and a record next to it (`.salvage-refit.json`) keeps the
//! original text, so `--undo` puts it back byte for byte. A plugin outside the
//! project is copied into the project's Plugins folder first: Salvage never
//! edits an engine install.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::scan::{ALLOW_KEYS, DENY_KEYS, SUPPORTED_KEY};
use super::spans::{self, Edit, Spanned};
use crate::fsutil::{back_up, write_atomically};

pub const RECORD: &str = ".salvage-refit.json";

/// The file BuildPlugin adds to a plugin that has none.
const FILTER_FILE: &str = "Config/FilterPlugin.ini";

/// What `patch` did, or would do.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Patched {
    /// The plugin folder that was (or would be) edited.
    pub dir: PathBuf,
    /// Copied from here into the project first.
    pub copied_from: Option<PathBuf>,
    /// One line per list changed, e.g. "PSDImporterCore: PlatformAllowList + Mac".
    pub changes: Vec<String>,
    pub backup: Option<PathBuf>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Record {
    /// The .uplugin's file name.
    uplugin: String,
    original: String,
    patched: String,
    copied_from: Option<PathBuf>,
    /// Files in the copy, with a hash of each, to tell later whether it's
    /// still untouched.
    #[serde(default)]
    copied_files: Vec<(String, u64)>,
}

/// The edits that let Mac in, with a line describing each.
pub fn mac_edits(text: &str) -> Result<Vec<(Edit, String)>, String> {
    let json = spans::parse(text)?;
    let mut edits = Vec::new();
    let mut visit = |object: &Spanned, label: &str, supported_too: bool| {
        for key in ALLOW_KEYS {
            if let Some(list) = object.get(key)
                && !list.strings().is_empty()
                && !names_mac(list)
                && let Some(edit) = spans::append_string(text, list, "Mac")
            {
                edits.push((edit, format!("{label}: {key} + Mac")));
            }
        }
        for key in DENY_KEYS {
            if let Some(list) = object.get(key) {
                // From the end, so earlier offsets stay valid if there are several.
                let indices: Vec<usize> = list
                    .items()
                    .iter()
                    .enumerate()
                    .filter(|(_, item)| {
                        item.as_str().is_some_and(|s| s.eq_ignore_ascii_case("Mac"))
                    })
                    .map(|(i, _)| i)
                    .collect();
                if let Some(&first) = indices.first()
                    && let Some(edit) = spans::remove_item(list, first)
                {
                    edits.push((edit, format!("{label}: {key} - Mac")));
                }
            }
        }
        if supported_too
            && let Some(list) = object.get(SUPPORTED_KEY)
            && !list.strings().is_empty()
            && !names_mac(list)
            && let Some(edit) = spans::append_string(text, list, "Mac")
        {
            edits.push((edit, format!("plugin: {SUPPORTED_KEY} + Mac")));
        }
    };
    visit(&json, "plugin", true);
    if let Some(modules) = json.get("Modules") {
        for module in modules.items() {
            let name = module
                .get("Name")
                .and_then(Spanned::as_str)
                .unwrap_or("module");
            visit(module, name, false);
        }
    }
    Ok(edits)
}

fn names_mac(list: &Spanned) -> bool {
    list.strings().iter().any(|s| s.eq_ignore_ascii_case("Mac"))
}

/// Where a plugin lives once patched: in place if it's already inside the
/// project, otherwise `<project>/Plugins/<Name>`.
pub fn destination(plugin_dir: &Path, project_dir: &Path) -> PathBuf {
    let canonical = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    if canonical(plugin_dir).starts_with(canonical(project_dir)) {
        plugin_dir.to_path_buf()
    } else {
        let name = plugin_dir.file_name().unwrap_or_default();
        project_dir.join("Plugins").join(name)
    }
}

/// Patches the plugin at `uplugin` into `project_dir` (copying it there first
/// if it lives elsewhere). With `dry_run`, works out the same answer and
/// changes nothing.
pub fn patch(
    uplugin: &Path,
    project_dir: &Path,
    stamp: u64,
    dry_run: bool,
) -> Result<Patched, String> {
    let source_dir = uplugin.parent().unwrap_or(Path::new(".")).to_path_buf();
    let file_name = uplugin
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let dir = destination(&source_dir, project_dir);
    let copy = dir != source_dir;
    if copy && dir.exists() && !dir.join(RECORD).exists() {
        // Already there, not ours: patch that copy instead of overwriting it.
        if dir.join(&file_name).is_file() {
            return patch(&dir.join(&file_name), project_dir, stamp, dry_run);
        }
        return Err(format!(
            "{} exists and isn't a copy of this plugin",
            dir.display()
        ));
    }
    let target = dir.join(&file_name);
    let current = if copy && !target.exists() {
        read(uplugin)?
    } else {
        read(&target)?
    };
    let edits = mac_edits(&current)?;
    let changes: Vec<String> = edits.iter().map(|(_, line)| line.clone()).collect();
    let copied_from = (copy && !dir.join(RECORD).exists()).then(|| source_dir.clone());
    if changes.is_empty() || dry_run {
        return Ok(Patched {
            dir,
            copied_from: if changes.is_empty() {
                None
            } else {
                copied_from
            },
            changes,
            backup: None,
        });
    }

    let mut copied_files = Vec::new();
    if let Some(from) = &copied_from {
        copied_files = copy_plugin(from, &dir)?;
    }
    let patched = spans::apply(&current, edits.into_iter().map(|(e, _)| e).collect());
    let mut record = match read_record(&dir) {
        Some(record) => record,
        None => Record {
            uplugin: file_name.clone(),
            original: current.clone(),
            patched: String::new(),
            copied_from: copied_from.clone(),
            copied_files,
        },
    };
    let backup = if copied_from.is_some() {
        None
    } else {
        Some(back_up(&target, &target, stamp)?)
    };
    write_atomically(&target, &patched, Some(&target))
        .map_err(|e| format!("can't write {}: {e}", target.display()))?;
    record.patched = patched;
    let json = serde_json::to_string_pretty(&record).map_err(|e| e.to_string())?;
    std::fs::write(dir.join(RECORD), json + "\n")
        .map_err(|e| format!("can't write {}: {e}", dir.join(RECORD).display()))?;
    Ok(Patched {
        dir,
        copied_from,
        changes,
        backup,
    })
}

/// What `undo` did.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub enum Undone {
    /// Nothing to undo here.
    NotPatched,
    /// The .uplugin is back as it was.
    Restored,
    /// The copy Salvage made was untouched apart from build output, so it's gone.
    RemovedCopy,
    /// The .uplugin is back, but the copy has other changes, so it stays.
    RestoredKeptCopy { changed: Vec<String> },
}

pub fn undo(plugin_dir: &Path) -> Result<Undone, String> {
    let Some(record) = read_record(plugin_dir) else {
        return Ok(Undone::NotPatched);
    };
    let target = plugin_dir.join(&record.uplugin);
    let current = read(&target)?;
    if current != record.patched {
        return Err(format!(
            "{} changed after salvage refit patched it, so it can't be put back safely; \
             its original text is in {}",
            target.display(),
            plugin_dir.join(RECORD).display()
        ));
    }
    write_atomically(&target, &record.original, Some(&target))
        .map_err(|e| format!("can't write {}: {e}", target.display()))?;
    std::fs::remove_file(plugin_dir.join(RECORD))
        .map_err(|e| format!("can't remove {}: {e}", plugin_dir.join(RECORD).display()))?;
    if record.copied_from.is_none() {
        return Ok(Undone::Restored);
    }
    let now = hash_tree(plugin_dir)?;
    let changed: Vec<String> = now
        .iter()
        .filter(|entry| !record.copied_files.contains(entry))
        // BuildPlugin writes a default filter file into the plugin when there
        // isn't one; that's build output too.
        .filter(|(path, _)| {
            path != FILTER_FILE || record.copied_files.iter().any(|(p, _)| p == FILTER_FILE)
        })
        .map(|(path, _)| path.clone())
        .chain(
            record
                .copied_files
                .iter()
                .filter(|entry| !now.iter().any(|(p, _)| *p == entry.0))
                .map(|(path, _)| format!("{path} (deleted)")),
        )
        .collect();
    if changed.is_empty() {
        std::fs::remove_dir_all(plugin_dir)
            .map_err(|e| format!("can't remove {}: {e}", plugin_dir.display()))?;
        Ok(Undone::RemovedCopy)
    } else {
        Ok(Undone::RestoredKeptCopy { changed })
    }
}

fn read(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("can't read {}: {e}", path.display()))
}

fn read_record(dir: &Path) -> Option<Record> {
    let text = std::fs::read_to_string(dir.join(RECORD)).ok()?;
    serde_json::from_str(&text).ok()
}

/// Build output, which doesn't count as a change to a copy.
fn is_build_output(relative: &str) -> bool {
    relative.starts_with("Binaries/") || relative.starts_with("Intermediate/")
}

/// Copies a plugin's source, content and settings (not its build output).
/// Returns each copied file with its hash.
fn copy_plugin(from: &Path, to: &Path) -> Result<Vec<(String, u64)>, String> {
    let files = list_tree(from)?;
    for relative in &files {
        let source = from.join(relative);
        let target = to.join(relative);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("can't create {}: {e}", parent.display()))?;
        }
        std::fs::copy(&source, &target).map_err(|e| {
            format!(
                "can't copy {} to {}: {e}",
                source.display(),
                target.display()
            )
        })?;
    }
    hash_tree(to)
}

/// Relative paths of every file under `root`, build output and our record left out.
fn list_tree(root: &Path) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries =
            std::fs::read_dir(&dir).map_err(|e| format!("can't read {}: {e}", dir.display()))?;
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            let relative = super::scan::relative(root, &path);
            if is_build_output(&format!("{relative}/")) || relative == RECORD {
                continue;
            }
            if entry.file_type().is_ok_and(|t| t.is_dir()) {
                stack.push(path);
            } else {
                out.push(relative);
            }
        }
    }
    out.sort();
    Ok(out)
}

fn hash_tree(root: &Path) -> Result<Vec<(String, u64)>, String> {
    list_tree(root)?
        .into_iter()
        .map(|relative| {
            let bytes = std::fs::read(root.join(&relative))
                .map_err(|e| format!("can't read {relative}: {e}"))?;
            Ok((relative, fnv1a(&bytes)))
        })
        .collect()
}

fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |hash, &b| {
        (hash ^ u64::from(b)).wrapping_mul(0x100000001b3)
    })
}
