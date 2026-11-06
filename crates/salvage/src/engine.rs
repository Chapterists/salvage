//! Finding an Unreal Engine install and reading the few facts doctor needs
//! from it. Everything is read at runtime from the user's own install.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::ini;
use crate::probe::Probe;
use crate::project::Project;
use crate::version::Version;

#[derive(Clone, Debug, serde::Serialize)]
pub struct Engine {
    /// The folder that contains `Engine/`.
    pub root: PathBuf,
    pub version: Option<Version>,
    /// True for Launcher (installed) builds, false for source builds.
    pub installed_build: bool,
    pub found_by: FoundBy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum FoundBy {
    /// `--engine` or `SALVAGE_ENGINE`.
    Flag,
    /// The project's `EngineAssociation`.
    Project,
    /// The newest engine in the Launcher's install list.
    Launcher,
    /// The newest engine in the Launcher's default install folder.
    DefaultFolder,
}

impl FoundBy {
    pub fn describe(self) -> &'static str {
        match self {
            FoundBy::Flag => "as given",
            FoundBy::Project => "from the project's engine association",
            FoundBy::Launcher => "newest in the Epic Games Launcher",
            FoundBy::DefaultFolder => "newest in the default install folder",
        }
    }
}

/// Xcode version limits the engine declares for itself.
#[derive(Clone, Debug, serde::Serialize)]
pub struct AppleSdk {
    pub main: Option<Version>,
    pub min: Version,
    pub max: Version,
    pub file: PathBuf,
}

impl Engine {
    /// Opens an engine at `path`, which may be the install folder or its `Engine/`.
    pub fn open(path: &Path, found_by: FoundBy) -> Result<Engine, String> {
        let root = if path.join("Engine/Build/Build.version").is_file() {
            path.to_path_buf()
        } else if path.join("Build/Build.version").is_file()
            && path.file_name().is_some_and(|n| n == "Engine")
        {
            path.parent().map(Path::to_path_buf).unwrap_or_default()
        } else {
            return Err(format!(
                "{} doesn't look like an Unreal Engine install (no Engine/Build/Build.version)",
                path.display()
            ));
        };
        let version = std::fs::read_to_string(root.join("Engine/Build/Build.version"))
            .ok()
            .and_then(|text| crate::jsonc::parse(&text).ok())
            .and_then(|json| build_version(&json));
        let installed_build = root.join("Engine/Build/InstalledBuild.txt").is_file();
        Ok(Engine {
            root,
            version,
            installed_build,
            found_by,
        })
    }

    pub fn dir(&self) -> PathBuf {
        self.root.join("Engine")
    }

    pub fn label(&self) -> String {
        match &self.version {
            Some(v) => format!("Unreal Engine {v}"),
            None => "Unreal Engine (unknown version)".to_string(),
        }
    }

    /// Reads the Xcode range from the engine's Apple SDK file, following
    /// `ParentSDKFile` links for keys a platform file doesn't set itself.
    pub fn apple_sdk(&self) -> Result<AppleSdk, String> {
        let config = self.dir().join("Config");
        let start = [
            config.join("Apple/Apple_SDK.json"),
            config.join("Mac/Mac_SDK.json"),
        ]
        .into_iter()
        .find(|p| p.is_file())
        .ok_or("the engine has no Apple SDK file under Engine/Config")?;

        let (mut main, mut min, mut max) = (None, None, None);
        let mut next = Some(start.clone());
        for _ in 0..4 {
            let Some(file) = next.take() else { break };
            let text = std::fs::read_to_string(&file)
                .map_err(|e| format!("can't read {}: {e}", file.display()))?;
            let json = crate::jsonc::parse(&text)
                .map_err(|e| format!("can't parse {}: {e}", file.display()))?;
            let get = |key: &str| {
                json.get(key)
                    .and_then(Value::as_str)
                    .and_then(Version::parse)
            };
            main = main.or_else(|| get("MainVersion"));
            min = min.or_else(|| get("MinVersion"));
            max = max.or_else(|| get("MaxVersion"));
            next = json
                .get("ParentSDKFile")
                .and_then(Value::as_str)
                .and_then(|parent| file.parent().map(|dir| dir.join(parent)));
        }
        match (min, max) {
            (Some(min), Some(max)) => Ok(AppleSdk {
                main,
                min,
                max,
                file: start,
            }),
            _ => Err(format!(
                "{} has no Xcode MinVersion/MaxVersion",
                start.display()
            )),
        }
    }

    /// Unreal's LLDB pretty-printers (FString, TArray...), if this engine ships them.
    pub fn lldb_formatters(&self) -> Option<PathBuf> {
        let dir = self.dir().join("Extras/LLDBDataFormatters");
        // Apple platforms use 2-byte TCHARs, which need the _2ByteChars flavor.
        ["UEDataFormatters_2ByteChars.py", "UEDataFormatters.py"]
            .into_iter()
            .map(|name| dir.join(name))
            .find(|p| p.is_file())
    }
}

