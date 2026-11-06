//! `salvage refit`: get Windows-only Unreal plugins running on a Mac by
//! rebuilding them from source, and say plainly where that isn't possible.
//!
//! Windows plugin binaries can't load in the Mac editor (a different binary
//! format, CPU and operating system in one process), so the only route is the
//! plugin's source. `scan` reports, per plugin, whether it works on Mac and
//! what stands in the way. `patch` lets Mac through allow lists, on a copy in
//! the project and reversibly. `build` drives BuildPlugin and then checks that
//! Mac binaries really came out, since a plugin that leaves Mac out "builds"
//! on a Mac without compiling anything.

pub mod package;
pub mod patch;
pub mod render;
pub mod scan;
pub mod spans;
pub mod winapi;

use std::path::{Path, PathBuf};

pub use package::{PackageCheck, check};
pub use patch::{Patched, Undone, patch, undo};
pub use scan::{Finding, Plugin, Verdict};

/// Scans every plugin under each root. Plugins that can't be read come back
/// as errors next to the ones that could.
pub fn scan_roots(roots: &[PathBuf], deep: bool) -> (Vec<Plugin>, Vec<String>) {
    let mut plugins = Vec::new();
    let mut errors = Vec::new();
    for root in roots {
        for file in scan::find_plugins(root) {
            match scan::scan(&file, deep) {
                Ok(plugin) => plugins.push(plugin),
                Err(error) => errors.push(error),
            }
        }
    }
    (plugins, errors)
}

/// Whether `path` is inside an Unreal Engine install or source tree (the
/// folder holding `Engine/Build/Build.version`). Refit never edits those.
pub fn inside_engine(path: &Path) -> bool {
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    path.ancestors()
        .any(|a| a.join("Engine/Build/Build.version").is_file())
}

/// Copies a package's Mac binaries into a plugin folder, replacing any there.
pub fn install_binaries(package: &Path, plugin_dir: &Path) -> Result<PathBuf, String> {
    let from = package.join("Binaries/Mac");
    let to = plugin_dir.join("Binaries/Mac");
    if to.exists() {
        std::fs::remove_dir_all(&to).map_err(|e| format!("can't replace {}: {e}", to.display()))?;
    }
    std::fs::create_dir_all(&to).map_err(|e| format!("can't create {}: {e}", to.display()))?;
    let entries =
        std::fs::read_dir(&from).map_err(|e| format!("can't read {}: {e}", from.display()))?;
    for entry in entries.filter_map(Result::ok) {
        if entry.file_type().is_ok_and(|t| t.is_file()) {
            std::fs::copy(entry.path(), to.join(entry.file_name()))
                .map_err(|e| format!("can't copy {}: {e}", entry.path().display()))?;
        }
    }
    Ok(to)
}

#[cfg(test)]
mod tests;
