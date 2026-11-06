//! Turning soundings into words: a terminal view, or JSON.

use std::fmt::Write as _;

use serde::Serialize;

use super::{Diff, Flag, Sounding, Target, View};
use crate::ui::{self, ACCENT, BAD, BOLD, DIM, GOOD, WARN, paint};
use crate::units::bytes;

/// Version of the `--json` layout. Bumped on breaking changes.
pub const JSON_SCHEMA: u32 = 1;

const BAR_WIDTH: usize = 20;

// --- JSON --------------------------------------------------------------------

#[derive(Serialize)]
struct RunJson<'a> {
    #[serde(flatten)]
    sounding: &'a Sounding,
    draft: Option<u64>,
    targets: Vec<Target>,
    oversized: Vec<Flag>,
}

impl<'a> RunJson<'a> {
    fn new(sounding: &'a Sounding, machines: &[u64], headroom: u64) -> RunJson<'a> {
        RunJson {
            sounding,
            draft: sounding.draft(),
            targets: sounding.targets(machines, headroom),
            oversized: sounding.oversized(),
        }
    }
}

pub fn render_json(sounding: &Sounding, machines: &[u64], headroom: u64) -> String {
    let value = serde_json::json!({
        "schema": JSON_SCHEMA,
        "salvage": env!("CARGO_PKG_VERSION"),
        "command": "soundings",
        "headroom_percent": headroom,
        "run": RunJson::new(sounding, machines, headroom),
    });
    pretty(&value)
}

pub fn render_diff_json(diff: &Diff, machines: &[u64], headroom: u64) -> String {
    let value = serde_json::json!({
        "schema": JSON_SCHEMA,
        "salvage": env!("CARGO_PKG_VERSION"),
        "command": "soundings",
        "headroom_percent": headroom,
        "before": RunJson::new(&diff.before, machines, headroom),
        "after": RunJson::new(&diff.after, machines, headroom),
        "draft_delta": diff.draft_delta(),
        "changes": diff.changes,
    });
    pretty(&value)
}

fn pretty(value: &serde_json::Value) -> String {
    serde_json::to_string_pretty(value).expect("soundings serialize") + "\n"
}

// --- Terminal ----------------------------------------------------------------

/// One run: draft, targets, where the memory goes, and what to trim.
pub fn render_report(
    sounding: &Sounding,
    machines: &[u64],
    headroom: u64,
    width: usize,
    top: usize,
) -> String {
    let mut out = String::new();
    header(&mut out, &file_name(sounding), sounding);

    let _ = writeln!(out);
    match draft_line(sounding) {
        Some(line) => {
            let _ = writeln!(out, "  {}  {line}", paint(BOLD, "Draft"));
            if sounding.kind == super::Kind::Llm {
                let note = "Memory LLM tracked. The LLMPlatform CSV from the same run also counts what LLM can't see.";
                for line in ui::wrap(note, width.saturating_sub(9)) {
                    let _ = writeln!(out, "         {}", paint(DIM, line));
                }
            }
        }
        None => {
            let _ = writeln!(
                out,
                "  {}",
                paint(
                    WARN,
                    "No process memory total in this capture, so there's no draft to measure."
                )
            );
        }
    }

    let targets = sounding.targets(machines, headroom);
    if !targets.is_empty() {
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "  {}  {}",
            paint(BOLD, "Against smaller machines"),
            paint(
                DIM,
                format!("{headroom}% kept free for the OS and other apps")
            )
        );
        for target in &targets {
            let _ = writeln!(out, "    {}", target_cell(target));
        }
    }

    let draft = sounding.draft();
    let flags = sounding.oversized();
    for view in sounding.views() {
        let pools = sounding.pools_in(view);
        let name_width = pools
            .iter()
            .take(top)
            .map(|p| p.name.chars().count())
            .max()
            .unwrap_or(0)
            .min(36);
        let _ = writeln!(out);
        let _ = writeln!(out, "  {}", paint(BOLD, view.title()));
        if let Some(note) = view.note() {
            for line in ui::wrap(note, width.saturating_sub(4)) {
                let _ = writeln!(out, "    {}", paint(DIM, line));
            }
        }
        for pool in pools.iter().take(top) {
            let flagged = flags
                .iter()
                .any(|f| f.view == pool.view && f.name == pool.name);
            let mark = if flagged {
                paint(WARN, "!")
            } else {
                " ".to_string()
            };
            let name = truncate(&pool.name, name_width);
            let mut line = format!("  {mark} {name:<name_width$}  {:>8}", bytes(pool.bytes));
            if let Some(draft) = draft.filter(|d| *d > 0) {
                let share = pool.bytes as f64 / draft as f64;
                let _ = write!(
                    line,
                    "  {}  {}",
                    bar(share),
                    paint(DIM, format!("{:>3.0}%", share * 100.0))
                );
            }
            if let Some(peak) = pool.peak.filter(|p| *p > pool.bytes) {
                let _ = write!(line, "  {}", paint(DIM, format!("peak {}", bytes(peak))));
            }
            if let Some(count) = pool.count {
                let _ = write!(line, "  {}", paint(DIM, format!("{count} objects")));
            }
            let _ = writeln!(out, "  {line}");
        }
        if pools.len() > top {
            let _ = writeln!(
                out,
                "      {}",
                paint(
                    DIM,
                    format!("and {} more (--top to see them)", pools.len() - top)
                )
            );
        }
    }

