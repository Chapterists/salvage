//! Every plugin here is invented for these tests and built in a temp folder.

use std::fs;
use std::path::{Path, PathBuf};

use super::package::{compile_actions, macho_architectures};
use super::scan::{Finding, Verdict};
use super::winapi::{self, Hit};
use super::*;

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

/// A plugin with one module per (name, allow list) and build rules for each.
fn plugin(
    root: &Path,
    name: &str,
    modules: &[(&str, Option<&str>)],
    supported: Option<&str>,
) -> PathBuf {
    let dir = root.join(name);
    let mut entries = Vec::new();
    for (module, allow) in modules {
        let allow = allow
            .map(|list| format!(",\n\t\t\t\"PlatformAllowList\": [ {list} ]"))
            .unwrap_or_default();
        entries.push(format!(
            "\t\t{{\n\t\t\t\"Name\": \"{module}\",\n\t\t\t\"Type\": \"Runtime\"{allow}\n\t\t}}"
        ));
        write(
            &dir.join(format!("Source/{module}/{module}.Build.cs")),
            &format!("public class {module} : ModuleRules {{ }}\n"),
        );
        write(
            &dir.join(format!("Source/{module}/Private/{module}Module.cpp")),
            "#include \"Modules/ModuleManager.h\"\n",
        );
    }
    let supported = supported
        .map(|list| format!("\t\"SupportedTargetPlatforms\": [\n\t\t{list}\n\t],\n"))
        .unwrap_or_default();
    write(
        &dir.join(format!("{name}.uplugin")),
        &format!(
            "{{\n\t\"FileVersion\": 3,\n\t\"FriendlyName\": \"{name}\",\n{supported}\t\"Modules\": [\n{}\n\t]\n}}\n",
            entries.join(",\n")
        ),
    );
    dir
}

/// A thin arm64 Mach-O file, with Unreal's module marker if `module`.
fn fake_dylib(module: bool) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&0xfeed_facfu32.to_le_bytes());
    bytes.extend_from_slice(&0x0100_000cu32.to_le_bytes());
    bytes.extend_from_slice(&[0; 24]);
    if module {
        bytes.extend_from_slice(b"\0_ThisIsAnUnrealEngineModule\0");
    }
    bytes
}

fn add_mac_binaries(dir: &Path, modules: &[&str]) {
    let entries: Vec<String> = modules
        .iter()
        .map(|m| format!("\"{m}\": \"libUnrealEditor-{m}.dylib\""))
        .collect();
    write(
        &dir.join("Binaries/Mac/UnrealEditor.modules"),
        &format!(
            "{{ \"BuildId\": \"1\", \"Modules\": {{ {} }} }}",
            entries.join(", ")
        ),
    );
    for m in modules {
        fs::write(
            dir.join(format!("Binaries/Mac/libUnrealEditor-{m}.dylib")),
            fake_dylib(true),
        )
        .unwrap();
    }
}

fn scan_dir(dir: &Path) -> Plugin {
    scan::scan(&scan::uplugin_in(dir).unwrap(), true).unwrap()
}

#[test]
fn a_plugin_with_mac_binaries_works() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = plugin(tmp.path(), "Lantern", &[("Lantern", None)], None);
    add_mac_binaries(&dir, &["Lantern"]);
    let plugin = scan_dir(&dir);
    assert_eq!(plugin.verdict, Verdict::Works);
    assert_eq!(
        plugin.modules[0].mac_binary.as_deref(),
        Some("Binaries/Mac/libUnrealEditor-Lantern.dylib")
    );
}

#[test]
fn an_allow_list_without_mac_is_the_only_blocker() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = plugin(
        tmp.path(),
        "Rigging",
        &[
            ("Rigging", Some("\"Win64\"")),
            ("RiggingEditor", Some("\"Win64\", \"Linux\"")),
        ],
        Some("\"Win64\""),
    );
    let plugin = scan_dir(&dir);
    assert_eq!(plugin.verdict, Verdict::PatchAndRebuild);
    assert!(plugin.findings.contains(&Finding::AllowList {
        module: Some("RiggingEditor".into()),
        key: "PlatformAllowList".into(),
        platforms: vec!["Win64".into(), "Linux".into()],
    }));
    assert!(plugin.findings.contains(&Finding::AllowList {
        module: None,
        key: "SupportedTargetPlatforms".into(),
        platforms: vec!["Win64".into()],
    }));
}

