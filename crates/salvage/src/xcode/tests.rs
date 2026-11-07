//! Version judging against invented engine ranges and Epic's table, and
//! finding Xcodes on a fake Mac. No real Xcode or engine needed.

use std::path::{Path, PathBuf};

use super::epic::{self, ROWS};
use super::*;
use crate::testing::{FakeProbe, fake_engine};

fn v(text: &str) -> Version {
    Version::parse(text).unwrap()
}

/// An engine range like UE 5.8's: accepts 15.2 to 27.9, built with 26.1.1.
fn sdk(min: &str, max: &str, main: Option<&str>) -> AppleSdk {
    AppleSdk {
        main: main.map(v),
        min: v(min),
        max: v(max),
        file: PathBuf::from("Apple_SDK.json"),
    }
}

fn verdict(xcode: &str, sdk: &AppleSdk, engine: &str) -> Verdict {
    judge(Some(&v(xcode)), Some(sdk), epic::row_for(&v(engine))).verdict
}

#[test]
fn judges_xcode_for_ue_5_8_by_both_sources() {
    let range = sdk("15.2.0", "27.9.0", Some("26.1.1"));
    assert_eq!(verdict("26.1.1", &range, "5.8.4"), Verdict::Recommended);
    assert_eq!(verdict("26.0", &range, "5.8.4"), Verdict::Supported);
    assert_eq!(verdict("26.2", &range, "5.8.4"), Verdict::Newer);
    assert_eq!(verdict("27.2", &range, "5.8.4"), Verdict::Newer);
    // Inside the engine's range, but Epic says 26.4 doesn't work: a warning, not a failure.
    assert_eq!(verdict("26.4", &range, "5.8.4"), Verdict::NotCompatible);
    assert_eq!(verdict("26.4.1", &range, "5.8.4"), Verdict::NotCompatible);
    assert_eq!(verdict("26.5", &range, "5.8.4"), Verdict::Newer);
    // The config accepts 16.4, but Epic's minimum for 5.8 is 26.0.
    assert_eq!(verdict("16.4", &range, "5.8.4"), Verdict::BelowEpicMinimum);
    assert_eq!(verdict("15.0", &range, "5.8.4"), Verdict::OutsideRange);
    assert_eq!(verdict("28.0", &range, "5.8.4"), Verdict::OutsideRange);
    assert!(!Verdict::NotCompatible.good());
    assert!(Verdict::Newer.good());
}

#[test]
fn the_engine_range_wins_over_epics_table() {
    // A project that narrowed its range: Epic's recommendation is outside it.
    let narrow = sdk("26.2", "27.9", None);
    assert_eq!(verdict("26.1.1", &narrow, "5.8.0"), Verdict::OutsideRange);
}

#[test]
fn judges_older_engines_by_their_rows() {
    let range = sdk("15.2.0", "16.9.0", Some("15.4"));
    assert_eq!(verdict("15.4", &range, "5.7.4"), Verdict::Recommended);
    assert_eq!(
        verdict("16.2", &range, "5.6.1"),
        Verdict::Recommended,
        "15.4 or newer"
    );
    assert_eq!(verdict("15.2", &range, "5.5.0"), Verdict::Supported);
    let range = sdk("14.1", "15.9", None);
    assert_eq!(
        verdict("14.3.1", &range, "5.4.4"),
        Verdict::Recommended,
        "latest Xcode 14"
    );
    assert_eq!(verdict("15.0", &range, "5.3.2"), Verdict::Newer);
}

#[test]
fn without_epic_guidance_the_engine_range_decides() {
    let range = sdk("15.2", "27.9", None);
    assert_eq!(
        judge(Some(&v("26.0")), Some(&range), None).verdict,
        Verdict::Supported
    );
    assert_eq!(
        judge(Some(&v("14.0")), Some(&range), None).verdict,
        Verdict::OutsideRange
    );
    assert_eq!(judge(None, Some(&range), None).verdict, Verdict::Unknown);
    assert!(epic::row_for(&v("4.27.2")).is_none());
    assert!(epic::row_for(&v("5.9.0")).is_none());
}

