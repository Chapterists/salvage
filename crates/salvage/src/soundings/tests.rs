use std::path::Path;

use super::*;
use crate::units::{GIB, MIB};

const BEFORE: &str = include_str!("../../tests/fixtures/harbor-before.memreport");
const AFTER: &str = include_str!("../../tests/fixtures/harbor-after.memreport");
const LLM: &str = include_str!("../../tests/fixtures/LLM_harbor.csv");
const LLM_PLATFORM: &str = include_str!("../../tests/fixtures/LLMPlatform_harbor.csv");

fn parse(name: &str, text: &str) -> Sounding {
    Sounding::parse(Path::new(name), text).unwrap()
}

fn mib(value: f64) -> u64 {
    (value * MIB as f64).round() as u64
}

fn pool<'a>(sounding: &'a Sounding, view: View, name: &str) -> &'a Pool {
    sounding
        .pools
        .iter()
        .find(|p| p.view == view && p.name == name)
        .unwrap_or_else(|| panic!("no {view:?} pool {name}"))
}

fn strip(text: &str) -> String {
    anstream::adapter::strip_str(text).to_string()
}

#[test]
fn reads_a_memreport() {
    let run = parse("before.memreport", BEFORE);
    assert_eq!(run.kind, Kind::Memreport);
    assert_eq!(run.labels, ["Mac", "Development"]);
    assert_eq!(run.footprint, Some(mib(9612.40)));
    assert_eq!(run.peak, Some(mib(10350.75)));
    assert_eq!(run.draft(), run.peak);
    assert_eq!(run.machine_memory, Some(16 * GIB));

    assert_eq!(pool(&run, View::Gpu, "Lumen").bytes, mib(742.30));
    assert_eq!(pool(&run, View::Gpu, "DistanceFields").bytes, mib(288.0));
    assert_eq!(run.pools_in(View::Gpu).len(), 6);
    assert_eq!(
        pool(&run, View::Totals, "RHI tracked resources").bytes,
        mib(3640.5)
    );
    assert_eq!(
        pool(&run, View::Totals, "Textures in memory").bytes,
        mib(1874.10)
    );
    assert_eq!(
        pool(&run, View::Totals, "Render target pool").bytes,
        mib(1210.42)
    );
    assert_eq!(
        pool(&run, View::Totals, "Objects and their resources").bytes,
        mib(201.442) + mib(3735.684)
    );

    let texture = pool(&run, View::Objects, "Texture2D");
    assert_eq!(texture.count, Some(2210));
    assert_eq!(
        texture.bytes,
        ((1520.31 + 2_210_400.0) * 1024.0_f64).round() as u64
    );
    assert_eq!(
        run.pools_in(View::Objects)[0].name,
        "Texture2D",
        "sorted biggest first"
    );
}

#[test]
fn reads_object_lists_written_as_csv() {
    let text = "MemReport: Begin command \"obj list -resourcesizesort -csv\"\n\
        ,Class,Count,NumKB,MaxKB,ResExcKB,ResExcDedSysKB,ResExcDedVidKB,ResExcUnkKB\n\
        ,Texture2D,10,1.000000,1.000000,1023.000000,0.000000,0.000000,1023.000000\n\
        MemReport: End command \"obj list -resourcesizesort -csv\"\n";
    let run = parse("csv.memreport", text);
    let texture = pool(&run, View::Objects, "Texture2D");
    assert_eq!(texture.bytes, MIB);
    assert_eq!(texture.count, Some(10));
    assert_eq!(run.draft(), None);
}

#[test]
fn reads_utf16_files() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("wide.memreport");
    let mut bytes = vec![0xFF, 0xFE];
    bytes.extend(BEFORE.encode_utf16().flat_map(u16::to_le_bytes));
    std::fs::write(&path, bytes).unwrap();
    let run = Sounding::read(&path).unwrap();
    assert_eq!(run.peak, Some(mib(10350.75)));
}

#[test]
fn reads_an_llm_csv() {
    let run = parse("LLM_harbor.csv", LLM);
    assert_eq!(run.kind, Kind::Llm);
    assert_eq!(run.labels, ["4 samples"]);
    assert_eq!(run.footprint, Some(mib(6802.10)));
    assert_eq!(run.peak, Some(mib(7010.30)));

    let textures = pool(&run, View::Llm, "Textures");
    assert_eq!(textures.bytes, mib(1890.0));
    assert_eq!(textures.peak, Some(mib(1960.40)));
    // UI only appears in later samples; earlier ones count as zero.
    assert_eq!(pool(&run, View::Llm, "UI").peak, Some(mib(61.30)));
    assert!(run.pools.iter().all(|p| p.name != "TrackedTotal"));
}

