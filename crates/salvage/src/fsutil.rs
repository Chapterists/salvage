//! Writing files that belong to the user: back up first, write atomically,
//! follow symlinks, and keep permissions.

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

/// Follows a symlinked file (dotfile managers do this) so we edit the real file.
pub fn resolve(file: &Path) -> PathBuf {
    fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf())
}

/// Copies `target` (the real file behind `file`) to `<file>.salvage-backup-<stamp>`
/// next to `file`, adding a counter if that name is taken.
pub fn back_up(file: &Path, target: &Path, stamp: u64) -> Result<PathBuf, String> {
    let name = file
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut backup = file.with_file_name(format!("{name}.salvage-backup-{stamp}"));
    let mut n = 1;
    while backup.exists() {
        backup = file.with_file_name(format!("{name}.salvage-backup-{stamp}-{n}"));
        n += 1;
    }
    fs::copy(target, &backup).map_err(|e| {
        format!(
            "can't back up {} to {}: {e}",
            file.display(),
            backup.display()
        )
    })?;
    Ok(backup)
}

/// Writes through a temporary file and a rename, keeping the original's permissions.
pub fn write_atomically(
    path: &Path,
    text: &str,
    keep_permissions_of: Option<&Path>,
) -> std::io::Result<()> {
    let permissions = keep_permissions_of
        .and_then(|p| fs::metadata(p).ok())
        .map(|m| m.permissions());
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let temp = path.with_file_name(format!(".{name}.salvage-tmp-{}", std::process::id()));
    {
        let mut file = fs::File::create(&temp)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
    }
    if let Some(permissions) = permissions {
        fs::set_permissions(&temp, permissions)?;
    }
    fs::rename(&temp, path).inspect_err(|_| {
        let _ = fs::remove_file(&temp);
    })
}

/// What happened when writing a user-visible file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Written {
    Created,
    /// Replaced; the old content was backed up first.
    Replaced {
        backup: PathBuf,
    },
    Unchanged,
}

/// Writes `text` to `file`. A new file is just created; an identical one is
/// left alone; anything else is backed up before it's replaced.
pub fn write_with_backup(file: &Path, text: &str, stamp: u64) -> Result<Written, String> {
    let target = resolve(file);
    match fs::read_to_string(&target) {
        Ok(existing) if existing == text => Ok(Written::Unchanged),
        Ok(_) => {
            let backup = back_up(file, &target, stamp)?;
            write_atomically(&target, text, Some(&target))
                .map_err(|e| format!("can't write {}: {e}", file.display()))?;
            Ok(Written::Replaced { backup })
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if let Some(dir) = file.parent() {
                fs::create_dir_all(dir)
                    .map_err(|e| format!("can't create {}: {e}", dir.display()))?;
            }
            write_atomically(file, text, None)
                .map_err(|e| format!("can't write {}: {e}", file.display()))?;
            Ok(Written::Created)
        }
        Err(e) => Err(format!("can't read {}: {e}", file.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_keeps_and_backs_up() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("sub/settings.json");
        assert_eq!(
            write_with_backup(&file, "one", 1).unwrap(),
            Written::Created
        );
        assert_eq!(
            write_with_backup(&file, "one", 2).unwrap(),
            Written::Unchanged
        );
        let Written::Replaced { backup } = write_with_backup(&file, "two", 3).unwrap() else {
            panic!("expected a backup")
        };
        assert_eq!(
            backup,
            dir.path().join("sub/settings.json.salvage-backup-3")
        );
        assert_eq!(fs::read_to_string(backup).unwrap(), "one");
        assert_eq!(fs::read_to_string(&file).unwrap(), "two");
    }
}