fn build_version(json: &Value) -> Option<Version> {
    let part = |key: &str| json.get(key).and_then(Value::as_u64);
    let text = format!(
        "{}.{}.{}",
        part("MajorVersion")?,
        part("MinorVersion")?,
        part("PatchVersion")?
    );
    Version::parse(&text)
}

/// Finds the engine to check: the flag, then the project's association, then
/// the newest Launcher install, then the newest engine in the default folder.
/// An explicit path that isn't an engine is an error; finding nothing is not.
pub fn discover(
    explicit: Option<&Path>,
    project: Option<&Project>,
    probe: &dyn Probe,
) -> Result<Option<Engine>, String> {
    if let Some(path) = explicit {
        return Engine::open(path, FoundBy::Flag).map(Some);
    }
    let launcher = launcher_engines(probe);
    if let Some(association) = project.and_then(|p| p.engine_association.as_deref()) {
        let by_name = launcher
            .iter()
            .filter(|(name, _)| name == association)
            .map(|(_, path)| path.clone());
        let by_folder = probe
            .install_roots()
            .into_iter()
            .map(|r| r.join(format!("UE_{association}")));
        let by_registry = source_builds(probe)
            .into_iter()
            .filter(|(id, _)| {
                id.trim_matches(['{', '}'])
                    .eq_ignore_ascii_case(association.trim_matches(['{', '}']))
            })
            .map(|(_, path)| path);
        for path in by_name.chain(by_folder).chain(by_registry) {
            if let Ok(engine) = Engine::open(&path, FoundBy::Project) {
                return Ok(Some(engine));
            }
        }
    }
    if let Some(engine) = newest(launcher, FoundBy::Launcher) {
        return Ok(Some(engine));
    }
    let in_folders = probe
        .install_roots()
        .into_iter()
        .flat_map(|root| {
            std::fs::read_dir(root)
                .into_iter()
                .flatten()
                .filter_map(Result::ok)
        })
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            name.strip_prefix("UE_")
                .map(|v| (v.to_string(), entry.path()))
        })
        .collect();
    Ok(newest(in_folders, FoundBy::DefaultFolder))
}

/// `(version, path)` for every engine the Launcher lists, e.g. `("5.8", ...)`.
fn launcher_engines(probe: &dyn Probe) -> Vec<(String, PathBuf)> {
    let mut engines = Vec::new();
    for manifest in probe.launcher_manifests() {
        let Some(json) = std::fs::read_to_string(&manifest)
            .ok()
            .and_then(|text| crate::jsonc::parse(&text).ok())
        else {
            continue;
        };
        let list = json.get("InstallationList").and_then(Value::as_array);
        for item in list.into_iter().flatten() {
            let app = item
                .get("AppName")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let location = item.get("InstallLocation").and_then(Value::as_str);
            if let (Some(version), Some(location)) = (app.strip_prefix("UE_"), location) {
                engines.push((version.to_string(), PathBuf::from(location)));
            }
        }
    }
    engines
}

/// `(id, path)` for every registered source build.
fn source_builds(probe: &dyn Probe) -> Vec<(String, PathBuf)> {
    probe
        .source_build_registries()
        .iter()
        .filter_map(|file| std::fs::read_to_string(file).ok())
        .flat_map(|text| ini::parse(&text))
        .filter(|e| e.section.eq_ignore_ascii_case("Installations"))
        .map(|e| (e.key, PathBuf::from(e.value)))
        .collect()
}