#[test]
fn a_deny_list_naming_mac_blocks_it_too() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("Keel");
    write(
        &dir.join("Keel.uplugin"),
        "{ \"Modules\": [ { \"Name\": \"Keel\", \"Type\": \"Editor\", \"PlatformDenyList\": [\"Mac\"] } ] }",
    );
    write(&dir.join("Source/Keel/Keel.Build.cs"), "class Keel {}\n");
    let plugin = scan_dir(&dir);
    assert_eq!(plugin.verdict, Verdict::PatchAndRebuild);
    assert!(
        matches!(&plugin.findings[0], Finding::AllowList { key, .. } if key == "PlatformDenyList")
    );
}

#[test]
fn source_that_builds_as_is_just_needs_a_rebuild() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = plugin(tmp.path(), "Compass", &[("Compass", None)], None);
    assert_eq!(scan_dir(&dir).verdict, Verdict::Rebuild);
}

#[test]
fn windows_only_libraries_need_porting_until_a_mac_one_is_there() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = plugin(tmp.path(), "Sonar", &[("Sonar", Some("\"Win64\""))], None);
    write(
        &dir.join("Source/ThirdParty/SonarLib/SonarLib.Build.cs"),
        "class SonarLib {}\n",
    );
    write(
        &dir.join("Source/ThirdParty/SonarLib/lib/Win64/sonar.lib"),
        "",
    );
    write(
        &dir.join("Binaries/ThirdParty/SonarLib/Win64/sonar.dll"),
        "",
    );
    let plugin = scan_dir(&dir);
    assert_eq!(plugin.verdict, Verdict::NeedsPorting);
    let library = plugin
        .findings
        .iter()
        .find_map(|f| match f {
            Finding::WindowsOnlyLibrary { folder, files } => Some((folder.clone(), files.len())),
            _ => None,
        })
        .expect("library finding");
    assert_eq!(library.0, "Source/ThirdParty/SonarLib");
    assert_eq!(library.1, 2, "the .lib and the .dll are one library");

    write(
        &dir.join("Source/ThirdParty/SonarLib/lib/Mac/libsonar.a"),
        "",
    );
    let plugin = scan_dir(&dir);
    assert_eq!(plugin.verdict, Verdict::PatchAndRebuild);
}

#[test]
fn unguarded_windows_api_needs_porting_and_guarded_api_does_not() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = plugin(tmp.path(), "Hull", &[("Hull", None)], None);
    write(
        &dir.join("Source/Hull/Private/Guarded.cpp"),
        "#if PLATFORM_WINDOWS\n#include <windows.h>\nvoid A() { CloseHandle(h); }\n#else\nvoid A() {}\n#endif\n",
    );
    assert_eq!(scan_dir(&dir).verdict, Verdict::Rebuild);

    write(
        &dir.join("Source/Hull/Private/Raw.cpp"),
        "#include \"Windows/AllowWindowsPlatformTypes.h\"\nvoid B() {\n  HANDLE h = CreateFileW(L\"x\", 0, 0, 0, 0, 0, 0);\n}\n",
    );
    let plugin = scan_dir(&dir);
    assert_eq!(plugin.verdict, Verdict::NeedsPorting);
    let hits: Vec<(String, usize, String)> = plugin
        .findings
        .iter()
        .filter_map(|f| match f {
            Finding::WindowsApi { file, line, what } => Some((file.clone(), *line, what.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(
        hits,
        vec![
            (
                "Source/Hull/Private/Raw.cpp".into(),
                1,
                "#include <Windows/AllowWindowsPlatformTypes.h>".into()
            ),
            (
                "Source/Hull/Private/Raw.cpp".into(),
                3,
                "CreateFileW()".into()
            ),
        ]
    );
}

#[test]
fn a_plugin_with_no_source_can_only_be_fixed_by_its_vendor() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("Mast");
    write(
        &dir.join("Mast.uplugin"),
        "{ \"Modules\": [ { \"Name\": \"Mast\", \"Type\": \"Runtime\" } ] }",
    );
    write(&dir.join("Binaries/Win64/UnrealEditor-Mast.dll"), "");
    let plugin = scan_dir(&dir);
    assert_eq!(plugin.verdict, Verdict::NoSource);
    assert!(plugin.findings.contains(&Finding::NoSource));
}

#[test]
fn windows_only_modules_beside_working_ones_are_partial() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = plugin(
        tmp.path(),
        "Rudder",
        &[("Rudder", None), ("RudderD3D", Some("\"Win64\""))],
        None,
    );
    add_mac_binaries(&dir, &["Rudder"]);
    assert_eq!(scan_dir(&dir).verdict, Verdict::Partial);
}

#[test]
fn content_only_plugins_work() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("Charts");
    write(
        &dir.join("Charts.uplugin"),
        "{ \"CanContainContent\": true }",
    );
    assert_eq!(scan_dir(&dir).verdict, Verdict::ContentOnly);
}

