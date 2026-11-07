//! Crash parsing, matching and bucketing, against hand-written fixtures with
//! invented UUIDs, addresses and paths, and a fake machine.

use std::path::{Path, PathBuf};

use super::bucket::{self, SignatureFrame};
use super::report::{self, Source};
use super::symbols::{self, Symbols};
use super::*;
use crate::testing::FakeProbe;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/crash")
        .join(name)
}

#[test]
fn reads_an_unreal_crash_folder_and_finds_the_thread_that_really_crashed() {
    let report = report::read(&fixture("unreal-folder")).unwrap();
    assert_eq!(report.source, Source::Unreal);
    let unreal = report.unreal.as_ref().unwrap();
    assert_eq!(unreal.crash_type.as_deref(), Some("Assert"));
    assert_eq!(unreal.game_name.as_deref(), Some("UE-Harbor"));
    assert!(unreal.is_assert && !unreal.is_ensure);
    assert_eq!(report.os_build.as_deref(), Some("99A123"));
    assert_eq!(report.images.len(), 5);
    assert_eq!(
        report.images[4].uuid.as_deref(),
        Some("CCCCCCCC-CCCC-CCCC-CCCC-CCCCCCCCCC01")
    );

    // The text report says thread 7 (Unreal's crash handler, SIGUSR2) crashed.
    // The crashing thread comes from the context's portable call stack instead.
    let crashed = report.crashed_thread().unwrap();
    assert!(crashed.crashed);
    assert_eq!(crashed.frames.len(), 4);
    assert_eq!(crashed.frames[0].address, 0x106000000 + 0x1200);
    // `Unknown 0x0 + d4b01a00` only kept the low 32 bits; the Harbor dylib's
    // range gives the rest, even though a macOS framework also fits.
    assert_eq!(crashed.frames[1].address, 0x70d4b01a00);
    assert_eq!(crashed.frames[1].module, "libUnrealEditor-Harbor.dylib");
    assert_eq!(crashed.frames[2].address, 0x70d4b02b00);
    assert_eq!(report.threads.iter().filter(|t| t.crashed).count(), 1);
}

#[test]
fn a_truncated_address_that_fits_two_of_our_images_is_not_guessed() {
    let image = |name: &str, base: u64, end: u64, path: &str| report::Image {
        name: name.into(),
        path: Some(PathBuf::from(path)),
        uuid: None,
        arch: None,
        base,
        end: Some(end),
    };
    let images = vec![
        image("A", 0x70_0000_0000, 0x70_ffff_ffff, "/Projects/A.dylib"),
        image("B", 0x71_0000_0000, 0x71_ffff_ffff, "/Projects/B.dylib"),
    ];
    assert_eq!(report::widen_truncated(0x1234, &images), None);
    assert_eq!(
        report::widen_truncated(0x1234, &images[..1]),
        Some((0, 0x70_0000_1234))
    );
}

#[test]
fn reads_a_macos_ips_report() {
    let report = report::read(&fixture("sinker.ips")).unwrap();
    assert_eq!(report.source, Source::Ips);
    assert_eq!(report.process.as_deref(), Some("sinker"));
    assert_eq!(
        report.exception.as_deref(),
        Some("EXC_BAD_ACCESS (SIGSEGV)")
    );
    assert_eq!(report.os_build.as_deref(), Some("99A123"));
    let crashed = report.crashed_thread().unwrap();
    assert_eq!(crashed.frames.len(), 4);
    assert_eq!(crashed.frames[0].address, 4297031680 + 16128);
    assert_eq!(crashed.frames[3].report_symbol.as_deref(), Some("start"));
    assert_eq!(
        report.images[0].path.as_deref(),
        Some(Path::new("/private/tmp/*/sinker"))
    );
    assert_eq!(report.images[0].end, Some(4297031680 + 16384 - 1));
}

