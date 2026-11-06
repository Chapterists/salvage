use std::fs;
use std::path::Path;

use super::*;
use crate::engine::FoundBy;
use crate::fsutil::Written;
use crate::testing::fake_engine;

/// An invented project: Harbor, with game, editor and server targets, one
/// module with a UCLASS header, and a plugin.
fn harbor(dir: &Path) -> Project {
    let root = dir.join("Harbor");
    let write = |rel: &str, text: &str| {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    };
    write(
        "Harbor.uproject",
        r#"{ "EngineAssociation": "9.9", "Modules": [{ "Name": "Harbor" }] }"#,
    );
    write("Source/Harbor.Target.cs", "// game target fixture\n");
    write(
        "Source/HarborEditor.Target.cs",
        "// editor target fixture\n",
    );
    write(
        "Source/HarborServer.Target.cs",
        "// server target fixture\n",
    );
    write("Source/Harbor/Harbor.Build.cs", "// module rules fixture\n");
    write("Source/Harbor/Harbor.cpp", "// module\n");
    write(
        "Source/Harbor/Boat.h",
        "#pragma once\n#include \"CoreMinimal.h\"\n#include \"Boat.generated.h\"\n",
    );
    write("Source/Harbor/Boat.cpp", "#include \"Boat.h\"\n");
    write("Plugins/Rigging/Rigging.uplugin", "{}\n");
    write(
        "Plugins/Rigging/Source/Rigging/Rigging.Build.cs",
        "// plugin rules fixture\n",
    );
    write(
        "Plugins/Rigging/Source/Rigging/Public/Rope.h",
        "#include \"Rope.generated.h\"\n",
    );
    write(
        "Plugins/Rigging/Source/Rigging/Private/Rope.cpp",
        "// rope\n",
    );
    write("Content/Maps/ignored.cpp", "// content isn't code\n");
    Project::open(&root).unwrap()
}

fn engine(dir: &Path) -> Engine {
    Engine::open(&fake_engine(&dir.join("UE_9.9"), (9, 9, 0)), FoundBy::Flag).unwrap()
}

fn request(target: &str) -> Request {
    Request {
        target: target.to_string(),
        platform: "Mac".to_string(),
        configuration: "Development".to_string(),
        vscode: false,
        neovim: false,
        refresh: false,
        os: Os::Macos,
        stamp: 1,
    }
}

/// Stands in for UnrealBuildTool: writes a two-entry database where it was told to.
fn fake_ubt(calls: &mut Vec<UbtCommand>) -> impl FnMut(&UbtCommand) -> Result<(), String> + '_ {
    move |command: &UbtCommand| {
        let dir = command
            .args
            .iter()
            .find_map(|a| a.strip_prefix("-OutputDir="))
            .unwrap();
        let entry = r#"{ "file": "Boat.cpp", "directory": "/engine/Source", "command": "clang++ -c Boat.cpp" }"#;
        fs::write(Path::new(dir).join(DATABASE), format!("[{entry}, {entry}]")).unwrap();
        calls.push(command.clone());
        Ok(())
    }
}

#[test]
fn lists_and_chooses_targets() {
    let dir = tempfile::tempdir().unwrap();
    let project = harbor(dir.path());
    assert_eq!(
        targets(&project),
        ["Harbor", "HarborEditor", "HarborServer"]
    );
    assert_eq!(choose_target(&project, None).unwrap(), "HarborEditor");
    assert_eq!(choose_target(&project, Some("harbor")).unwrap(), "Harbor");
    let error = choose_target(&project, Some("HarborClient")).unwrap_err();
    assert!(
        error.contains("Harbor, HarborEditor, HarborServer"),
        "{error}"
    );

    fs::remove_file(project.dir.join("Source/HarborEditor.Target.cs")).unwrap();
    assert_eq!(choose_target(&project, None).unwrap(), "Harbor");
}

#[test]
fn blueprint_only_projects_are_explained() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("Plain.uproject"), "{}").unwrap();
    let project = Project::open(dir.path()).unwrap();
    let error = choose_target(&project, None).unwrap_err();
    assert!(error.contains("Blueprint-only"), "{error}");
}