#[test]
fn finds_plugins_without_looking_inside_them() {
    let tmp = tempfile::tempdir().unwrap();
    plugin(
        &tmp.path().join("Marketplace"),
        "Anchor",
        &[("Anchor", None)],
        None,
    );
    plugin(tmp.path(), "Bell", &[("Bell", None)], None);
    write(&tmp.path().join("Bell/Extras/Inner/Inner.uplugin"), "{}");
    write(
        &tmp.path().join("Bell/Intermediate/Junk/Junk.uplugin"),
        "{}",
    );
    let found: Vec<String> = scan::find_plugins(tmp.path())
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    // Sorted by path: Bell/ comes before Marketplace/Anchor/.
    assert_eq!(found, vec!["Bell.uplugin", "Anchor.uplugin"]);
}

#[test]
fn the_api_scanner_ignores_comments_strings_and_member_functions() {
    let text = "// CreateFileW(x)\n/* LoadLibraryW(y)\n */\nconst char* s = \"GetProcAddress(\";\nThing.CloseHandle();\nFFoo::CloseHandle(h);\n";
    assert!(winapi::scan(text).is_empty(), "{:?}", winapi::scan(text));
    assert_eq!(
        winapi::scan("void F() { ::CloseHandle(h); }\n"),
        vec![Hit::Api {
            line: 1,
            what: "CloseHandle()".into()
        }]
    );
}

#[test]
fn the_api_scanner_follows_ifdefs() {
    // #ifndef _WIN32 ... #else: the else branch is the Windows one.
    let text = "#ifndef _WIN32\nint x;\n#else\n#include <windows.h>\n#endif\n";
    assert!(winapi::scan(text).is_empty());
    // A library's own switch counts as Windows.
    let text = "#if FOO_USE_MSVC\n#include <Windows.h>\n#endif\n";
    assert_eq!(winapi::scan(text), vec![Hit::OnlyBlock { line: 1 }]);
    // Windows-only with no else is noted, but isn't API use.
    let text = "#if PLATFORM_WINDOWS\nvoid Tray();\n#endif\n#if PLATFORM_WINDOWS || PLATFORM_MAC\nHWND w;\n#endif\n";
    assert_eq!(
        winapi::scan(text),
        vec![
            Hit::OnlyBlock { line: 1 },
            Hit::Api {
                line: 5,
                what: "HWND".into()
            }
        ]
    );
}

const UPLUGIN: &str = "{\n\t\"FileVersion\": 3,\n\t\"SupportedTargetPlatforms\": [\n\t\t\"Win64\"\n\t],\n\t\"Modules\": [\n\t\t{\n\t\t\t\"Name\": \"Sail\",\n\t\t\t\"PlatformAllowList\": [ \"Win64\" ]\n\t\t},\n\t\t{\n\t\t\t\"Name\": \"SailEditor\",\n\t\t\t\"PlatformDenyList\": [ \"Linux\", \"Mac\" ]\n\t\t}\n\t]\n}\n";

fn project(root: &Path) -> PathBuf {
    let dir = root.join("Harbor");
    write(&dir.join("Harbor.uproject"), "{}");
    dir
}