    if !flags.is_empty() {
        let _ = writeln!(out);
        let _ = writeln!(out, "  {}", paint(BOLD, "Worth a look"));
        for flag in &flags {
            let share = flag
                .share
                .map(|s| format!(", {:.0}% of the draft", s * 100.0))
                .unwrap_or_default();
            let _ = writeln!(
                out,
                "    {}  {} is {}{share}",
                paint(WARN, "!"),
                flag.name,
                bytes(flag.bytes)
            );
            if let Some(knob) = &flag.knob {
                let text = format!("Try {}. {}", knob.setting, knob.hint);
                for (i, line) in ui::wrap(&text, width.saturating_sub(9)).iter().enumerate() {
                    let lead = if i == 0 {
                        paint(ACCENT, "→")
                    } else {
                        " ".to_string()
                    };
                    let line = if i == 0 {
                        line.replacen(knob.setting, &paint(ACCENT, knob.setting), 1)
                    } else {
                        line.clone()
                    };
                    let _ = writeln!(out, "       {lead} {line}");
                }
            }
        }
    }

    let _ = writeln!(out);
    let _ = writeln!(out, "  {}", verdict(&targets, &flags));
    let _ = writeln!(out);
    out
}

/// Two runs side by side: how the draft and each pool moved.
pub fn render_diff(
    diff: &Diff,
    machines: &[u64],
    headroom: u64,
    width: usize,
    top: usize,
) -> String {
    let mut out = String::new();
    let title = format!("{} → {}", file_name(&diff.before), file_name(&diff.after));
    header(&mut out, &title, &diff.after);

    let _ = writeln!(out);
    match (diff.before.draft(), diff.after.draft(), diff.draft_delta()) {
        (Some(before), Some(after), Some(delta)) => {
            let what = if diff.after.peak.is_some() {
                " at peak"
            } else {
                ""
            };
            let _ = writeln!(
                out,
                "  {}  {} → {}{what}   {}",
                paint(BOLD, "Draft"),
                bytes(before),
                bytes(after),
                delta_text(delta)
            );
        }
        _ => {
            let _ = writeln!(
                out,
                "  {}",
                paint(
                    WARN,
                    "One of these captures has no process memory total, so the drafts can't be compared."
                )
            );
        }
    }

    let before_targets = diff.before.targets(machines, headroom);
    let after_targets = diff.after.targets(machines, headroom);
    if !before_targets.is_empty() && before_targets.len() == after_targets.len() {
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "  {}  {}",
            paint(BOLD, "Against smaller machines"),
            paint(
                DIM,
                format!("{headroom}% kept free for the OS and other apps")
            )
        );
        for (before, after) in before_targets.iter().zip(&after_targets) {
            let before_text = format!("{:<16}", target_state_text(before));
            let _ = writeln!(
                out,
                "    {}  {} {before_text}  →  {} {}",
                paint(BOLD, format!("{:>3} GB", before.machine_gb)),
                fit_mark(before),
                fit_mark(after),
                target_state_text(after)
            );
        }
    }

    let _ = writeln!(out);
    let _ = writeln!(out, "  {}", paint(BOLD, "Biggest changes"));
    if diff.changes.is_empty() {
        let _ = writeln!(out, "    {}", paint(DIM, "Nothing moved."));
    }
    let shown: Vec<_> = diff.changes.iter().take(top).collect();
    let name_width = shown
        .iter()
        .map(|c| c.name.chars().count())
        .max()
        .unwrap_or(0)
        .min(36);
    for change in &shown {
        let view = match change.view {
            View::Totals => "total",
            View::Gpu => "GPU",
            View::Llm => "LLM",
            View::Objects => "class",
        };
        let from = change
            .before
            .map(bytes)
            .unwrap_or_else(|| "new".to_string());
        let to = change
            .after
            .map(bytes)
            .unwrap_or_else(|| "gone".to_string());
        let name = truncate(&change.name, name_width);
        let _ = writeln!(
            out,
            "    {}  {name:<name_width$}  {:>8} → {:<8}  {}",
            paint(DIM, format!("{view:<5}")),
            from,
            to,
            delta_text(change.delta)
        );
    }
    if diff.changes.len() > top {
        let more = format!("and {} more (--top to see them)", diff.changes.len() - top);
        let _ = writeln!(out, "    {}", paint(DIM, more));
    }

    let _ = writeln!(out);
    let summary = match diff.draft_delta() {
        Some(d) if d < 0 => format!("{} lighter than before.", bytes(d.unsigned_abs())),
        Some(d) if d > 0 => format!("{} heavier than before.", bytes(d.unsigned_abs())),
        Some(_) => "Same draft as before.".to_string(),
        None => String::new(),
    };
    let fits = after_targets
        .iter()
        .filter(|t| t.fits)
        .map(|t| format!("{} GB", t.machine_gb))
        .collect::<Vec<_>>();
    let rides = if fits.is_empty() || after_targets.is_empty() {
        String::new()
    } else {
        format!(" Now fits {}.", fits.join(", "))
    };
    for line in ui::wrap(&format!("{summary}{rides}"), width.saturating_sub(2)) {
        let _ = writeln!(out, "  {line}");
    }
    let _ = writeln!(out);
    out
}