#[test]
fn every_row_cites_its_source_and_date() {
    for row in ROWS {
        assert!(
            row.source
                .starts_with("https://dev.epicgames.com/documentation/")
        );
        assert_eq!(row.checked.len(), 10, "{}: YYYY-MM-DD", row.engines);
        assert!(row.says.starts_with(row.engines), "{}", row.says);
        assert!(Version::parse(row.minimum).is_some());
        for listed in row.not_compatible {
            assert!(row.says.contains(listed), "{listed} is quoted");
        }
    }
    assert_eq!(epic::row_for(&v("5.8.4")).unwrap().engines, "5.8");
    assert_eq!(epic::row_for(&v("5.7.2")).unwrap().engines, "5.6 - 5.7");
}

fn installed(app: &str, version: &str, verdict: Verdict) -> Installed {
    Installed {
        app: PathBuf::from(app),
        developer_dir: PathBuf::from(app).join("Contents/Developer"),
        version: Version::parse(version),
        build: None,
        metal_toolchain: true,
        missing_metal_tools: Vec::new(),
        judgement: Judgement {
            verdict,
            reasons: Vec::new(),
        },
    }
}

#[test]
fn picks_the_recommended_xcode_and_never_a_bad_one() {
    let list = vec![
        installed("/A/Xcode-27.2.app", "27.2", Verdict::Newer),
        installed("/A/Xcode-26.4.app", "26.4", Verdict::NotCompatible),
        installed("/A/Xcode-26.1.1.app", "26.1.1", Verdict::Recommended),
        installed("/A/Xcode-26.0.app", "26.0", Verdict::Supported),
    ];
    assert_eq!(
        pick(&list).unwrap().app,
        PathBuf::from("/A/Xcode-26.1.1.app")
    );
    assert_eq!(
        pick(&list[..2]).unwrap().app,
        PathBuf::from("/A/Xcode-27.2.app")
    );
    assert!(pick(&list[1..2]).is_none(), "26.4 alone: nothing to pin");
}

/// A fake Xcode bundle with a developer folder, answering xcodebuild and xcrun
/// when run with DEVELOPER_DIR pointing at it.
fn fake_xcode(probe: &mut FakeProbe, app: &Path, version: &str, build: &str, metal: bool) {
    let developer = app.join("Contents/Developer");
    std::fs::create_dir_all(&developer).unwrap();
    let env = format!("env DEVELOPER_DIR={}", developer.display());
    probe.ok(
        &format!("{env} xcodebuild -version"),
        &format!("Xcode {version}\nBuild version {build}\n"),
    );
    if metal {
        probe.ok(
            &format!("{env} xcrun metal --version"),
            "Apple metal version 1 (fixture)\n",
        );
        for tool in METAL_TOOLS {
            probe.ok(
                &format!("{env} xcrun --find {tool}"),
                &format!("/toolchain/{tool}\n"),
            );
        }
    } else {
        probe.err(
            &format!("{env} xcrun metal --version"),
            "error: missing Metal Toolchain\n",
        );
    }
}