#[test]
fn patch_adds_mac_in_place_and_undo_puts_every_byte_back() {
    let tmp = tempfile::tempdir().unwrap();
    let project = project(tmp.path());
    let file = project.join("Plugins/Sail/Sail.uplugin");
    write(&file, UPLUGIN);

    let patched = patch::patch(&file, &project, 7, false).unwrap();
    assert_eq!(patched.copied_from, None);
    assert_eq!(
        patched.changes,
        vec![
            "plugin: SupportedTargetPlatforms + Mac",
            "Sail: PlatformAllowList + Mac",
            "SailEditor: PlatformDenyList - Mac",
        ]
    );
    let text = fs::read_to_string(&file).unwrap();
    assert!(
        text.contains("\t\t\"Win64\",\n\t\t\"Mac\"\n\t]"),
        "keeps the layout:\n{text}"
    );
    assert!(text.contains("\"PlatformAllowList\": [ \"Win64\", \"Mac\" ]"));
    assert!(text.contains("\"PlatformDenyList\": [ \"Linux\" ]"));
    assert_eq!(
        fs::read_to_string(project.join("Plugins/Sail/Sail.uplugin.salvage-backup-7")).unwrap(),
        UPLUGIN
    );
    assert_eq!(
        scan_dir(&project.join("Plugins/Sail"))
            .findings
            .iter()
            .filter(|f| matches!(f, Finding::AllowList { .. }))
            .count(),
        0
    );

    let again = patch::patch(&file, &project, 8, false).unwrap();
    assert!(again.changes.is_empty(), "idempotent");
    assert!(
        !project
            .join("Plugins/Sail/Sail.uplugin.salvage-backup-8")
            .exists()
    );

    assert_eq!(
        patch::undo(&project.join("Plugins/Sail")).unwrap(),
        Undone::Restored
    );
    assert_eq!(fs::read_to_string(&file).unwrap(), UPLUGIN);
    assert_eq!(
        patch::undo(&project.join("Plugins/Sail")).unwrap(),
        Undone::NotPatched
    );
}

#[test]
fn a_dry_run_patches_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let project = project(tmp.path());
    let file = project.join("Plugins/Sail/Sail.uplugin");
    write(&file, UPLUGIN);
    let preview = patch::patch(&file, &project, 1, true).unwrap();
    assert_eq!(preview.changes.len(), 3);
    assert_eq!(fs::read_to_string(&file).unwrap(), UPLUGIN);
    assert!(!project.join("Plugins/Sail").join(patch::RECORD).exists());
}

#[test]
fn a_plugin_outside_the_project_is_copied_and_the_original_left_alone() {
    let tmp = tempfile::tempdir().unwrap();
    let project = project(tmp.path());
    let engine_copy = tmp.path().join("Engine/Plugins/Sail");
    write(&engine_copy.join("Sail.uplugin"), UPLUGIN);
    write(
        &engine_copy.join("Source/Sail/Sail.Build.cs"),
        "class Sail {}\n",
    );
    write(
        &engine_copy.join("Binaries/Win64/UnrealEditor-Sail.dll"),
        "",
    );

    let patched = patch::patch(&engine_copy.join("Sail.uplugin"), &project, 3, false).unwrap();
    let copy = project.join("Plugins/Sail");
    assert_eq!(patched.dir, copy);
    assert_eq!(patched.copied_from.as_deref(), Some(engine_copy.as_path()));
    assert_eq!(
        fs::read_to_string(engine_copy.join("Sail.uplugin")).unwrap(),
        UPLUGIN,
        "original untouched"
    );
    assert!(copy.join("Source/Sail/Sail.Build.cs").is_file());
    assert!(!copy.join("Binaries").exists(), "build output isn't copied");

    // Patching the original again finds the copy and has nothing to do.
    assert!(
        patch::patch(&engine_copy.join("Sail.uplugin"), &project, 4, false)
            .unwrap()
            .changes
            .is_empty()
    );

    // Build output alone doesn't count as a change: undo removes the copy.
    write(&copy.join("Binaries/Mac/libUnrealEditor-Sail.dylib"), "");
    write(&copy.join("Config/FilterPlugin.ini"), "[FilterPlugin]\n");
    assert_eq!(patch::undo(&copy).unwrap(), Undone::RemovedCopy);
    assert!(!copy.exists());
}

#[test]
fn undo_keeps_a_copy_someone_worked_in() {
    let tmp = tempfile::tempdir().unwrap();
    let project = project(tmp.path());
    let original = tmp.path().join("Elsewhere/Sail");
    write(&original.join("Sail.uplugin"), UPLUGIN);
    patch::patch(&original.join("Sail.uplugin"), &project, 1, false).unwrap();
    let copy = project.join("Plugins/Sail");
    write(&copy.join("Source/ThirdParty/Lib/Mac/libsail.a"), "port");

    match patch::undo(&copy).unwrap() {
        Undone::RestoredKeptCopy { changed } => {
            assert_eq!(changed, vec!["Source/ThirdParty/Lib/Mac/libsail.a"])
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        fs::read_to_string(copy.join("Sail.uplugin")).unwrap(),
        UPLUGIN
    );
}

#[test]
fn undo_refuses_when_the_uplugin_changed_since() {
    let tmp = tempfile::tempdir().unwrap();
    let project = project(tmp.path());
    let file = project.join("Plugins/Sail/Sail.uplugin");
    write(&file, UPLUGIN);
    patch::patch(&file, &project, 1, false).unwrap();
    fs::write(&file, "{ \"edited\": true }").unwrap();
    assert!(patch::undo(&project.join("Plugins/Sail")).is_err());
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "{ \"edited\": true }",
        "left alone"
    );
}

