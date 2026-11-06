//! What a compile database depends on, so `--refresh` can skip regenerating it.
//!
//! The database lists every source file with its flags. Flags come from the
//! .uproject, .uplugin, *.Build.cs and *.Target.cs files and the engine
//! version; the file list changes when .cpp files are added or removed. Edits
//! inside existing files and new headers don't change it (clangd works headers
//! out on its own), so they don't trigger a refresh.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};

use super::{source_roots, walk};
use crate::engine::Engine;
use crate::project::Project;

/// Where the fingerprint of the last generation is kept, inside the project.
pub const STAMP_FILE: &str = "Intermediate/Salvage/clangd.json";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fingerprint {
    /// Target, platform and configuration the database was made for.
    pub build: String,
    pub engine: String,
    /// Build description files (relative path → size and modification time).
    pub rules: BTreeMap<String, (u64, u64)>,
    /// Number of translation units, and a hash of their paths.
    pub sources: usize,
    pub sources_hash: String,
}

const SOURCE_EXTENSIONS: [&str; 6] = ["cpp", "cc", "cxx", "c", "mm", "m"];

impl Fingerprint {
    pub fn of(project: &Project, engine: &Engine, build: &str) -> Fingerprint {
        let relative = |p: &Path| {
            p.strip_prefix(&project.dir)
                .unwrap_or(p)
                .to_string_lossy()
                .replace('\\', "/")
        };
        let mut rules = BTreeMap::new();
        let mut note = |path: &Path| {
            if let Ok(meta) = std::fs::metadata(path) {
                let modified = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                rules.insert(relative(path), (meta.len(), modified));
            }
        };
        note(&project.file);
        let is_rule = |p: &Path| {
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            name.ends_with(".Build.cs")
                || name.ends_with(".Target.cs")
                || name.ends_with(".uplugin")
        };
        for path in walk(&project.dir.join("Source"), &is_rule) {
            note(&path);
        }
        for path in walk(&project.dir.join("Plugins"), &is_rule) {
            note(&path);
        }

        let is_source = |p: &Path| {
            p.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| SOURCE_EXTENSIONS.iter().any(|s| e.eq_ignore_ascii_case(s)))
        };
        let sources: Vec<String> = source_roots(project)
            .iter()
            .flat_map(|root| walk(root, &is_source))
            .map(|p| relative(&p))
            .collect();

        let engine = format!(
            "{} {}",
            engine.root.display(),
            engine
                .version
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_default()
        );
        Fingerprint {
            build: build.to_string(),
            engine,
            rules,
            sources: sources.len(),
            sources_hash: format!("{:016x}", fnv1a(sources.join("\n").as_bytes())),
        }
    }

    pub fn load(project: &Project) -> Option<Fingerprint> {
        let text = std::fs::read_to_string(project.dir.join(STAMP_FILE)).ok()?;
        serde_json::from_str(&text).ok()
    }

    pub fn save(&self, project: &Project) -> Result<(), String> {
        let path = project.dir.join(STAMP_FILE);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)
                .map_err(|e| format!("can't create {}: {e}", dir.display()))?;
        }
        let text = serde_json::to_string_pretty(self).expect("fingerprint serializes");
        std::fs::write(&path, text + "\n")
            .map_err(|e| format!("can't write {}: {e}", path.display()))
    }

    /// Why `self` (now) differs from `previous`, in words, or None if it doesn't.
    pub fn changes_since(&self, previous: &Fingerprint) -> Option<String> {
        if self.build != previous.build {
            return Some(format!(
                "the build changed ({} → {})",
                previous.build, self.build
            ));
        }
        if self.engine != previous.engine {
            return Some("the engine changed".to_string());
        }
        if self.rules != previous.rules {
            let changed: Vec<&String> = self
                .rules
                .keys()
                .chain(previous.rules.keys())
                .filter(|k| self.rules.get(*k) != previous.rules.get(*k))
                .collect();
            let first = changed.first().map(|s| s.as_str()).unwrap_or("build rules");
            return Some(format!("{first} changed"));
        }
        if self.sources != previous.sources || self.sources_hash != previous.sources_hash {
            return Some(format!(
                "source files were added or removed ({} → {})",
                previous.sources, self.sources
            ));
        }
        None
    }
}

/// FNV-1a: small, stable across runs and platforms, fine for change detection.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, b| {
        (hash ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3)
    })
}
