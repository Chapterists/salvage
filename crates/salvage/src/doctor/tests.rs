use super::*;
use crate::engine::{Engine, FoundBy};
use crate::probe::Memory;
use crate::testing::{FakeProbe, fake_engine};
use crate::units::GIB;

struct Setup {
    dir: tempfile::TempDir,
    probe: FakeProbe,
    engine: Option<Engine>,
    project: Option<Project>,
}

impl Setup {
    /// A healthy Mac with a fixture engine and Unreal's LLDB formatters loaded.
    fn mac() -> Setup {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        std::fs::create_dir_all(&home).unwrap();
        let root = fake_engine(&dir.path().join("UE_9.9"), (9, 9, 1));
        let formatters =
            root.join("Engine/Extras/LLDBDataFormatters/UEDataFormatters_2ByteChars.py");
        std::fs::write(
            home.join(".lldbinit"),
            format!(
                "# mine\ncommand script import \"{}\"\n",
                formatters.display()
            ),
        )
        .unwrap();
        let probe = FakeProbe::healthy_mac(&home);
        let engine = Some(Engine::open(&root, FoundBy::Flag).unwrap());
        Setup {
            dir,
            probe,
            engine,
            project: None,
        }
    }

    fn home(&self) -> std::path::PathBuf {
        self.probe.home.clone().unwrap()
    }

    /// Adds a project whose config targets only SM6.
    fn with_project(mut self, folder: &str) -> Setup {
        let dir = self.dir.path().join(folder);
        std::fs::create_dir_all(dir.join("Config")).unwrap();
        std::fs::write(
            dir.join("Harbor.uproject"),
            r#"{ "EngineAssociation": "9.9" }"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("Config/DefaultEngine.ini"),
            "[/Script/MacTargetPlatform.MacTargetSettings]\n-TargetedRHIs=SF_METAL_SM5\n",
        )
        .unwrap();
        self.project = Some(Project::open(&dir).unwrap());
        self
    }

    fn run(&self) -> Report {
        run(&self.probe, self.engine.clone(), self.project.clone())
    }
}

fn status(report: &Report, id: &str) -> Status {
    report
        .check(id)
        .unwrap_or_else(|| panic!("no check {id}"))
        .status
}

fn fix_command(report: &Report, id: &str) -> String {
    report
        .check(id)
        .unwrap()
        .fix
        .as_ref()
        .unwrap()
        .command
        .clone()
        .unwrap_or_default()
}

#[test]
fn a_healthy_mac_with_a_tidy_project_passes_everything() {
    let report = Setup::mac().with_project("Harbor").run();
    let failing: Vec<_> = report
        .checks
        .iter()
        .filter(|c| c.status != Status::Pass)
        .map(|c| c.id)
        .collect();
    assert!(failing.is_empty(), "expected all passes, got {failing:?}");
    assert!(report.ok());
    assert_eq!(report.counts().pass, report.checks.len());
}

#[test]
fn every_problem_comes_with_a_fix() {
    let mut setup = Setup::mac();
    setup.probe.err(
        "xcrun metal --version",
        "error: cannot execute tool 'metal' due to missing Metal Toolchain\n",
    );
    setup.probe.rosetta = false;
    setup.probe.free_space = Some(5 * GIB);
    std::fs::remove_file(setup.home().join(".lldbinit")).unwrap();
    let report = setup.run();
    for check in report
        .checks
        .iter()
        .filter(|c| matches!(c.status, Status::Warn | Status::Fail))
    {
        assert!(check.fix.is_some(), "{} has no fix", check.id);
    }
}

#[test]
fn missing_metal_toolchain_fails_with_the_download_command() {
    let mut setup = Setup::mac();
    setup.probe.err(
        "xcrun metal --version",
        "error: error: cannot execute tool 'metal' due to missing Metal Toolchain; use: xcodebuild -downloadComponent MetalToolchain\n",
    );
    let report = setup.run();
    assert_eq!(status(&report, "metal-toolchain"), Status::Fail);
    assert_eq!(
        fix_command(&report, "metal-toolchain"),
        "xcodebuild -downloadComponent MetalToolchain"
    );
    assert!(!report.ok());
}