#[test]
fn reads_an_llm_platform_csv() {
    let run = parse("LLMPlatform_harbor.csv", LLM_PLATFORM);
    assert_eq!(run.kind, Kind::LlmPlatform);
    assert_eq!(run.footprint, Some(mib(9480.75)));
    assert_eq!(pool(&run, View::Llm, "Untracked").bytes, mib(550.50));
    assert!(
        run.pools
            .iter()
            .all(|p| p.name != "Total" && p.name != "TrackedTotal")
    );
}

#[test]
fn rejects_things_that_are_not_captures() {
    let error = Sounding::parse(Path::new("notes.txt"), "hello\nworld\n").unwrap_err();
    assert!(error.contains("doesn't look like a memreport or an LLM CSV"));
    assert!(Sounding::parse(Path::new("LLM.csv"), "TrackedTotal,Textures,\n").is_err());
}

#[test]
fn measures_against_machine_sizes() {
    let run = parse("before.memreport", BEFORE);
    let targets = run.targets(&DEFAULT_TARGETS_GB, DEFAULT_HEADROOM_PERCENT);
    let draft = mib(10350.75);
    assert_eq!(targets.len(), 3);
    assert_eq!(
        targets[0],
        Target {
            machine_gb: 8,
            budget: 6 * GIB,
            draft,
            fits: false,
            trim: draft - 6 * GIB,
            spare: 0
        }
    );
    assert!(!targets[1].fits);
    assert!(targets[2].fits);
    assert_eq!(targets[2].spare, 12 * GIB - draft);

    let no_headroom = Target::measure(8, 0, 8 * GIB);
    assert!(no_headroom.fits);
    assert_eq!(no_headroom.spare, 0);
}

#[test]
fn flags_oversized_pools_with_a_knob() {
    let run = parse("before.memreport", BEFORE);
    let flags = run.oversized();
    let names: Vec<&str> = flags.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, ["Textures in memory", "Render target pool"]);
    assert_eq!(
        flags[0].knob.as_ref().unwrap().setting,
        "r.Streaming.PoolSize"
    );

    let llm = parse("LLM_harbor.csv", LLM);
    let flags = llm.oversized();
    assert_eq!(flags.len(), 1);
    assert_eq!(flags[0].name, "Textures");
}

#[test]
fn knobs_match_common_systems() {
    let setting = |name: &str| knob_for(name).map(|k| k.setting);
    assert_eq!(
        setting("Nanite"),
        Some("r.Nanite.Streaming.StreamingPoolSize")
    );
    assert_eq!(setting("Shadow"), Some("r.Shadow.Virtual.MaxPhysicalPages"));
    assert_eq!(setting("VirtualTexture"), Some("r.VT.PoolSizeScale"));
    assert_eq!(setting("RenderTargets"), Some("r.RenderTargetPoolMin"));
    assert_eq!(setting("GPUScene"), None);
}

#[test]
fn diffs_two_runs() {
    let diff = diff(
        parse("before.memreport", BEFORE),
        parse("after.memreport", AFTER),
    )
    .unwrap();
    assert_eq!(
        diff.draft_delta(),
        Some(mib(7702.10) as i64 - mib(10350.75) as i64)
    );

    let first = &diff.changes[0];
    assert_eq!(first.name, "RHI tracked resources");
    let gone = diff
        .changes
        .iter()
        .find(|c| c.name == "DistanceFields")
        .unwrap();
    assert_eq!(gone.after, None);
    assert_eq!(gone.delta, -(mib(288.0) as i64));
    assert!(
        diff.changes.iter().all(|c| c.delta != 0),
        "unchanged pools are left out"
    );
    assert!(diff.changes.iter().all(|c| c.name != "Texture2D"));
}

#[test]
fn refuses_to_diff_different_kinds() {
    let error = diff(
        parse("before.memreport", BEFORE),
        parse("LLM_harbor.csv", LLM),
    )
    .unwrap_err();
    assert!(error.contains("can't compare a memreport with a LLM CSV"));
}