const TRAP_LOG: &str = "Building plugin for host platforms: Mac\nBuilding plugin for target platforms: \nBUILD SUCCESSFUL\n";

#[test]
fn a_build_that_compiled_nothing_is_caught() {
    let tmp = tempfile::tempdir().unwrap();
    // What BuildPlugin leaves for a plugin whose allow lists leave Mac out:
    // the descriptor and source, and no binaries.
    let package = plugin(
        tmp.path(),
        "Sail",
        &[("Sail", Some("\"Win64\""))],
        Some("\"Win64\""),
    );
    let check = check(&package, Some(TRAP_LOG)).unwrap();
    assert!(!check.ok());
    assert_eq!(check.compiled, Some(0));
    assert!(
        check
            .problems
            .iter()
            .any(|p| p.contains("allow lists leave Mac out"))
    );
    assert!(
        check
            .problems
            .iter()
            .any(|p| p.contains("no target platform"))
    );
}

#[test]
fn a_package_missing_a_module_binary_fails() {
    let tmp = tempfile::tempdir().unwrap();
    let package = plugin(
        tmp.path(),
        "Sail",
        &[("Sail", None), ("SailEditor", None)],
        None,
    );
    add_mac_binaries(&package, &["Sail"]);
    let check = check(&package, None).unwrap();
    assert_eq!(check.problems, vec!["SailEditor: no Mac binary"]);
}

#[test]
fn a_binary_without_the_module_marker_fails_and_a_real_one_passes() {
    let tmp = tempfile::tempdir().unwrap();
    let package = plugin(tmp.path(), "Sail", &[("Sail", None)], None);
    add_mac_binaries(&package, &["Sail"]);
    let log = "Building plugin for target platforms: Mac\n[1/2] Compile [Apple] Sail.cpp\n[2/2] Link [Apple] libUnrealEditor-Sail.dylib\n";
    let good = check(&package, Some(log)).unwrap();
    assert!(good.ok(), "{:?}", good.problems);
    assert_eq!(good.compiled, Some(1));
    assert_eq!(good.modules[0].architectures, vec!["arm64"]);

    fs::write(
        package.join("Binaries/Mac/libUnrealEditor-Sail.dylib"),
        fake_dylib(false),
    )
    .unwrap();
    let bad = check(&package, Some(log)).unwrap();
    assert!(bad.problems[0].contains("isn't an Unreal module binary"));
}

#[test]
fn reads_universal_binaries() {
    let mut fat = Vec::new();
    fat.extend_from_slice(&0xcafe_babeu32.to_be_bytes());
    fat.extend_from_slice(&2u32.to_be_bytes());
    for cpu in [0x0100_0007u32, 0x0100_000c] {
        fat.extend_from_slice(&cpu.to_be_bytes());
        fat.extend_from_slice(&[0; 16]);
    }
    assert_eq!(macho_architectures(&fat), vec!["x86_64", "arm64"]);
    assert!(macho_architectures(b"MZ not a mach-o").is_empty());
    assert_eq!(
        compile_actions(
            "[1/3] Compile [Apple] A.cpp\n  [2/3] Compile [Intel] A.cpp\n[3/3] Link X\nCompile nothing\n"
        ),
        2
    );
}

#[test]
fn engine_installs_are_recognised() {
    let tmp = tempfile::tempdir().unwrap();
    write(&tmp.path().join("UE_5.8/Engine/Build/Build.version"), "{}");
    assert!(inside_engine(&tmp.path().join("UE_5.8/Engine/Plugins")));
    assert!(!inside_engine(tmp.path()));
}

#[test]
fn mac_binaries_held_back_by_a_list_need_only_the_patch() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = plugin(tmp.path(), "Wake", &[("Wake", None)], Some("\"Win64\""));
    add_mac_binaries(&dir, &["Wake"]);
    assert_eq!(scan_dir(&dir).verdict, Verdict::PatchOnly);
}

#[test]
fn a_byte_order_mark_is_skipped() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("Buoy");
    write(
        &dir.join("Buoy.uplugin"),
        "\u{feff}{ \"CanContainContent\": true }",
    );
    assert_eq!(scan_dir(&dir).verdict, Verdict::ContentOnly);
}