fn header(out: &mut String, title: &str, sounding: &Sounding) {
    let _ = writeln!(out);
    let _ = writeln!(out, "  {}  {}", paint(BOLD, "salvage soundings"), title);
    let mut labels = vec![sounding.kind.describe().to_string()];
    labels.extend(sounding.labels.iter().cloned());
    if let Some(memory) = sounding.machine_memory {
        labels.push(format!("captured on a {} machine", bytes(memory)));
    }
    let _ = writeln!(out, "  {}", paint(DIM, labels.join(" · ")));
}

fn file_name(sounding: &Sounding) -> String {
    sounding
        .file
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| sounding.file.display().to_string())
}

fn draft_line(sounding: &Sounding) -> Option<String> {
    let draft = sounding.draft()?;
    Some(match (sounding.peak, sounding.footprint) {
        (Some(peak), Some(now)) if peak != now => {
            let when = if sounding.kind == super::Kind::Memreport {
                "at capture"
            } else {
                "at the end"
            };
            format!(
                "{} at peak {}",
                bytes(draft),
                paint(DIM, format!("({} {when})", bytes(now)))
            )
        }
        (Some(_), _) => format!("{} at peak", bytes(draft)),
        _ => bytes(draft),
    })
}

fn target_cell(target: &Target) -> String {
    let mark = fit_mark(target);
    let machine = format!("{:>3} GB", target.machine_gb);
    let budget = format!(
        "  {}",
        paint(DIM, format!("budget {:<7}", bytes(target.budget)))
    );
    format!("{mark}  {machine}{budget}  {}", target_state_text(target))
}

fn fit_mark(target: &Target) -> String {
    if target.fits {
        paint(GOOD, "✓")
    } else {
        paint(BAD, "✗")
    }
}

fn target_state_text(target: &Target) -> String {
    if target.fits {
        format!("{} spare", bytes(target.spare))
    } else {
        format!("trim {}", bytes(target.trim))
    }
}

fn verdict(targets: &[Target], flags: &[Flag]) -> String {
    if targets.is_empty() {
        return "Capture with memreport or -llmcsv to get a draft; see docs/benchmark-recipe.md."
            .to_string();
    }
    let Some(nearest) = targets
        .iter()
        .filter(|t| !t.fits)
        .max_by_key(|t| t.machine_gb)
    else {
        let smallest = targets
            .iter()
            .min_by_key(|t| t.machine_gb)
            .expect("non-empty");
        return paint(
            GOOD,
            format!(
                "Rides under every target, with {} spare on {} GB.",
                bytes(smallest.spare),
                smallest.machine_gb
            ),
        );
    };
    let levers: Vec<String> = flags
        .iter()
        .take(3)
        .map(|f| format!("{} ({})", f.name, bytes(f.bytes)))
        .collect();
    let start = if levers.is_empty() {
        String::new()
    } else {
        format!(" Biggest levers: {}.", levers.join(", "))
    };
    format!(
        "To ride under {} GB, trim {}.{start}",
        nearest.machine_gb,
        paint(BOLD, bytes(nearest.trim))
    )
}

fn delta_text(delta: i64) -> String {
    let size = bytes(delta.unsigned_abs());
    match delta.signum() {
        -1 => paint(GOOD, format!("−{size}")),
        1 => paint(BAD, format!("+{size}")),
        _ => paint(DIM, "±0"),
    }
}

fn bar(share: f64) -> String {
    const EIGHTHS: [&str; 8] = ["", "▏", "▎", "▍", "▌", "▋", "▊", "▉"];
    let cells = (share.clamp(0.0, 1.0) * BAR_WIDTH as f64 * 8.0).round() as usize;
    let full = cells / 8;
    let mut text = "█".repeat(full);
    text.push_str(EIGHTHS[cells % 8]);
    let used = full + usize::from(!cells.is_multiple_of(8));
    text.push_str(&" ".repeat(BAR_WIDTH.saturating_sub(used)));
    paint(ACCENT, text)
}

fn truncate(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_string();
    }
    let mut cut: String = text.chars().take(width.saturating_sub(1)).collect();
    cut.push('…');
    cut
}