#[test]
fn builds_the_unrealbuildtool_command() {
    let dir = tempfile::tempdir().unwrap();
    let project = harbor(dir.path());
    let engine = engine(dir.path());
    let command = UbtCommand::new(&engine, &project, "HarborEditor", "Mac", "Development");
    assert!(
        command
            .program
            .ends_with("Engine/Build/BatchFiles/RunUBT.sh")
    );
    let project_dir = fs::canonicalize(&project.dir).unwrap();
    assert_eq!(
        command.args,
        [
            "HarborEditor".to_string(),
            "Mac".to_string(),
            "Development".to_string(),
            format!("-Project={}", project_dir.join("Harbor.uproject").display()),
            "-Mode=GenerateClangDatabase".to_string(),
            format!("-OutputDir={}", project_dir.display()),
            "-OutputFilename=compile_commands.json".to_string(),
        ]
    );
    let windows = UbtCommand::new(&engine, &project, "HarborEditor", "Win64", "Development");
    assert!(windows.program.ends_with("RunUBT.bat"));
    assert_eq!(host_platform(Os::Linux), Some("Linux"));
}

#[test]
fn clangd_config_is_tuned_for_unreal() {
    let config = clangd_config();
    for expected in [
        "Remove: [-Werror*]",
        "UnusedIncludes: None",
        "MissingIncludes: None",
        "PathMatch: .*\\.generated\\.h",
        "Suppress: \"*\"",
    ] {
        assert!(config.contains(expected), "missing {expected}");
    }
    assert_eq!(config.matches("\n---\n").count(), 1, "two YAML documents");
    assert!(!config.contains('\t'), "YAML must not use tabs");
}

#[test]
fn vscode_settings_merge_with_whats_there() {
    let args = clangd_args(Os::Linux);
    let existing =
        "{\n  // mine\n  \"editor.fontSize\": 14,\n  \"clangd.arguments\": [\"--old\"],\n}";
    let merged: serde_json::Value =
        serde_json::from_str(&vscode_settings(Some(existing), &args).unwrap()).unwrap();
    assert_eq!(merged["editor.fontSize"], 14);
    assert_eq!(merged["clangd.arguments"][0], "--background-index");
    assert_eq!(merged["C_Cpp.intelliSenseEngine"], "disabled");
    assert!(vscode_settings(Some("[1, 2]"), &args).is_err());
    assert!(vscode_settings(Some("{ nope"), &args).is_err());
    assert!(vscode_settings(Some("  "), &args).is_ok());
    assert!(
        clangd_args(Os::Macos)
            .iter()
            .any(|a| a.starts_with("--query-driver="))
    );
    assert!(!args.iter().any(|a| a.starts_with("--query-driver=")));
}

#[test]
fn neovim_config_starts_clangd_with_the_flags() {
    let text = neovim_config(&["--background-index".to_string(), "--odd=\"q\"".to_string()]);
    assert!(
        text.contains(r#"local cmd = { "clangd", "--background-index", "--odd=\"q\"" }"#),
        "{text}"
    );
    assert!(text.contains("vim.lsp.config(\"clangd\""));
    assert!(text.contains("lspconfig.clangd.setup"));
}

#[test]
fn spots_missing_generated_headers() {
    let dir = tempfile::tempdir().unwrap();
    let project = harbor(dir.path());
    let check = check_generated_headers(&project);
    assert_eq!(check.expected, 2);
    assert_eq!(check.missing, ["Boat.generated.h", "Rope.generated.h"]);

    let write = |rel: &str| {
        let path = project.dir.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "// generated\n").unwrap();
    };
    write("Intermediate/Build/Mac/UnrealEditor/Inc/Harbor/UHT/Boat.generated.h");
    write("Plugins/Rigging/Intermediate/Build/Mac/UnrealEditor/Inc/Rigging/UHT/Rope.generated.h");
    assert!(check_generated_headers(&project).missing.is_empty());
}

#[test]
fn fingerprint_tracks_what_shapes_the_database() {
    let dir = tempfile::tempdir().unwrap();
    let project = harbor(dir.path());
    let engine = engine(dir.path());
    let first = Fingerprint::of(&project, &engine, "HarborEditor Mac Development");
    assert_eq!(
        first.sources, 3,
        "Harbor.cpp, Boat.cpp, Rope.cpp; not Content"
    );
    assert!(first.rules.contains_key("Source/Harbor/Harbor.Build.cs"));
    assert!(first.rules.contains_key("Plugins/Rigging/Rigging.uplugin"));

    // A new header or an edit inside a source file changes nothing.
    fs::write(project.dir.join("Source/Harbor/Dock.h"), "#pragma once\n").unwrap();
    fs::write(
        project.dir.join("Source/Harbor/Boat.cpp"),
        "// a longer edit to the body\n",
    )
    .unwrap();
    let same = Fingerprint::of(&project, &engine, "HarborEditor Mac Development");
    assert_eq!(same.changes_since(&first), None);

    fs::write(project.dir.join("Source/Harbor/Dock.cpp"), "// new\n").unwrap();
    let added = Fingerprint::of(&project, &engine, "HarborEditor Mac Development");
    assert!(
        added
            .changes_since(&first)
            .unwrap()
            .contains("added or removed (3 → 4)")
    );

    fs::write(
        project.dir.join("Source/Harbor/Harbor.Build.cs"),
        "// new dependency module added here\n",
    )
    .unwrap();
    let rules = Fingerprint::of(&project, &engine, "HarborEditor Mac Development");
    assert_eq!(
        rules.changes_since(&added).unwrap(),
        "Source/Harbor/Harbor.Build.cs changed"
    );

    let other = Fingerprint::of(&project, &engine, "Harbor Mac Development");
    assert!(
        other
            .changes_since(&rules)
            .unwrap()
            .starts_with("the build changed")
    );
}

