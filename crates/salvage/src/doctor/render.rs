//! Turning a [`Report`] into words: a friendly terminal view, or JSON.

use std::fmt::Write as _;

use super::{Check, Group, Report, Status};
use crate::probe::Os;
use crate::ui::{self, ACCENT, BAD, BOLD, COMMAND, DIM, GOOD, WARN, paint};
use crate::units::bytes;

/// Version of the `--json` layout. Bumped on breaking changes.
pub const JSON_SCHEMA: u32 = 1;

#[derive(serde::Serialize)]
struct JsonReport<'a> {
    schema: u32,
    salvage: &'static str,
    command: &'static str,
    ok: bool,
    counts: super::Counts,
    #[serde(flatten)]
    report: &'a Report,
}

pub fn render_json(report: &Report) -> String {
    let json = JsonReport {
        schema: JSON_SCHEMA,
        salvage: env!("CARGO_PKG_VERSION"),
        command: "doctor",
        ok: report.ok(),
        counts: report.counts(),
        report,
    };
    serde_json::to_string_pretty(&json).expect("doctor report serializes") + "\n"
}

/// The terminal view, with ANSI styles (stripped later if color is off).
pub fn render_human(report: &Report, width: usize) -> String {
    let mut out = String::new();
    let os = report.platform.os;

    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "  {}  {}",
        paint(BOLD, "salvage doctor"),
        paint(DIM, machine_line(report))
    );
    if let Some(engine) = &report.engine {
        let _ = writeln!(
            out,
            "  {}",
            paint(
                DIM,
                format!("{} at {}", engine.label(), engine.root.display())
            )
        );
    }
    if let Some(project) = &report.project {
        let _ = writeln!(
            out,
            "  {}",
            paint(
                DIM,
                format!("Project {} at {}", project.name, project.dir.display())
            )
        );
    }
    let who = if os == Os::Macos {
        "this Mac"
    } else {
        "this machine"
    };
    let _ = writeln!(out, "  Giving {who} the once-over.");

    let mac_only_hidden = os != Os::Macos;
    let visible =
        |c: &&Check| !(mac_only_hidden && c.status == Status::Skip && c.summary == "macOS only");
    let title_width = report
        .checks
        .iter()
        .filter(visible)
        .map(|c| c.title.chars().count())
        .max()
        .unwrap_or(0);
    let indent = 4 + 1 + 2 + title_width + 2;
    let text_width = width.saturating_sub(indent).max(30);

    for group in Group::ORDER {
        let checks: Vec<&Check> = report
            .checks
            .iter()
            .filter(|c| c.group == group)
            .filter(visible)
            .collect();
        if checks.is_empty() {
            continue;
        }
        let _ = writeln!(out);
        let _ = writeln!(out, "  {}", paint(BOLD, group.title()));
        for check in checks {
            render_check(&mut out, check, title_width, indent, text_width);
        }
    }

    if mac_only_hidden {
        let _ = writeln!(out);
        let note = format!(
            "On {}, doctor checks the engine install, disk, caches and memory. The Xcode, Metal, Rosetta, Mac shader format and LLDB checks only apply on macOS.",
            os.name()
        );
        for line in ui::wrap(&note, width.saturating_sub(2)) {
            let _ = writeln!(out, "  {}", paint(DIM, line));
        }
    }

    let _ = writeln!(out);
    let _ = writeln!(out, "  {}", summary_line(report));
    let _ = writeln!(out);
    out
}

fn machine_line(report: &Report) -> String {
    let p = &report.platform;
    let mut parts = vec![match &p.os_version {
        Some(v) if p.os == Os::Macos => format!("macOS {v}"),
        Some(v) => v.clone(),
        None => p.os.name().to_string(),
    }];
    parts.push(p.cpu.clone().unwrap_or_else(|| p.arch.clone()));
    if let Some(memory) = p.memory {
        parts.push(format!("{} RAM", bytes(memory)));
    }
    parts.join(" · ")
}

fn render_check(
    out: &mut String,
    check: &Check,
    title_width: usize,
    indent: usize,
    text_width: usize,
) {
    let (mark, style) = match check.status {
        Status::Pass => ("✓", GOOD),
        Status::Warn => ("!", WARN),
        Status::Fail => ("✗", BAD),
        Status::Skip => ("–", DIM),
    };
    let pad = " ".repeat(indent);
    let title = format!("{:<title_width$}", check.title);
    let title = if check.status == Status::Skip {
        paint(DIM, title)
    } else {
        title
    };
    let summary = ui::wrap(&check.summary, text_width);
    let mut lines = summary.iter();
    let first = lines.next().map(String::as_str).unwrap_or("");
    let first = if check.status == Status::Skip {
        paint(DIM, first)
    } else {
        first.to_string()
    };
    let _ = writeln!(out, "    {}  {title}  {first}", paint(style, mark));
    for line in lines {
        let _ = writeln!(out, "{pad}{line}");
    }
    for detail in &check.details {
        for line in ui::wrap(detail, text_width) {
            let _ = writeln!(out, "{pad}{}", paint(DIM, line));
        }
    }
    if let Some(fix) = &check.fix {
        let arrow = paint(ACCENT, "→");
        for (i, line) in ui::wrap(&fix.hint, text_width.saturating_sub(2))
            .iter()
            .enumerate()
        {
            let lead = if i == 0 {
                arrow.clone()
            } else {
                " ".to_string()
            };
            let _ = writeln!(out, "{pad}{lead} {line}");
        }
        if let Some(command) = &fix.command {
            let _ = writeln!(out, "{pad}  {}", paint(COMMAND, command));
        }
    }
}

fn summary_line(report: &Report) -> String {
    let counts = report.counts();
    let look = |n: usize| format!("{n} worth a look");
    if counts.fail > 0 {
        let mut parts = vec![paint(BAD, format!("{} to fix", counts.fail))];
        if counts.warn > 0 {
            parts.push(paint(WARN, look(counts.warn)));
        }
        parts.push(format!("{} shipshape", counts.pass));
        return format!(
            "{}. Start with the ✗ items; each one says how.",
            parts.join(", ")
        );
    }
    if counts.warn > 0 {
        return format!(
            "Nothing broken: {}, {} shipshape.",
            paint(WARN, look(counts.warn)),
            counts.pass
        );
    }
    paint(GOOD, format!("All {} checks pass. Shipshape.", counts.pass))
}
