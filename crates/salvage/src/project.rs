//! Unreal projects (`.uproject` files).

use std::path::{Path, PathBuf};

#[derive(Clone, Debug, serde::Serialize)]
pub struct Project {
    pub name: String,
    pub file: PathBuf,
    pub dir: PathBuf,
    /// `EngineAssociation` from the .uproject: `"5.8"` for Launcher engines,
    /// a GUID for source builds, empty when the project sits inside an engine tree.
    pub engine_association: Option<String>,
}

impl Project {
    /// Opens a `.uproject` file, or the single `.uproject` inside a folder.
    pub fn open(path: &Path) -> Result<Project, String> {
        let file = if path.is_dir() {
            match find_uproject(path) {
                Some(file) => file,
                None => return Err(format!("no .uproject file in {}", path.display())),
            }
        } else if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("uproject"))
        {
            path.to_path_buf()
        } else {
            return Err(format!(
                "{} is not a .uproject file or a project folder",
                path.display()
            ));
        };
        let text = std::fs::read_to_string(&file)
            .map_err(|e| format!("can't read {}: {e}", file.display()))?;
        let json = crate::jsonc::parse(&text)
            .map_err(|e| format!("{} isn't valid JSON: {e}", file.display()))?;
        let engine_association = json
            .get("EngineAssociation")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let name = file
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let dir = file.parent().map(Path::to_path_buf).unwrap_or_default();
        Ok(Project {
            name,
            file,
            dir,
            engine_association,
        })
    }

    /// Finds a project in `dir`, if there is exactly one `.uproject` there.
    pub fn find_in(dir: &Path) -> Option<Project> {
        find_uproject(dir).and_then(|file| Project::open(&file).ok())
    }
}

fn find_uproject(dir: &Path) -> Option<PathBuf> {
    let mut found = std::fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("uproject"))
        });
    let first = found.next()?;
    found.next().is_none().then_some(first)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_a_project_folder() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Harbor.uproject"),
            r#"{ "FileVersion": 3, "EngineAssociation": "5.8" }"#,
        )
        .unwrap();
        let project = Project::open(dir.path()).unwrap();
        assert_eq!(project.name, "Harbor");
        assert_eq!(project.engine_association.as_deref(), Some("5.8"));
        assert_eq!(project.dir, dir.path());
    }

    #[test]
    fn rejects_folders_without_a_project() {
        let dir = tempfile::tempdir().unwrap();
        assert!(Project::open(dir.path()).is_err());
        assert!(Project::find_in(dir.path()).is_none());
    }
}