#[test]
fn reads_a_text_crash_report() {
    let text = "Process:         Sloop [12]\nOS Version:      macOS 99.1 (99A123)\nException Type:  EXC_BAD_ACCESS (SIGSEGV)\nCrashed Thread:  1\n\nThread 0:\n0   Sloop    0x0000000100001000 main + 16\n\nThread 1 Crashed:\n0   Sloop    0x0000000100002000 0x100000000 + 8192\n1   libsystem_pthread.dylib  0x0000000180002000 _pthread_start + 136\n\nBinary Images:\n       0x100000000 -        0x100007fff Sloop arm64  <eeeeeeeeeeeeeeeeeeeeeeeeeeeeee01> /Applications/Sloop.app/Contents/MacOS/Sloop\n";
    let report = report::parse_text(Path::new("Sloop.crash"), text, Source::Text).unwrap();
    assert_eq!(report.process.as_deref(), Some("Sloop"));
    let crashed = report.crashed_thread().unwrap();
    assert_eq!(crashed.index, 1);
    assert_eq!(crashed.frames[0].address, 0x100002000);
    assert_eq!(crashed.frames[0].image, Some(0));
    assert_eq!(
        crashed.frames[1].report_symbol.as_deref(),
        Some("_pthread_start")
    );
    assert_eq!(
        report.threads[0].frames[0].report_symbol.as_deref(),
        Some("main")
    );
    assert!(report::parse_text(Path::new("x"), "not a report", Source::Text).is_err());
}

#[test]
fn parses_atos_and_dwarfdump_output() {
    let full = symbols::parse_atos_line(
        "CountPlanks(FHull*) (in libUnrealEditor-Harbor.dylib) (Harbor.cpp:15)",
    )
    .unwrap();
    assert_eq!(full.function, "CountPlanks(FHull*)");
    assert_eq!(full.file.as_deref(), Some("Harbor.cpp"));
    assert_eq!(full.line, Some(15));
    let no_line =
        symbols::parse_atos_line("_pthread_start (in libsystem_pthread.dylib) + 136").unwrap();
    assert_eq!(no_line.function, "_pthread_start");
    assert_eq!(no_line.line, None);
    assert!(symbols::parse_atos_line("0x70d4b01a00 (in libUnrealEditor-Harbor.dylib)").is_none());
    let templated = symbols::parse_atos_line(
        "TFoo<void (), $_1>::Run() const (in libX.dylib) (Delegate.h:804)",
    )
    .unwrap();
    assert_eq!(templated.function, "TFoo<void (), $_1>::Run() const");
    assert_eq!(templated.line, Some(804));

    let uuids = symbols::parse_dwarfdump_uuids(
        "UUID: c2c742d0-ffa8-3433-a7fa-3335d1f62885 (arm64) /x/libFoo.dylib\nUUID: A17136CE-9E4F-3F75-B5F3-5E69724BABC3 (x86_64) /x/libFoo.dylib\n",
    );
    assert_eq!(
        uuids[0],
        (
            "C2C742D0-FFA8-3433-A7FA-3335D1F62885".to_string(),
            "arm64".to_string()
        )
    );
    assert_eq!(uuids.len(), 2);
    assert_eq!(
        report::normalize_uuid("2423ab55dd623a34b8c81d496c0c7be4").as_deref(),
        Some("2423AB55-DD62-3A34-B8C8-1D496C0C7BE4")
    );
    assert_eq!(
        symbols::redacted_prefix(Path::new("/private/tmp/*/sinker")),
        Some(PathBuf::from("/private/tmp"))
    );
    assert_eq!(symbols::redacted_prefix(Path::new("/*/sinker")), None);
}

/// A fake machine where the Harbor dylib exists (with `uuid`) and atos knows two addresses.
fn machine(dir: &Path, harbor_uuid: &str) -> (FakeProbe, PathBuf) {
    let mut probe = FakeProbe::healthy_mac(dir);
    let dylib = dir.join("Harbor/Binaries/Mac/libUnrealEditor-Harbor.dylib");
    std::fs::create_dir_all(dylib.parent().unwrap()).unwrap();
    std::fs::write(
        dylib
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("Harbor.uproject"),
        "{}",
    )
    .unwrap();
    std::fs::write(&dylib, "fixture").unwrap();
    probe.ok(
        &format!("dwarfdump --uuid {}", dylib.display()),
        &format!("UUID: {harbor_uuid} (arm64) {}\n", dylib.display()),
    );
    // Frames after a thread's first are return addresses, looked up one byte back.
    probe.ok(
        &format!("atos -o {} -arch arm64 -l 0x70d4b00000 0x70d4b019ff 0x70d4b02aff", dylib.display()),
        "CountPlanks(FHull*) (in libUnrealEditor-Harbor.dylib) (Harbor.cpp:15)\nRunCommand() (in libUnrealEditor-Harbor.dylib) (Harbor.cpp:40)\n",
    );
    probe.ok("sw_vers -buildVersion", "99A123\n");
    (probe, dylib)
}

fn with_harbor_at(dir: &Path, dylib: &Path) -> Report {
    let mut report = report::read(&fixture("unreal-folder")).unwrap();
    report.images[4].path = Some(dylib.to_path_buf());
    let _ = dir;
    report
}