#[test]
fn xcode_is_checked_against_the_engines_range() {
    let mut setup = Setup::mac();
    let report = setup.run();
    let xcode = report.check("xcode").unwrap();
    assert_eq!(xcode.status, Status::Pass);
    assert!(
        xcode.summary.contains("29.0.0 to 31.9.0"),
        "{}",
        xcode.summary
    );
    assert!(
        xcode.details.is_empty(),
        "30.1 is the main version, so no note"
    );

    setup
        .probe
        .ok("xcodebuild -version", "Xcode 31.2\nBuild version 31B1\n");
    let xcode = setup.run().check("xcode").unwrap().clone();
    assert_eq!(xcode.status, Status::Pass);
    assert!(xcode.details[0].contains("30.1.0"));

    setup
        .probe
        .ok("xcodebuild -version", "Xcode 32.0\nBuild version 32A1\n");
    let report = setup.run();
    assert_eq!(status(&report, "xcode"), Status::Fail);
    assert!(report.check("xcode").unwrap().summary.contains("newer"));
    // Pinned per build with salvage xcode, never sudo xcode-select.
    assert_eq!(fix_command(&report, "xcode"), "salvage xcode");

    setup
        .probe
        .ok("xcodebuild -version", "Xcode 28.4\nBuild version 28E1\n");
    let report = setup.run();
    assert_eq!(status(&report, "xcode"), Status::Fail);
    assert!(report.check("xcode").unwrap().summary.contains("older"));
}

#[test]
fn xcode_epic_says_is_not_compatible_warns_inside_the_range() {
    let mut setup = Setup::mac();
    let root = fake_engine(&setup.dir.path().join("UE_5.8"), (5, 8, 4));
    std::fs::write(
        root.join("Engine/Config/Apple/Apple_SDK.json"),
        r#"{ "MainVersion": "26.1.1", "MinVersion": "15.2.0", "MaxVersion": "27.9.0" }"#,
    )
    .unwrap();
    setup.engine = Some(Engine::open(&root, FoundBy::Flag).unwrap());
    setup
        .probe
        .ok("xcodebuild -version", "Xcode 26.4\nBuild version 17E1\n");
    let report = setup.run();
    let xcode = report.check("xcode").unwrap();
    assert_eq!(xcode.status, Status::Warn, "a warning, not a failure");
    assert!(
        xcode
            .details
            .iter()
            .any(|d| d.contains("26.4 is not compatible"))
    );
    assert_eq!(fix_command(&report, "xcode"), "salvage xcode");

    setup
        .probe
        .ok("xcodebuild -version", "Xcode 26.1.1\nBuild version 17B1\n");
    let xcode = setup.run().check("xcode").unwrap().clone();
    assert_eq!(xcode.status, Status::Pass);
    assert!(
        xcode
            .details
            .iter()
            .any(|d| d.contains("Epic recommends Xcode 26.1.1"))
    );
}

#[test]
fn command_line_tools_alone_fail_both_xcode_checks() {
    let mut setup = Setup::mac();
    setup.probe.err(
        "xcodebuild -version",
        "xcode-select: error: tool 'xcodebuild' requires Xcode, but active developer directory '/Library/Developer/CommandLineTools' is a command line tools instance\n",
    );
    setup
        .probe
        .ok("xcode-select -p", "/Library/Developer/CommandLineTools\n");
    let report = setup.run();
    assert_eq!(status(&report, "xcode"), Status::Fail);
    assert_eq!(status(&report, "xcode-select"), Status::Fail);
    assert_eq!(
        fix_command(&report, "xcode-select"),
        "sudo xcode-select -s /Applications/Xcode.app"
    );
}

#[test]
fn missing_xcode_fails() {
    let mut setup = Setup::mac();
    setup.probe.commands.clear();
    let report = setup.run();
    assert_eq!(status(&report, "xcode"), Status::Fail);
    assert_eq!(status(&report, "metal-toolchain"), Status::Fail);
}

#[test]
fn rosetta_matters_only_on_apple_silicon() {
    let mut setup = Setup::mac();
    setup.probe.rosetta = false;
    let report = setup.run();
    assert_eq!(status(&report, "rosetta"), Status::Warn);
    assert!(fix_command(&report, "rosetta").starts_with("softwareupdate --install-rosetta"));

    setup.probe.arch = "x86_64".into();
    assert_eq!(status(&setup.run(), "rosetta"), Status::Skip);
}

#[test]
fn free_space_thresholds() {
    let mut setup = Setup::mac();
    setup.probe.free_space = Some(checks::DISK_FAIL_BELOW - 1);
    assert_eq!(status(&setup.run(), "free-space"), Status::Fail);
    setup.probe.free_space = Some(checks::DISK_WARN_BELOW - 1);
    assert_eq!(status(&setup.run(), "free-space"), Status::Warn);
    setup.probe.free_space = Some(checks::DISK_WARN_BELOW);
    assert_eq!(status(&setup.run(), "free-space"), Status::Pass);
}