fn newest(candidates: Vec<(String, PathBuf)>, found_by: FoundBy) -> Option<Engine> {
    let mut candidates: Vec<(Version, PathBuf)> = candidates
        .into_iter()
        .filter_map(|(v, p)| Version::parse(&v).map(|v| (v, p)))
        .collect();
    candidates.sort_by(|a, b| b.0.cmp(&a.0));
    candidates
        .into_iter()
        .find_map(|(_, path)| Engine::open(&path, found_by).ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{FakeProbe, fake_engine};

    #[test]
    fn opens_the_install_or_its_engine_folder() {
        let dir = tempfile::tempdir().unwrap();
        let root = fake_engine(&dir.path().join("UE_9.9"), (9, 9, 1));
        let engine = Engine::open(&root, FoundBy::Flag).unwrap();
        assert_eq!(engine.version.as_ref().unwrap().to_string(), "9.9.1");
        assert!(engine.installed_build);
        assert_eq!(
            Engine::open(&root.join("Engine"), FoundBy::Flag)
                .unwrap()
                .root,
            root
        );
        assert!(Engine::open(dir.path(), FoundBy::Flag).is_err());
    }

    #[test]
    fn explicit_paths_must_be_engines() {
        let dir = tempfile::tempdir().unwrap();
        let probe = FakeProbe::healthy_mac(dir.path());
        let error = discover(Some(dir.path()), None, &probe).unwrap_err();
        assert!(error.contains("doesn't look like an Unreal Engine install"));
    }

    #[test]
    fn finds_the_newest_engine_in_the_default_folder() {
        let dir = tempfile::tempdir().unwrap();
        let installs = dir.path().join("Epic Games");
        fake_engine(&installs.join("UE_9.10"), (9, 10, 0));
        fake_engine(&installs.join("UE_9.9"), (9, 9, 2));
        std::fs::create_dir_all(installs.join("UE_broken")).unwrap();
        let mut probe = FakeProbe::healthy_mac(dir.path());
        probe.install_roots = vec![installs];
        let engine = discover(None, None, &probe).unwrap().unwrap();
        assert_eq!(engine.version.unwrap().to_string(), "9.10.0");
        assert_eq!(engine.found_by, FoundBy::DefaultFolder);
    }

    #[test]
    fn prefers_the_projects_engine_then_the_launcher_list() {
        let dir = tempfile::tempdir().unwrap();
        let older = fake_engine(&dir.path().join("Engines/Older"), (9, 8, 0));
        let newer = fake_engine(&dir.path().join("Engines/Newer"), (9, 9, 0));
        let manifest = dir
            .path()
            .join("Library/Application Support/Epic/UnrealEngineLauncher/LauncherInstalled.dat");
        std::fs::create_dir_all(manifest.parent().unwrap()).unwrap();
        let entry = |name: &str, path: &Path| {
            format!(
                r#"{{ "InstallLocation": {:?}, "AppName": "{name}" }}"#,
                path.display().to_string()
            )
        };
        std::fs::write(
            &manifest,
            format!(
                r#"{{ "InstallationList": [ {}, {}, {} ] }}"#,
                entry("UE_9.8", &older),
                entry("UE_9.9", &newer),
                entry("SomePlugin_9.9", &newer)
            ),
        )
        .unwrap();
        let probe = FakeProbe::healthy_mac(dir.path());

        let engine = discover(None, None, &probe).unwrap().unwrap();
        assert_eq!(engine.root, newer);
        assert_eq!(engine.found_by, FoundBy::Launcher);

        std::fs::write(
            dir.path().join("Old.uproject"),
            r#"{ "EngineAssociation": "9.8" }"#,
        )
        .unwrap();
        let project = Project::open(&dir.path().join("Old.uproject")).unwrap();
        let engine = discover(None, Some(&project), &probe).unwrap().unwrap();
        assert_eq!(engine.root, older);
        assert_eq!(engine.found_by, FoundBy::Project);
    }

    #[test]
    fn finds_registered_source_builds_by_guid() {
        let dir = tempfile::tempdir().unwrap();
        let source = fake_engine(&dir.path().join("src/MyEngine"), (9, 9, 0));
        let registry = dir
            .path()
            .join("Library/Application Support/Epic/UnrealEngine/Install.ini");
        std::fs::create_dir_all(registry.parent().unwrap()).unwrap();
        std::fs::write(
            &registry,
            format!("[Installations]\n{{ABCD-1234}}={}\n", source.display()),
        )
        .unwrap();
        std::fs::write(
            dir.path().join("Src.uproject"),
            r#"{ "EngineAssociation": "{ABCD-1234}" }"#,
        )
        .unwrap();
        let project = Project::open(&dir.path().join("Src.uproject")).unwrap();
        let probe = FakeProbe::healthy_mac(dir.path());
        let engine = discover(None, Some(&project), &probe).unwrap().unwrap();
        assert_eq!(engine.root, source);
    }

    #[test]
    fn reads_the_xcode_range_through_parent_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = fake_engine(&dir.path().join("UE"), (9, 9, 0));
        let config = root.join("Engine/Config");
        // An older layout: the Mac file only points at a parent with the range.
        std::fs::remove_file(config.join("Apple/Apple_SDK.json")).unwrap();
        std::fs::create_dir_all(config.join("Mac")).unwrap();
        std::fs::write(
            config.join("Mac/Mac_SDK.json"),
            r#"{ "ParentSDKFile": "../Shared/Range.json" }"#,
        )
        .unwrap();
        std::fs::create_dir_all(config.join("Shared")).unwrap();
        std::fs::write(
            config.join("Shared/Range.json"),
            r#"{ "MinVersion": "1.0", "MaxVersion": "2.5" }"#,
        )
        .unwrap();
        let sdk = Engine::open(&root, FoundBy::Flag)
            .unwrap()
            .apple_sdk()
            .unwrap();
        assert_eq!(sdk.min.to_string(), "1.0");
        assert_eq!(sdk.max.to_string(), "2.5");
        assert!(sdk.main.is_none());
    }
}
