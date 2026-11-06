//! `salvage lldb`: installs Salvage's LLDB formatters for Unreal types by
//! adding one marked `command script import` line to `~/.lldbinit`.
//!
//! Existing content is never rewritten: the line is appended, a backup is
//! taken before any change, symlinked init files are edited in place, and
//! running install twice changes nothing.

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

/// The formatter script, embedded so the binary can install it anywhere.
pub const FORMATTERS: &str = include_str!("../lldb/salvage_unreal.py");
pub const SCRIPT_NAME: &str = "salvage_unreal.py";
/// Comment written above the import line, so people can see where it came from.
pub const MARKER: &str = "# Added by `salvage lldb install` (Unreal Engine formatters). Remove with `salvage lldb uninstall`.";

/// Where things live for one user.
#[derive(Clone, Debug)]
pub struct Layout {
    pub home: PathBuf,
    /// Salvage's data folder; the script goes in `lldb/` under it.
    pub data_dir: PathBuf,
}

impl Layout {
    /// The usual places on this OS.
    pub fn for_user(
        home: PathBuf,
        os: crate::probe::Os,
        env: impl Fn(&str) -> Option<String>,
    ) -> Layout {
        use crate::probe::Os;
        let data_dir = match os {
            Os::Macos => home.join("Library/Application Support/Salvage"),
            Os::Windows => env("APPDATA")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join("AppData/Roaming"))
                .join("Salvage"),
            _ => env("XDG_DATA_HOME")
                .filter(|d| Path::new(d).is_absolute())
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".local/share"))
                .join("salvage"),
        };
        Layout { home, data_dir }
    }

    pub fn script(&self) -> PathBuf {
        self.data_dir.join("lldb").join(SCRIPT_NAME)
    }

    /// `~/.lldbinit`, which plain lldb reads.
    pub fn lldbinit(&self) -> PathBuf {
        self.home.join(".lldbinit")
    }

    /// `~/.lldbinit-Xcode`, which Xcode reads instead when it exists.
    pub fn lldbinit_xcode(&self) -> PathBuf {
        self.home.join(".lldbinit-Xcode")
    }

    /// The line that loads the script.
    pub fn import_line(&self) -> String {
        // LLDB treats backslashes in quotes as escapes, so use forward slashes.
        let path = self
            .script()
            .to_string_lossy()
            .replace('\\', "/")
            .replace('"', "\\\"");
        format!("command script import \"{path}\"")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change {
    /// The import line was appended.
    Added,
    /// An older Salvage import line (another path) was pointed at the current script.
    Updated,
    /// Already there; nothing written.
    Unchanged,
    /// Salvage's lines were taken out.
    Removed,
    /// Nothing of Salvage's was there.
    Absent,
}

/// What happened to one init file.
#[derive(Clone, Debug)]
pub struct Edit {
    pub file: PathBuf,
    pub change: Change,
    pub backup: Option<PathBuf>,
}

#[derive(Clone, Debug)]
pub struct Outcome {
    pub script: PathBuf,
    /// Whether the script file was written (new or changed) or removed.
    pub script_changed: bool,
    pub edits: Vec<Edit>,
}

/// Writes the script and adds the import line to `~/.lldbinit` (and to
/// `~/.lldbinit-Xcode` when that file exists).
pub fn install(layout: &Layout, stamp: u64) -> Result<Outcome, String> {
    let script = layout.script();
    let script_changed = fs::read_to_string(&script).ok().as_deref() != Some(FORMATTERS);
    if script_changed {
        let dir = script.parent().expect("script has a folder");
        fs::create_dir_all(dir).map_err(|e| format!("can't create {}: {e}", dir.display()))?;
        write_atomically(&script, FORMATTERS, None)
            .map_err(|e| format!("can't write {}: {e}", script.display()))?;
    }
    let mut edits = vec![add_import(
        &layout.lldbinit(),
        &layout.import_line(),
        stamp,
    )?];
    if layout.lldbinit_xcode().exists() {
        edits.push(add_import(
            &layout.lldbinit_xcode(),
            &layout.import_line(),
            stamp,
        )?);
    }
    Ok(Outcome {
        script,
        script_changed,
        edits,
    })
}

/// Removes Salvage's lines from the init files and deletes the script.
pub fn uninstall(layout: &Layout, stamp: u64) -> Result<Outcome, String> {
    let mut edits = Vec::new();
    for file in [layout.lldbinit(), layout.lldbinit_xcode()] {
        if file.exists() {
            edits.push(remove_import(&file, stamp)?);
        }
    }
    let script = layout.script();
    let script_changed = script.exists();
    if script_changed {
        fs::remove_file(&script).map_err(|e| format!("can't remove {}: {e}", script.display()))?;
        // Tidy up the folders we made, if they're now empty.
        let _ = script.parent().map(fs::remove_dir);
        let _ = fs::remove_dir(&layout.data_dir);
    }
    Ok(Outcome {
        script,
        script_changed,
        edits,
    })
}

/// Whether a line is one of ours (the marker, or an import of our script).
fn is_ours(line: &str) -> bool {
    let line = line.trim();
    line == MARKER || (line.starts_with("command script import") && line.contains(SCRIPT_NAME))
}

fn add_import(file: &Path, import: &str, stamp: u64) -> Result<Edit, String> {
    let target = resolve(file);
    let existing = match fs::read_to_string(&target) {
        Ok(text) => Some(text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(format!("can't read {}: {e}", file.display())),
    };
    let text = existing.clone().unwrap_or_default();

    let ours: Vec<&str> = text
        .lines()
        .filter(|l| is_ours(l) && l.trim() != MARKER)
        .collect();
    let (updated, change) = if ours.iter().any(|l| l.trim() == import) {
        return Ok(Edit {
            file: file.to_path_buf(),
            change: Change::Unchanged,
            backup: None,
        });
    } else if !ours.is_empty() {
        // Point an earlier install at the current script, in place.
        let mut out: Vec<String> = Vec::new();
        let mut replaced = false;
        for line in text.lines() {
            if is_ours(line) && line.trim() != MARKER {
                if !replaced {
                    out.push(import.to_string());
                    replaced = true;
                }
            } else {
                out.push(line.to_string());
            }
        }
        (join_lines(&out, &text), Change::Updated)
    } else {
        let mut appended = text.clone();
        if !appended.is_empty() && !appended.ends_with('\n') {
            appended.push('\n');
        }
        if !appended.is_empty() {
            appended.push('\n');
        }
        appended.push_str(MARKER);
        appended.push('\n');
        appended.push_str(import);
        appended.push('\n');
        (appended, Change::Added)
    };

    let backup = match existing {
        Some(_) => Some(back_up(file, &target, stamp)?),
        None => None,
    };
    write_atomically(&target, &updated, Some(&target))
        .map_err(|e| format!("can't write {}: {e}", file.display()))?;
    Ok(Edit {
        file: file.to_path_buf(),
        change,
        backup,
    })
}

fn remove_import(file: &Path, stamp: u64) -> Result<Edit, String> {
    let target = resolve(file);
    let text =
        fs::read_to_string(&target).map_err(|e| format!("can't read {}: {e}", file.display()))?;
    if !text.lines().any(is_ours) {
        return Ok(Edit {
            file: file.to_path_buf(),
            change: Change::Absent,
            backup: None,
        });
    }
    let mut kept: Vec<String> = text
        .lines()
        .filter(|l| !is_ours(l))
        .map(str::to_string)
        .collect();
    // Drop the blank line install added before the marker, if it's now trailing.
    while kept.last().is_some_and(|l| l.trim().is_empty()) {
        kept.pop();
    }
    let updated = if kept.is_empty() {
        String::new()
    } else {
        join_lines(&kept, "\n")
    };
    let backup = back_up(file, &target, stamp)?;
    write_atomically(&target, &updated, Some(&target))
        .map_err(|e| format!("can't write {}: {e}", file.display()))?;
    Ok(Edit {
        file: file.to_path_buf(),
        change: Change::Removed,
        backup: Some(backup),
    })
}

/// Joins lines with the original file's line ending, ending with a newline.
fn join_lines(lines: &[String], original: &str) -> String {
    let newline = if original.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut text = lines.join(newline);
    text.push_str(newline);
    text
}

/// Follows a symlinked init file (dotfile managers do this) so we edit the real file.
fn resolve(file: &Path) -> PathBuf {
    fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf())
}

/// Copies the current file next to the one users know about: `~/.lldbinit.salvage-backup-<stamp>`.
fn back_up(file: &Path, target: &Path, stamp: u64) -> Result<PathBuf, String> {
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
fn write_atomically(
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

#[cfg(test)]
mod tests {
    use super::*;

    fn layout(dir: &Path) -> Layout {
        let home = dir.join("home");
        fs::create_dir_all(&home).unwrap();
        Layout {
            data_dir: home.join("Library/Application Support/Salvage"),
            home,
        }
    }

    fn read(path: &Path) -> String {
        fs::read_to_string(path).unwrap()
    }

    #[test]
    fn installs_into_a_fresh_home() {
        let dir = tempfile::tempdir().unwrap();
        let layout = layout(dir.path());
        let outcome = install(&layout, 100).unwrap();
        assert!(outcome.script_changed);
        assert_eq!(read(&layout.script()), FORMATTERS);
        assert_eq!(outcome.edits.len(), 1);
        assert_eq!(outcome.edits[0].change, Change::Added);
        assert!(outcome.edits[0].backup.is_none(), "nothing to back up");
        assert_eq!(
            read(&layout.lldbinit()),
            format!("{MARKER}\n{}\n", layout.import_line())
        );
        assert!(
            layout
                .import_line()
                .contains("Application Support/Salvage/lldb/salvage_unreal.py\"")
        );
    }

    #[test]
    fn appends_after_existing_content_and_backs_it_up() {
        let dir = tempfile::tempdir().unwrap();
        let layout = layout(dir.path());
        let mine = "settings set target.x86-disassembly-flavor intel\ncommand alias bt2 thread backtrace -c 2";
        fs::write(layout.lldbinit(), mine).unwrap();

        let outcome = install(&layout, 200).unwrap();
        let edit = &outcome.edits[0];
        assert_eq!(edit.change, Change::Added);
        let backup = edit.backup.clone().unwrap();
        assert_eq!(backup, layout.home.join(".lldbinit.salvage-backup-200"));
        assert_eq!(read(&backup), mine);
        assert_eq!(
            read(&layout.lldbinit()),
            format!("{mine}\n\n{MARKER}\n{}\n", layout.import_line())
        );
    }

    #[test]
    fn installing_twice_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let layout = layout(dir.path());
        fs::write(layout.lldbinit(), "# my settings\n").unwrap();
        install(&layout, 1).unwrap();
        let after_first = read(&layout.lldbinit());
        let outcome = install(&layout, 2).unwrap();
        assert!(!outcome.script_changed);
        assert_eq!(outcome.edits[0].change, Change::Unchanged);
        assert!(outcome.edits[0].backup.is_none());
        assert_eq!(read(&layout.lldbinit()), after_first);
        assert!(!layout.home.join(".lldbinit.salvage-backup-2").exists());
    }

    #[test]
    fn repoints_an_older_install_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let layout = layout(dir.path());
        let old = format!(
            "before\n{MARKER}\ncommand script import \"/old/place/salvage_unreal.py\"\nafter\n"
        );
        fs::write(layout.lldbinit(), &old).unwrap();
        let outcome = install(&layout, 3).unwrap();
        assert_eq!(outcome.edits[0].change, Change::Updated);
        assert_eq!(
            read(&layout.lldbinit()),
            format!("before\n{MARKER}\n{}\nafter\n", layout.import_line())
        );
    }

    #[test]
    fn also_covers_the_xcode_init_file_when_present() {
        let dir = tempfile::tempdir().unwrap();
        let layout = layout(dir.path());
        fs::write(layout.lldbinit_xcode(), "command source ~/.lldb/xcode\n").unwrap();
        let outcome = install(&layout, 4).unwrap();
        assert_eq!(outcome.edits.len(), 2);
        assert!(read(&layout.lldbinit_xcode()).ends_with(&format!("{}\n", layout.import_line())));
        assert!(read(&layout.lldbinit_xcode()).starts_with("command source ~/.lldb/xcode\n"));
    }

    #[cfg(unix)]
    #[test]
    fn edits_a_symlinked_init_file_through_the_link() {
        let dir = tempfile::tempdir().unwrap();
        let layout = layout(dir.path());
        let real = dir.path().join("dotfiles-lldbinit");
        fs::write(&real, "# managed by my dotfiles\n").unwrap();
        std::os::unix::fs::symlink(&real, layout.lldbinit()).unwrap();
        install(&layout, 5).unwrap();
        assert!(
            fs::symlink_metadata(layout.lldbinit())
                .unwrap()
                .file_type()
                .is_symlink(),
            "link kept"
        );
        assert!(read(&real).contains(&layout.import_line()));
    }

    #[cfg(unix)]
    #[test]
    fn keeps_file_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let layout = layout(dir.path());
        fs::write(layout.lldbinit(), "# private\n").unwrap();
        fs::set_permissions(layout.lldbinit(), fs::Permissions::from_mode(0o600)).unwrap();
        install(&layout, 6).unwrap();
        let mode = fs::metadata(layout.lldbinit())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn uninstall_removes_only_our_lines() {
        let dir = tempfile::tempdir().unwrap();
        let layout = layout(dir.path());
        let mine = "settings set stop-line-count-before 5\n";
        fs::write(layout.lldbinit(), mine).unwrap();
        install(&layout, 7).unwrap();

        let outcome = uninstall(&layout, 8).unwrap();
        assert!(outcome.script_changed);
        assert!(!layout.script().exists());
        assert!(!layout.data_dir.exists(), "empty folders tidied up");
        assert_eq!(outcome.edits[0].change, Change::Removed);
        assert_eq!(read(&layout.lldbinit()), mine);
        assert!(outcome.edits[0].backup.as_ref().unwrap().exists());

        let again = uninstall(&layout, 9).unwrap();
        assert_eq!(again.edits[0].change, Change::Absent);
        assert!(!again.script_changed);
        assert_eq!(read(&layout.lldbinit()), mine);
    }

    #[test]
    fn uninstall_with_nothing_installed_touches_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let layout = layout(dir.path());
        let outcome = uninstall(&layout, 10).unwrap();
        assert!(outcome.edits.is_empty());
        assert!(!layout.lldbinit().exists());
    }

    #[test]
    fn keeps_windows_line_endings() {
        let dir = tempfile::tempdir().unwrap();
        let layout = layout(dir.path());
        let old =
            format!("one\r\n{MARKER}\r\ncommand script import \"/old/salvage_unreal.py\"\r\n");
        fs::write(layout.lldbinit(), &old).unwrap();
        install(&layout, 11).unwrap();
        assert_eq!(
            read(&layout.lldbinit()),
            format!("one\r\n{MARKER}\r\n{}\r\n", layout.import_line())
        );
    }

    #[test]
    fn data_folders_follow_each_platform() {
        use crate::probe::Os;
        let home = PathBuf::from("/home/sam");
        let none = |_: &str| None;
        assert_eq!(
            Layout::for_user(home.clone(), Os::Macos, none).script(),
            home.join("Library/Application Support/Salvage/lldb/salvage_unreal.py")
        );
        assert_eq!(
            Layout::for_user(home.clone(), Os::Linux, none).script(),
            home.join(".local/share/salvage/lldb/salvage_unreal.py")
        );
        let xdg = |k: &str| (k == "XDG_DATA_HOME").then(|| "/data".to_string());
        assert_eq!(
            Layout::for_user(home, Os::Linux, xdg).script(),
            PathBuf::from("/data/salvage/lldb/salvage_unreal.py")
        );
    }
}
