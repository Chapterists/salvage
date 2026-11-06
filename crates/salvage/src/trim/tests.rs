use std::fs;
use std::path::Path;

use super::*;
use crate::ini;

fn project(dir: &Path) -> Project {
    let root = dir.join("Harbor");
    fs::create_dir_all(root.join("Config")).unwrap();
    fs::write(root.join("Harbor.uproject"), "{}").unwrap();
    Project::open(&root).unwrap()
}

fn read(project: &Project, relative: &str) -> String {
    fs::read_to_string(project.dir.join(relative)).unwrap()
}

/// The cvars a profile sets for one bucket, as "name=value".
fn bucket_cvars(text: &str, profile: &str, bucket: &str) -> Vec<String> {
    ini::resolve_array(
        &[text],
        &format!("{profile} DeviceProfile"),
        &format!("CVars_{bucket}"),
    )
}

#[test]
fn every_value_has_a_reason_right_above_it() {
    for (file, block) in plan(true) {
        let mut previous = "";
        for line in block.lines() {
            let line = line.trim();
            let is_value = !line.is_empty() && !line.starts_with(';') && !line.starts_with('[');
            if is_value {
                assert!(
                    previous.starts_with(';'),
                    "{file}: no reason above {line:?}"
                );
            }
            previous = line;
        }
    }
}

#[test]
fn tiers_land_in_the_right_buckets() {
    let block = &plan(false)[0].1;
    let mac_8 = bucket_cvars(block, "Mac", "Smallest");
    assert!(
        mac_8.contains(&"r.Streaming.PoolSize=512".to_string()),
        "{mac_8:?}"
    );
    assert!(mac_8.contains(&"r.Nanite.Streaming.StreamingPoolSize=256".to_string()));
    assert!(mac_8.contains(&"foliage.DiscardDataOnLoad=1".to_string()));
    assert!(
        !mac_8.iter().any(|c| c.starts_with("grass.")),
        "grass is left alone"
    );
    assert!(
        bucket_cvars(block, "Mac", "Smaller").contains(&"r.Streaming.PoolSize=768".to_string())
    );
    assert!(
        bucket_cvars(block, "Mac", "Default").contains(&"r.Streaming.PoolSize=1000".to_string())
    );

    // The texture pool tiers are Mac-only; Windows and Linux size it from dedicated VRAM.
    let windows_8 = bucket_cvars(block, "Windows", "Smallest");
    assert!(
        !windows_8
            .iter()
            .any(|c| c.starts_with("r.Streaming.PoolSize"))
    );
    assert!(windows_8.contains(&"r.Shadow.Virtual.MaxPhysicalPages=768".to_string()));
    assert!(
        bucket_cvars(block, "Mac", "Larger").is_empty(),
        "bigger machines are untouched"
    );

    // No editor settings unless asked for.
    assert!(!block.contains("r.ShaderCompiler.MemoryLimit"));
    assert!(
        bucket_cvars(&plan(true)[0].1, "Linux", "Smaller")
            .contains(&"r.ShaderCompiler.MemoryLimit=3072".to_string())
    );
}

#[test]
fn buckets_never_set_the_smallest_threshold() {
    for (file, block) in plan(true) {
        assert!(
            !block.contains("SmallestMemoryBucket_MinGB="),
            "{file} would make the engine refuse to start"
        );
    }
    let mac = &plan(false)
        .into_iter()
        .find(|(f, _)| f == "Config/Mac/MacEngine.ini")
        .unwrap()
        .1;
    let entries = ini::parse(mac);
    let bucket = |key: &str| {
        entries
            .iter()
            .find(|e| e.section == "PlatformMemoryBuckets" && e.key == key)
            .map(|e| e.value.clone())
    };
    assert_eq!(bucket("DefaultMemoryBucket_MinGB").as_deref(), Some("14"));
    assert_eq!(bucket("SmallerMemoryBucket_MinGB").as_deref(), Some("10"));
    assert_eq!(bucket("LargerMemoryBucket_MinGB").as_deref(), Some("17"));
    assert!(entries.iter().any(|e| e.section == "TextureStreaming"
        && e.key == "PoolSizeVRAMPercentage"
        && e.value == "0"));
    let windows = &plan(false)
        .into_iter()
        .find(|(f, _)| f == "Config/Windows/WindowsEngine.ini")
        .unwrap()
        .1;
    assert!(!windows.contains("PoolSizeVRAMPercentage"));
    assert!(windows.contains("[/Script/WindowsTargetPlatform.WindowsTargetSettings]\n"));
}