#[test]
fn spaces_in_the_project_path_warn_but_the_engine_default_does_not() {
    let report = Setup::mac().with_project("My Harbor").run();
    assert_eq!(status(&report, "path-spaces"), Status::Warn);
    assert!(report.check("path-spaces").unwrap().details[0].contains("My Harbor"));

    let mut setup = Setup::mac();
    let spaced = setup.dir.path().join("Epic Games/UE_9.9");
    let root = fake_engine(&spaced, (9, 9, 1));
    setup.engine = Some(Engine::open(&root, FoundBy::Flag).unwrap());
    let check = setup.run().check("path-spaces").unwrap().clone();
    assert_eq!(check.status, Status::Pass);
    assert!(check.summary.contains("Only the engine path"));
}

#[test]
fn caches_are_found_and_sized() {
    let setup = Setup::mac();
    let report = setup.run();
    assert_eq!(
        report.check("caches").unwrap().summary,
        "No local caches yet"
    );

    let zen = setup
        .home()
        .join("Library/Application Support/Epic/Zen/Data/cas");
    std::fs::create_dir_all(&zen).unwrap();
    std::fs::write(zen.join("chunk"), vec![0u8; 3000]).unwrap();
    let ddc = setup
        .engine
        .as_ref()
        .unwrap()
        .dir()
        .join("DerivedDataCache");
    std::fs::create_dir_all(&ddc).unwrap();
    std::fs::write(ddc.join("entry"), vec![0u8; 1000]).unwrap();

    let check = setup.run().check("caches").unwrap().clone();
    assert_eq!(check.status, Status::Pass);
    assert!(
        check.summary.ends_with("across 2 caches"),
        "{}",
        check.summary
    );
    assert!(
        check.details[0].starts_with("Zen storage: 2.9 KB in ~/Library"),
        "{:?}",
        check.details
    );
    assert_eq!(checks::dir_size(&setup.home().join("Library")), 3000);
}

#[test]
fn heavy_swap_warns_and_small_ram_is_tight() {
    let mut setup = Setup::mac();
    setup.probe.memory = Some(Memory {
        total: 16 * GIB,
        swap_total: 10 * GIB,
        swap_used: 9 * GIB,
    });
    let report = setup.run();
    assert_eq!(status(&report, "swap"), Status::Warn);
    assert_eq!(status(&report, "ram"), Status::Pass);

    setup.probe.memory = Some(Memory {
        total: 8 * GIB,
        swap_total: 2 * GIB,
        swap_used: GIB,
    });
    let report = setup.run();
    assert_eq!(status(&report, "ram"), Status::Warn);
    assert_eq!(status(&report, "swap"), Status::Pass);
}

#[test]
fn lldbinit_states() {
    let setup = Setup::mac();
    let lldbinit = setup.home().join(".lldbinit");
    let check = setup.run().check("lldbinit").unwrap().clone();
    assert_eq!(check.status, Status::Pass);
    assert!(check.summary.starts_with("Loads the engine's formatters"));
    assert!(
        check.details[0].contains("salvage lldb install"),
        "recommends ours"
    );

    std::fs::write(
        &lldbinit,
        "command script import \"/gone/UEDataFormatters_2ByteChars.py\"\n",
    )
    .unwrap();
    let report = setup.run();
    assert_eq!(status(&report, "lldbinit"), Status::Warn);
    assert!(
        report
            .check("lldbinit")
            .unwrap()
            .summary
            .contains("no longer exists")
    );
    assert_eq!(fix_command(&report, "lldbinit"), "salvage lldb install");

    std::fs::write(
        &lldbinit,
        "settings set target.x86-disassembly-flavor intel\n",
    )
    .unwrap();
    assert_eq!(status(&setup.run(), "lldbinit"), Status::Warn);

    std::fs::remove_file(&lldbinit).unwrap();
    let report = setup.run();
    assert_eq!(status(&report, "lldbinit"), Status::Warn);
    assert_eq!(fix_command(&report, "lldbinit"), "salvage lldb install");

    // After `salvage lldb install`, doctor is happy.
    let layout = crate::lldb::Layout {
        home: setup.home(),
        data_dir: setup.home().join("Library/Application Support/Salvage"),
    };
    crate::lldb::install(&layout, 1).unwrap();
    let check = setup.run().check("lldbinit").unwrap().clone();
    assert_eq!(check.status, Status::Pass);
    assert!(
        check
            .summary
            .starts_with("Loads Salvage's Unreal formatters from ~/Library"),
        "{}",
        check.summary
    );
}