#[test]
fn symbolicates_with_a_matching_binary_and_says_why_others_have_none() {
    let tmp = tempfile::tempdir().unwrap();
    let (probe, dylib) = machine(tmp.path(), "CCCCCCCC-CCCC-CCCC-CCCC-CCCCCCCCCC01");
    let report = with_harbor_at(tmp.path(), &dylib);
    let analysis = analyze(&probe, &report, &[], false, 3);

    let frames = &analysis.threads[0].frames;
    assert_eq!(frames[1].function.as_deref(), Some("CountPlanks(FHull*)"));
    assert_eq!(frames[1].file.as_deref(), Some("Harbor.cpp"));
    assert_eq!(frames[1].line, Some(15));
    assert_eq!(frames[2].line, Some(40));
    let harbor = analysis
        .images
        .iter()
        .find(|i| i.name.contains("Harbor"))
        .unwrap();
    assert_eq!(
        harbor.symbols,
        Symbols::Binary {
            path: dylib.clone()
        }
    );
    // The engine's Core isn't on this fake machine at all.
    let core = analysis
        .images
        .iter()
        .find(|i| i.name.contains("Core"))
        .unwrap();
    assert_eq!(core.symbols, Symbols::Missing);
    // Bucketed by our own frames, not the engine's assert plumbing.
    assert_eq!(
        analysis.signature,
        vec![
            "UnrealEditor-Harbor!CountPlanks(FHull*)",
            "UnrealEditor-Harbor!RunCommand()"
        ]
    );
}

#[test]
fn a_binary_from_another_build_is_a_mismatch() {
    let tmp = tempfile::tempdir().unwrap();
    let (probe, dylib) = machine(tmp.path(), "C2C742D0-FFA8-3433-A7FA-3335D1F62885");
    let report = with_harbor_at(tmp.path(), &dylib);
    let analysis = analyze(&probe, &report, &[], false, 3);
    let harbor = analysis
        .images
        .iter()
        .find(|i| i.name.contains("Harbor"))
        .unwrap();
    match &harbor.symbols {
        Symbols::Mismatch { found } => {
            assert_eq!(found[0].1, "C2C742D0-FFA8-3433-A7FA-3335D1F62885")
        }
        other => panic!("{other:?}"),
    }
    assert!(analysis.threads[0].frames[1].function.is_none());
    // Unsymbolicated, the signature falls back to offsets in the image.
    assert_eq!(analysis.signature[0], "UnrealEditor-Harbor!+0x1a00");
}

#[test]
fn spotlight_finds_a_dsym_by_uuid() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut probe, dylib) = machine(tmp.path(), "C2C742D0-FFA8-3433-A7FA-3335D1F62885");
    let dsym = tmp.path().join("Archive/libUnrealEditor-Harbor.dylib.dSYM");
    std::fs::create_dir_all(&dsym).unwrap();
    probe.ok(
        "mdfind com_apple_xcode_dsym_uuids == CCCCCCCC-CCCC-CCCC-CCCC-CCCCCCCCCC01",
        &format!("{}\n", dsym.display()),
    );
    probe.ok(
        &format!("dwarfdump --uuid {}", dsym.display()),
        &format!(
            "UUID: CCCCCCCC-CCCC-CCCC-CCCC-CCCCCCCCCC01 (arm64) {}\n",
            dsym.display()
        ),
    );
    let report = with_harbor_at(tmp.path(), &dylib);
    let search = symbols::SearchPaths::default();
    assert_eq!(
        symbols::locate(&probe, &report.images[4], &search, true),
        Symbols::Dsym { path: dsym }
    );
    // A macOS library with no readable UUID uses the shared cache only on the same macOS build.
    assert_eq!(
        symbols::locate(&probe, &report.images[3], &search, true),
        Symbols::SharedCache {
            path: report.images[3].path.clone().unwrap()
        }
    );
    assert_eq!(
        symbols::locate(&probe, &report.images[3], &search, false),
        Symbols::System
    );
}

fn frame(module: &str, function: &str, own: bool, system: bool) -> SignatureFrame {
    SignatureFrame {
        module: module.into(),
        function: function.into(),
        own,
        system,
    }
}