#[test]
fn apply_appends_backs_up_and_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let project = project(dir.path());
    let mine =
        "[/Script/Engine.Engine]\n+ActiveGameNameRedirects=(OldGameName=\"A\",NewGameName=\"B\")\n";
    fs::write(project.dir.join("Config/DefaultEngine.ini"), mine).unwrap();
    let profiles =
        "[/Script/Engine.DeviceProfileManager]\n\n[Mac DeviceProfile]\n+CVars=r.Mine=1\n";
    fs::write(
        project.dir.join("Config/DefaultDeviceProfiles.ini"),
        profiles,
    )
    .unwrap();

    let results = apply(&project, true, 5).unwrap();
    let engine = read(&project, "Config/DefaultEngine.ini");
    assert!(engine.starts_with(mine), "user content first and unchanged");
    assert!(engine.contains(BEGIN) && engine.trim_end().ends_with(END));
    assert_eq!(
        read(&project, "Config/DefaultEngine.ini.salvage-backup-5"),
        mine
    );
    let dp = read(&project, "Config/DefaultDeviceProfiles.ini");
    assert!(
        ini::resolve_array(&[dp.as_str()], "Mac DeviceProfile", "CVars")
            .contains(&"r.Mine=1".to_string())
    );
    assert!(
        results
            .iter()
            .any(|(p, w)| p.ends_with("Config/Mac/MacEngine.ini") && *w == Written::Created)
    );
    assert!(is_applied(&project));

    let again = apply(&project, true, 6).unwrap();
    assert!(
        again.iter().all(|(_, w)| *w == Written::Unchanged),
        "{again:?}"
    );
    assert!(
        !project
            .dir
            .join("Config/DefaultEngine.ini.salvage-backup-6")
            .exists()
    );
}

#[test]
fn dropping_the_editor_preset_removes_only_its_blocks() {
    let dir = tempfile::tempdir().unwrap();
    let project = project(dir.path());
    apply(&project, true, 1).unwrap();
    assert!(
        project
            .dir
            .join("Config/DefaultEditorPerProjectUserSettings.ini")
            .exists()
    );
    apply(&project, false, 2).unwrap();
    assert!(
        !project
            .dir
            .join("Config/DefaultEditorPerProjectUserSettings.ini")
            .exists(),
        "file only held our block"
    );
    assert!(
        !read(&project, "Config/DefaultDeviceProfiles.ini")
            .contains("r.ShaderCompiler.MemoryLimit")
    );
    assert!(read(&project, "Config/Mac/MacEngine.ini").contains("PoolSizeVRAMPercentage=0"));
}

#[test]
fn undo_puts_everything_back() {
    let dir = tempfile::tempdir().unwrap();
    let project = project(dir.path());
    let mine = "; my settings\n[/Script/EngineSettings.GeneralProjectSettings]\nProjectID=1234\n";
    fs::write(project.dir.join("Config/DefaultEngine.ini"), mine).unwrap();
    let no_newline = "[TextureStreaming]\nPoolSizeVRAMPercentage=50";
    fs::create_dir_all(project.dir.join("Config/Mac")).unwrap();
    fs::write(project.dir.join("Config/Mac/MacEngine.ini"), no_newline).unwrap();

    apply(&project, true, 1).unwrap();
    let results = undo(&project, 2).unwrap();
    assert!(!results.is_empty());
    assert_eq!(
        read(&project, "Config/DefaultEngine.ini"),
        mine,
        "byte for byte"
    );
    assert_eq!(
        read(&project, "Config/Mac/MacEngine.ini").trim_end(),
        no_newline
    );
    assert!(
        !project
            .dir
            .join("Config/DefaultDeviceProfiles.ini")
            .exists(),
        "created by us, so removed"
    );
    assert!(
        !project
            .dir
            .join("Config/Windows/WindowsEngine.ini")
            .exists()
    );
    assert!(!is_applied(&project));
    assert!(
        undo(&project, 3).unwrap().is_empty(),
        "nothing left to undo"
    );
}

#[test]
fn a_block_edited_in_the_middle_of_a_file_is_replaced_in_place() {
    let dir = tempfile::tempdir().unwrap();
    let project = project(dir.path());
    let path = project.dir.join("Config/DefaultEngine.ini");
    fs::write(
        &path,
        format!("[A]\nX=1\n\n{BEGIN}\n[ConsoleVariables]\nr.Old=1\n{END}\n[B]\nY=2\n"),
    )
    .unwrap();
    upsert_block(&path, "[ConsoleVariables]\nr.New=1\n", 9).unwrap();
    assert_eq!(
        read(&project, "Config/DefaultEngine.ini"),
        format!("[A]\nX=1\n\n{BEGIN}\n[ConsoleVariables]\nr.New=1\n{END}\n[B]\nY=2\n")
    );
    remove_block(&path, 10).unwrap();
    assert_eq!(
        read(&project, "Config/DefaultEngine.ini"),
        "[A]\nX=1\n[B]\nY=2\n"
    );
}