#[test]
fn shader_formats_follow_the_ini_layers() {
    let report = Setup::mac().run();
    assert_eq!(status(&report, "shader-platforms"), Status::Warn);

    let setup = Setup::mac().with_project("Harbor");
    let check = setup.run().check("shader-platforms").unwrap().clone();
    assert_eq!(check.status, Status::Pass);
    assert_eq!(check.summary, "Harbor targets SF_METAL_SM6");

    let config = setup
        .project
        .as_ref()
        .unwrap()
        .dir
        .join("Config/DefaultEngine.ini");
    std::fs::write(
        &config,
        "[/Script/MacTargetPlatform.MacTargetSettings]\n!TargetedRHIs=ClearArray\n",
    )
    .unwrap();
    assert_eq!(status(&setup.run(), "shader-platforms"), Status::Fail);
}

#[test]
fn without_an_engine_the_engine_checks_step_aside() {
    let mut setup = Setup::mac();
    setup.engine = None;
    let report = setup.run();
    assert_eq!(status(&report, "engine"), Status::Warn);
    assert_eq!(status(&report, "xcode"), Status::Warn);
    assert_eq!(status(&report, "shader-platforms"), Status::Skip);
    assert!(report.ok());
}

#[test]
fn on_linux_mac_checks_are_skipped_and_said_plainly() {
    let dir = tempfile::tempdir().unwrap();
    let probe = FakeProbe::linux(dir.path());
    let report = run(&probe, None, None);
    for id in [
        "xcode",
        "xcode-select",
        "metal-toolchain",
        "rosetta",
        "lldbinit",
        "shader-platforms",
    ] {
        assert_eq!(status(&report, id), Status::Skip, "{id}");
    }
    assert_eq!(status(&report, "ram"), Status::Pass);
    let text = strip(&render_human(&report, 100));
    assert!(text.contains("only apply on macOS"), "{text}");
    assert!(
        !text.contains("macOS only"),
        "skipped Mac rows should be folded into the note"
    );
    assert!(!text.contains("Xcode and Metal"));
}

#[test]
fn json_is_stable_and_machine_friendly() {
    let mut setup = Setup::mac();
    setup
        .probe
        .err("xcrun metal --version", "missing Metal Toolchain");
    let report = setup.run();
    let json: serde_json::Value = serde_json::from_str(&render_json(&report)).unwrap();
    assert_eq!(json["schema"], 1);
    assert_eq!(json["command"], "doctor");
    assert_eq!(json["ok"], false);
    assert_eq!(json["counts"]["fail"], 1);
    assert_eq!(json["platform"]["os"], "macos");
    assert_eq!(json["engine"]["version"], "9.9.1");
    assert_eq!(json["engine"]["found_by"], "flag");
    let metal = json["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "metal-toolchain")
        .unwrap();
    assert_eq!(metal["status"], "fail");
    assert_eq!(metal["group"], "toolchain");
    assert_eq!(
        metal["fix"]["command"],
        "xcodebuild -downloadComponent MetalToolchain"
    );
}

#[test]
fn human_output_groups_checks_and_sums_up() {
    let mut setup = Setup::mac();
    setup
        .probe
        .err("xcrun metal --version", "missing Metal Toolchain");
    let text = strip(&render_human(&setup.run(), 100));
    for heading in ["Engine", "Xcode and Metal", "Disk", "Memory", "Debugging"] {
        assert!(
            text.lines().any(|l| l.trim() == heading),
            "missing {heading}:\n{text}"
        );
    }
    assert!(text.contains("✗  Metal Toolchain"));
    assert!(text.contains("→ Download it"));
    assert!(text.contains("1 to fix, 1 worth a look"), "{text}");
    assert_eq!(
        text.matches("shipshape").count() + text.matches("Shipshape").count(),
        1
    );

    // The continuation lines line up under the summary column.
    let metal = text
        .lines()
        .position(|l| l.contains("Metal Toolchain"))
        .unwrap();
    let lines: Vec<&str> = text.lines().collect();
    let byte = lines[metal].find("Not installed").unwrap();
    let column = lines[metal][..byte].chars().count();
    let next = lines[metal + 1];
    assert_eq!(next.len() - next.trim_start().len(), column, "{next:?}");

    let all_good = Setup::mac().with_project("Harbor").run();
    assert!(strip(&render_human(&all_good, 100)).contains("Shipshape."));
}

#[test]
fn rendering_keeps_colors_out_of_the_text_itself() {
    let text = render_human(&Setup::mac().run(), 100);
    assert!(text.contains('\u{1b}'), "styled for terminals");
    assert!(!strip(&text).contains('\u{1b}'));
}

fn strip(text: &str) -> String {
    anstream::adapter::strip_str(text).to_string()
}