#[test]
fn signatures_skip_plumbing_and_prefer_your_own_frames() {
    let frames = vec![
        frame("libsystem_kernel.dylib", "__pthread_kill", false, true),
        frame(
            "UnrealEditor-Core",
            "FGenericPlatformMisc::RaiseException(unsigned int)",
            false,
            false,
        ),
        frame(
            "UnrealEditor-Core",
            "FDebug::CheckVerifyFailedImpl2(char const*)",
            false,
            false,
        ),
        frame("UnrealEditor-Harbor", "FailCheck()", true, false),
        frame("UnrealEditor-Harbor", "RunCommand()", true, false),
        frame(
            "UnrealEditor-Core",
            "FConsoleCommand::Execute()",
            false,
            false,
        ),
        frame("UnrealEditor-Harbor", "Tick()", true, false),
    ];
    assert_eq!(
        bucket::signature(&frames, 2),
        vec![
            "UnrealEditor-Harbor!FailCheck()",
            "UnrealEditor-Harbor!RunCommand()"
        ]
    );
    // None of it ours: the top frames that aren't plumbing.
    let engine_only: Vec<SignatureFrame> = frames.iter().filter(|f| !f.own).cloned().collect();
    assert_eq!(
        bucket::signature(&engine_only, 3),
        vec!["UnrealEditor-Core!FConsoleCommand::Execute()"]
    );
    // Stable: the same frames give the same id, different frames a different one.
    let a = bucket::signature(&frames, 3);
    assert_eq!(bucket::id(&a), bucket::id(&bucket::signature(&frames, 3)));
    assert_ne!(bucket::id(&a), bucket::id(&bucket::signature(&frames, 2)));
    assert_eq!(bucket::id(&a).len(), 12);
    assert_eq!(
        bucket::module_label("libUnrealEditor-Harbor.dylib"),
        "UnrealEditor-Harbor"
    );
    assert_eq!(
        bucket::module_label("libsystem_c.dylib"),
        "libsystem_c.dylib"
    );
}

#[test]
fn groups_repeat_crashes_with_counts_and_dates() {
    let tmp = tempfile::tempdir().unwrap();
    let (probe, dylib) = machine(tmp.path(), "CCCCCCCC-CCCC-CCCC-CCCC-CCCCCCCCCC01");
    let mut first = with_harbor_at(tmp.path(), &dylib);
    first.time = Some("2026-01-01 10:00:00 UTC".into());
    let mut second = first.clone();
    second.time = Some("2026-01-03 09:00:00 UTC".into());
    let other = report::read(&fixture("sinker.ips")).unwrap();
    let analyses: Vec<Analysis> = [&first, &second, &other]
        .iter()
        .map(|r| analyze(&probe, r, &[], false, 3))
        .collect();
    let groups = group(&analyses);
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].count, 2);
    assert_eq!(groups[0].first.as_deref(), Some("2026-01-01 10:00:00 UTC"));
    assert_eq!(groups[0].last.as_deref(), Some("2026-01-03 09:00:00 UTC"));
    assert_eq!(groups[0].kinds, vec!["Assert"]);
    assert_eq!(groups[1].count, 1);
}

#[test]
fn unreal_times_become_readable_utc() {
    assert_eq!(
        readable_time("639296069869620000"),
        "2026-11-07 00:09:46 UTC"
    );
    assert_eq!(
        readable_time("2026-11-06 19:12:38.3995 -0500"),
        "2026-11-06 19:12:38 -0500"
    );
    assert_eq!(readable_time("anything else"), "anything else");
}

#[test]
fn finds_a_projects_crashes_in_every_place_they_land() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let project = tmp.path().join("Harbor");
    let write = |path: PathBuf, text: &str| {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    };
    write(
        project.join("Saved/Crashes/One/CrashContext.runtime-xml"),
        "<GameName>UE-Harbor</GameName>",
    );
    let engine = home.join("Library/Application Support/Epic/UnrealEngine/5.8/Saved/Crashes");
    write(
        engine.join("CrashReport-UE-Harbor-pid-1-AAAA/minidump.dmp"),
        "Incident Identifier: x",
    );
    write(
        engine.join("CrashReport-UE-Other-pid-2-BBBB/CrashContext.runtime-xml"),
        "<GameName>UE-Other</GameName>",
    );
    write(
        engine.join("Renamed/CrashContext.runtime-xml"),
        "<GameName>UE-Harbor</GameName>",
    );
    let reports = home.join("Library/Logs/DiagnosticReports");
    write(reports.join("Harbor-2026-01-01-000000.ips"), "{}");
    write(reports.join("Elsewhere-2026-01-01-000000.ips"), "{}");
    let found = find_for_project(&project, "Harbor", Some(&home));
    let names: Vec<String> = found
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(found.len(), 4, "{names:?}");
    assert!(
        !names
            .iter()
            .any(|n| n.contains("Other") || n.contains("Elsewhere"))
    );
}