#[test]
fn finds_xcodes_from_every_source_once_each() {
    let tmp = tempfile::tempdir().unwrap();
    let apps = tmp.path().join("Applications");
    let mut probe = FakeProbe::healthy_mac(tmp.path());
    let a = apps.join("Xcode.app");
    let b = apps.join("Xcode-26.1.1.app");
    let c = tmp.path().join("Elsewhere/Xcode-beta.app");
    fake_xcode(&mut probe, &a, "27.2", "27B52", true);
    fake_xcode(&mut probe, &b, "26.1.1", "17B100", false);
    fake_xcode(&mut probe, &c, "27.3", "27C5", true);
    std::fs::create_dir_all(apps.join("NotXcode.app/Contents/Developer")).unwrap();
    std::fs::create_dir_all(apps.join("Xcode-broken.app")).unwrap();
    probe.ok(
        "mdfind kMDItemCFBundleIdentifier == 'com.apple.dt.Xcode'",
        &format!("{}\n{}\n", a.display(), c.display()),
    );
    probe.ok(
        "xcodes installed",
        &format!("26.1.1 (17B100) {}\n", b.display()),
    );
    probe.ok(
        "xcode-select -p",
        &format!("{}\n", a.join("Contents/Developer").display()),
    );

    let found = find(&probe, std::slice::from_ref(&apps));
    let names: Vec<String> = found
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names.len(), 3, "{names:?}");
    assert!(names.contains(&"Xcode-beta.app".to_string()));
    assert!(
        !names.contains(&"Xcode-broken.app".to_string()),
        "no developer folder"
    );
}

#[test]
fn reports_the_pick_the_active_xcode_and_what_to_install() {
    let tmp = tempfile::tempdir().unwrap();
    let apps = tmp.path().join("Applications");
    let mut probe = FakeProbe::healthy_mac(tmp.path());
    let current = apps.join("Xcode.app");
    let bad = apps.join("Xcode-26.4.app");
    fake_xcode(&mut probe, &current, "27.2", "27B52", true);
    fake_xcode(&mut probe, &bad, "26.4", "17E100", false);
    probe.ok(
        "xcode-select -p",
        &format!("{}\n", current.join("Contents/Developer").display()),
    );
    probe.env.insert(
        "DEVELOPER_DIR".into(),
        bad.join("Contents/Developer").display().to_string(),
    );
    let root = fake_engine(&tmp.path().join("UE_5.8"), (5, 8, 4));
    // Our own range in UE 5.8's shape, replacing the fixture's invented one.
    std::fs::write(
        root.join("Engine/Config/Apple/Apple_SDK.json"),
        r#"{ "MainVersion": "26.1.1", "MinVersion": "15.2.0", "MaxVersion": "27.9.0" }"#,
    )
    .unwrap();
    let engine = Engine::open(&root, crate::engine::FoundBy::Flag).unwrap();

    let report = report(&probe, Some(&engine), std::slice::from_ref(&apps));
    assert_eq!(report.installed.len(), 2);
    assert_eq!(report.installed[0].version, Some(v("27.2")), "newest first");
    let by_app = |app: &Path| report.installed.iter().find(|i| i.app == app).unwrap();
    assert_eq!(by_app(&bad).judgement.verdict, Verdict::NotCompatible);
    assert!(!by_app(&bad).metal_toolchain);
    assert_eq!(by_app(&bad).missing_metal_tools.len(), METAL_TOOLS.len());
    // DEVELOPER_DIR wins over xcode-select for builds from this shell.
    assert_eq!(report.active, Some(bad.join("Contents/Developer")));
    assert_eq!(report.pick, Some(current.clone()));
    assert_eq!(
        report.install.as_deref(),
        Some("26.1.1"),
        "Epic's recommendation isn't installed"
    );
    assert!(!report.xcodes_installed);
    assert_eq!(report.epic.as_ref().unwrap().not_compatible, vec!["26.4"]);
}

#[test]
fn the_wrapper_pins_one_command_and_quotes_the_path() {
    let script = wrapper_script(
        Path::new("/Apps/Xcode it's.app/Contents/Developer"),
        Some("Unreal Engine 5.8.4"),
    );
    assert!(script.starts_with("#!/bin/sh\n"));
    assert!(script.contains("export DEVELOPER_DIR='/Apps/Xcode it'\\''s.app/Contents/Developer'"));
    assert!(script.contains("exec \"$@\""));
    assert!(script.contains(WRAPPER_MARK));
    assert!(!script.contains("sudo") && !script.contains("xcode-select -s"));
}