#[test]
fn report_reads_well_in_a_terminal() {
    let run = parse("harbor-before.memreport", BEFORE);
    let text = strip(&render_report(&run, &DEFAULT_TARGETS_GB, 25, 100, 10));
    assert!(text.contains("salvage soundings  harbor-before.memreport"));
    assert!(
        text.contains("Draft  10.1 GB at peak (9.4 GB at capture)"),
        "{text}"
    );
    assert!(text.contains("✗    8 GB  budget 6 GB"), "{text}");
    assert!(text.contains("✓   16 GB  budget 12 GB"), "{text}");
    for heading in [
        "Headline numbers",
        "GPU resources by system",
        "Biggest object classes",
        "Worth a look",
    ] {
        assert!(text.contains(heading), "missing {heading}");
    }
    assert!(text.contains("Try r.Streaming.PoolSize."));
    assert!(text.contains("To ride under 12 GB, trim 1.1 GB."), "{text}");
    assert!(
        text.lines().all(|l| l == l.trim_end()),
        "no trailing spaces"
    );
}

#[test]
fn top_limits_rows() {
    let run = parse("LLM_harbor.csv", LLM);
    let text = strip(&render_report(&run, &[16], 25, 100, 3));
    assert!(text.contains("and 10 more (--top to see them)"), "{text}");
    assert!(text.contains("Rides under every target"), "{text}");
}

#[test]
fn diff_reads_well_in_a_terminal() {
    let diff = diff(
        parse("harbor-before.memreport", BEFORE),
        parse("harbor-after.memreport", AFTER),
    )
    .unwrap();
    let text = strip(&render_diff(&diff, &DEFAULT_TARGETS_GB, 25, 100, 10));
    assert!(text.contains("harbor-before.memreport → harbor-after.memreport"));
    assert!(
        text.contains("Draft  10.1 GB → 7.5 GB at peak   −2.6 GB"),
        "{text}"
    );
    assert!(
        text.contains(" 12 GB  ✗ trim 1.1 GB       →  ✓ 1.5 GB spare"),
        "{text}"
    );
    assert!(text.contains("DistanceFields") && text.contains("gone"));
    assert!(
        text.contains("2.6 GB lighter than before. Now fits 12 GB, 16 GB."),
        "{text}"
    );
    assert!(
        text.lines().all(|l| l == l.trim_end()),
        "no trailing spaces"
    );
}

#[test]
fn json_for_one_run_and_for_a_diff() {
    let run = parse("before.memreport", BEFORE);
    let json: serde_json::Value = serde_json::from_str(&render_json(&run, &[8, 16], 25)).unwrap();
    assert_eq!(json["schema"], 1);
    assert_eq!(json["command"], "soundings");
    assert_eq!(json["run"]["kind"], "memreport");
    assert_eq!(json["run"]["draft"], mib(10350.75));
    assert_eq!(json["run"]["targets"][0]["machine_gb"], 8);
    assert_eq!(json["run"]["targets"][0]["fits"], false);
    assert_eq!(
        json["run"]["oversized"][0]["knob"]["setting"],
        "r.Streaming.PoolSize"
    );
    assert_eq!(json["run"]["pools"][0]["view"], "objects");

    let diff = diff(run, parse("after.memreport", AFTER)).unwrap();
    let json: serde_json::Value =
        serde_json::from_str(&render_diff_json(&diff, &[12], 25)).unwrap();
    assert_eq!(json["before"]["targets"][0]["fits"], false);
    assert_eq!(json["after"]["targets"][0]["fits"], true);
    assert!(json["draft_delta"].as_i64().unwrap() < 0);
    assert_eq!(json["changes"][0]["view"], "totals");
}

#[test]
fn the_verdict_wraps_to_the_terminal_width() {
    let run = parse("harbor-before.memreport", BEFORE);
    let text = strip(&render_report(&run, &DEFAULT_TARGETS_GB, 25, 60, 10));
    let lines: Vec<&str> = text.lines().collect();
    let start = lines
        .iter()
        .position(|l| l.contains("To ride under 12 GB"))
        .expect("verdict shown");
    let verdict: Vec<&str> = lines[start..]
        .iter()
        .take_while(|l| !l.is_empty())
        .copied()
        .collect();
    assert!(verdict.len() > 1, "long verdict wraps: {verdict:?}");
    for line in &verdict {
        assert!(line.chars().count() <= 60, "too wide: {line:?}");
        assert!(line.starts_with("  "), "keeps its indent: {line:?}");
    }
    let joined = verdict
        .iter()
        .map(|l| l.trim())
        .collect::<Vec<_>>()
        .join(" ");
    assert_eq!(
        joined,
        "To ride under 12 GB, trim 1.1 GB. Biggest levers: Textures in memory (1.8 GB), Render target pool (1.2 GB)."
    );
}