#[test]
fn sets_up_a_project_and_refreshes_only_when_needed() {
    let dir = tempfile::tempdir().unwrap();
    let project = harbor(dir.path());
    let engine = engine(dir.path());
    let mut calls = Vec::new();

    let mut first = request("HarborEditor");
    first.vscode = true;
    first.neovim = true;
    let outcome = setup(&project, &engine, &first, &mut fake_ubt(&mut calls)).unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(outcome.entries, Some(2));
    assert!(outcome.skipped.is_none());
    assert!(outcome.files.iter().all(|(_, w)| *w == Written::Created));
    assert_eq!(
        fs::read_to_string(project.dir.join(CLANGD_FILE)).unwrap(),
        clangd_config()
    );
    assert!(project.dir.join(VSCODE_FILE).is_file());
    assert!(project.dir.join(NEOVIM_FILE).is_file());
    assert!(project.dir.join(STAMP_FILE).is_file());
    assert_eq!(outcome.generated.missing.len(), 2);

    // Refresh with nothing changed: no UnrealBuildTool run, files untouched.
    let mut refresh = first.clone();
    refresh.refresh = true;
    refresh.stamp = 2;
    let outcome = setup(&project, &engine, &refresh, &mut fake_ubt(&mut calls)).unwrap();
    assert_eq!(calls.len(), 1, "UnrealBuildTool not run again");
    assert!(outcome.skipped.is_some());
    assert!(outcome.files.iter().all(|(_, w)| *w == Written::Unchanged));

    // A new source file makes refresh regenerate.
    fs::write(project.dir.join("Source/Harbor/Dock.cpp"), "// new\n").unwrap();
    let outcome = setup(&project, &engine, &refresh, &mut fake_ubt(&mut calls)).unwrap();
    assert_eq!(calls.len(), 2);
    assert!(outcome.reason.unwrap().contains("added or removed"));
}

#[test]
fn never_replaces_someone_elses_files_without_a_backup() {
    let dir = tempfile::tempdir().unwrap();
    let project = harbor(dir.path());
    let engine = engine(dir.path());
    fs::write(
        project.dir.join(CLANGD_FILE),
        "CompileFlags:\n  Add: [-DMINE]\n",
    )
    .unwrap();
    fs::create_dir_all(project.dir.join(".vscode")).unwrap();
    fs::write(
        project.dir.join(VSCODE_FILE),
        "{ \"files.trimTrailingWhitespace\": true }",
    )
    .unwrap();

    let mut req = request("HarborEditor");
    req.vscode = true;
    req.stamp = 77;
    let mut calls = Vec::new();
    let outcome = setup(&project, &engine, &req, &mut fake_ubt(&mut calls)).unwrap();

    let backup = project.dir.join(".clangd.salvage-backup-77");
    assert_eq!(
        fs::read_to_string(&backup).unwrap(),
        "CompileFlags:\n  Add: [-DMINE]\n"
    );
    assert!(outcome.files.iter().any(|(p, w)| p.ends_with(CLANGD_FILE)
        && *w
            == Written::Replaced {
                backup: backup.clone()
            }));
    assert!(
        project
            .dir
            .join(".vscode/settings.json.salvage-backup-77")
            .is_file()
    );
    let settings: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(project.dir.join(VSCODE_FILE)).unwrap()).unwrap();
    assert_eq!(settings["files.trimTrailingWhitespace"], true);
}

#[test]
fn a_failed_build_tool_run_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    let project = harbor(dir.path());
    let engine = engine(dir.path());
    let error = setup(&project, &engine, &request("HarborEditor"), &mut |_| {
        Err("UnrealBuildTool exited with 6".to_string())
    })
    .unwrap_err();
    assert_eq!(error, "UnrealBuildTool exited with 6");
    assert!(
        !project.dir.join(CLANGD_FILE).exists(),
        "nothing written after a failure"
    );

    let silent = setup(&project, &engine, &request("HarborEditor"), &mut |_| Ok(())).unwrap_err();
    assert!(silent.contains("didn't write"), "{silent}");
}
