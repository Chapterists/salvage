//! Checks that UnrealHeaderTool's generated headers exist. Without them every
//! `#include "Foo.generated.h"` is an error and clangd can't make sense of
//! UCLASS/USTRUCT types.

use std::collections::BTreeSet;
use std::path::Path;

use super::{source_roots, walk};
use crate::project::Project;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GeneratedHeaders {
    /// Generated headers the project's code includes.
    pub expected: usize,
    /// Those not found under any Intermediate/Build folder.
    pub missing: Vec<String>,
}

pub fn check_generated_headers(project: &Project) -> GeneratedHeaders {
    let is_header = |p: &Path| p.extension().is_some_and(|e| e == "h" || e == "hpp");
    let mut wanted: BTreeSet<String> = BTreeSet::new();
    for root in source_roots(project) {
        for header in walk(&root, &is_header) {
            let Ok(text) = std::fs::read_to_string(&header) else {
                continue;
            };
            for line in text.lines() {
                let line = line.trim_start();
                if let Some(rest) = line.strip_prefix("#include") {
                    let name = rest
                        .trim()
                        .trim_matches(|c| c == '"' || c == '<' || c == '>');
                    if name.ends_with(".generated.h") {
                        let file = name.rsplit('/').next().unwrap_or(name);
                        wanted.insert(file.to_string());
                    }
                }
            }
        }
    }
    if wanted.is_empty() {
        return GeneratedHeaders::default();
    }

    let mut intermediates = vec![project.dir.join("Intermediate/Build")];
    for root in source_roots(project) {
        if let Some(plugin) = root.parent() {
            intermediates.push(plugin.join("Intermediate/Build"));
        }
    }
    let have: BTreeSet<String> = intermediates
        .iter()
        .flat_map(|dir| generated_in(dir))
        .collect();
    GeneratedHeaders {
        expected: wanted.len(),
        missing: wanted.into_iter().filter(|w| !have.contains(w)).collect(),
    }
}

/// Generated header names under an Intermediate/Build folder (which the
/// source walk skips, so this walks it directly).
fn generated_in(dir: &Path) -> Vec<String> {
    let mut names = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let name = entry.file_name().to_string_lossy().into_owned();
            if kind.is_dir() {
                pending.push(entry.path());
            } else if name.ends_with(".generated.h") {
                names.push(name);
            }
        }
    }
    names
}
