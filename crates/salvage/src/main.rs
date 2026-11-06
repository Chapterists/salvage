use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};

use salvage::doctor;
use salvage::engine;
use salvage::lldb;
use salvage::probe::Probe;
use salvage::probe::SystemProbe;
use salvage::project::Project;
use salvage::soundings::{self, Sounding};
use salvage::ui::{self, BAD, DIM, GOOD, paint};

/// `0.1.0 (1a2b3c4d5e6f)`: the crate version and the commit it was built from.
const VERSION: &str = if env!("SALVAGE_COMMIT").is_empty() {
    env!("CARGO_PKG_VERSION")
} else {
    concat!(env!("CARGO_PKG_VERSION"), " (", env!("SALVAGE_COMMIT"), ")")
};

/// Salvage: Chapterists' open-source toolbox, the tools we use to make our
/// stories. First up: Unreal Engine on lighter machines and on macOS.
#[derive(Parser)]
#[command(name = "salvage", version = VERSION, propagate_version = true)]
struct Cli {
    /// When to use color.
    #[arg(long, value_enum, global = true, default_value_t = Color::Auto)]
    color: Color,

    #[command(subcommand)]
    command: Command,
}

#[derive(Clone, Copy, ValueEnum)]
enum Color {
    Auto,
    Always,
    Never,
}

#[derive(Subcommand)]
enum Command {
    /// Check this machine for the things that make Unreal slow or broken.
    ///
    /// Each check prints pass, warn or fail with a concrete fix. Exits
    /// non-zero if anything fails.
    Doctor {
        /// Unreal Engine install to check against (the folder containing Engine/).
        /// Found automatically when omitted.
        #[arg(long, env = "SALVAGE_ENGINE", value_name = "PATH")]
        engine: Option<PathBuf>,

        /// Project to check: a .uproject file or its folder. Defaults to a
        /// .uproject in the current folder, if there is one.
        #[arg(long, value_name = "PATH")]
        project: Option<PathBuf>,

        /// Print machine-readable JSON instead.
        #[arg(long)]
        json: bool,
    },

    /// Read a memreport or LLM CSV and see where the memory goes.
    ///
    /// With one capture, shows the draft (peak process memory), how it sits
    /// against 8, 12 and 16 GB machines, each system's share, and the pools
    /// worth trimming. With two, shows what changed from the first to the
    /// second. See docs/benchmark-recipe.md for how to capture runs.
    Soundings {
        /// A .memreport file or an LLM CSV; give two to compare them.
        #[arg(value_name = "CAPTURE", num_args = 1..=2, required = true)]
        captures: Vec<PathBuf>,

        /// Machine memory to measure against, in GB. Repeat for several.
        #[arg(long = "target", value_name = "GB", value_parser = clap::value_parser!(u64).range(1..=4096))]
        targets: Vec<u64>,

        /// Percent of a machine's memory left for the OS and other apps.
        #[arg(long, value_name = "PERCENT", default_value_t = soundings::DEFAULT_HEADROOM_PERCENT, value_parser = clap::value_parser!(u64).range(0..=90))]
        headroom: u64,

        /// Rows to show per section.
        #[arg(long, value_name = "N", default_value_t = 10)]
        top: usize,

        /// Exit 1 unless the (last) capture fits this machine size, in GB. For CI.
        #[arg(long, value_name = "GB")]
        require: Option<u64>,

        /// Print machine-readable JSON instead.
        #[arg(long)]
        json: bool,
    },

    /// Install or remove Salvage's LLDB formatters for Unreal types.
    #[command(subcommand)]
    Lldb(LldbCommand),
}

#[derive(Subcommand)]
enum LldbCommand {
    /// Make LLDB and Xcode show Unreal types (FString, TSharedPtr, TObjectPtr,
    /// FText, TOptional, FVector...) as values instead of raw memory.
    ///
    /// Adds one marked `command script import` line to ~/.lldbinit (and to
    /// ~/.lldbinit-Xcode if you have one), after backing the file up. Safe to
    /// run again; it never rewrites your own lines.
    Install,
    /// Remove the line `salvage lldb install` added, and the formatter script.
    Uninstall,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.color {
        Color::Auto => {}
        Color::Always => anstream::ColorChoice::Always.write_global(),
        Color::Never => anstream::ColorChoice::Never.write_global(),
    }
    match cli.command {
        Command::Doctor {
            engine,
            project,
            json,
        } => run_doctor(engine, project, json),
        Command::Soundings {
            captures,
            targets,
            headroom,
            top,
            require,
            json,
        } => run_soundings(&captures, targets, headroom, top, require, json),
        Command::Lldb(command) => run_lldb(command),
    }
}

fn run_doctor(engine: Option<PathBuf>, project: Option<PathBuf>, json: bool) -> ExitCode {
    let probe = SystemProbe;
    let project = match project {
        Some(path) => match Project::open(&path) {
            Ok(project) => Some(project),
            Err(error) => return usage_error(&error),
        },
        None => std::env::current_dir()
            .ok()
            .and_then(|dir| Project::find_in(&dir)),
    };
    let engine = match engine::discover(engine.as_deref(), project.as_ref(), &probe) {
        Ok(engine) => engine,
        Err(error) => return usage_error(&error),
    };
    let report = doctor::run(&probe, engine, project);
    let text = if json {
        doctor::render_json(&report)
    } else {
        doctor::render_human(&report, ui::width())
    };
    let mut stdout = anstream::stdout();
    let _ = stdout.write_all(text.as_bytes());
    let _ = stdout.flush();
    if report.ok() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn run_soundings(
    captures: &[PathBuf],
    mut targets: Vec<u64>,
    headroom: u64,
    top: usize,
    require: Option<u64>,
    json: bool,
) -> ExitCode {
    if targets.is_empty() {
        targets = soundings::DEFAULT_TARGETS_GB.to_vec();
    }
    if let Some(gb) = require.filter(|gb| !targets.contains(gb)) {
        targets.push(gb);
    }
    targets.sort_unstable();
    targets.dedup();

    let mut runs = Vec::new();
    for path in captures {
        match Sounding::read(path) {
            Ok(run) => runs.push(run),
            Err(error) => return usage_error(&error),
        }
    }
    let width = ui::width();
    let (text, last) = match <[Sounding; 2]>::try_from(runs) {
        Ok([before, after]) => match soundings::diff(before, after) {
            Ok(diff) => {
                let text = if json {
                    soundings::render_diff_json(&diff, &targets, headroom)
                } else {
                    soundings::render_diff(&diff, &targets, headroom, width, top)
                };
                (text, diff.after)
            }
            Err(error) => return usage_error(&error),
        },
        Err(mut runs) => {
            let run = runs.remove(0);
            let text = if json {
                soundings::render_json(&run, &targets, headroom)
            } else {
                soundings::render_report(&run, &targets, headroom, width, top)
            };
            (text, run)
        }
    };
    let mut stdout = anstream::stdout();
    let _ = stdout.write_all(text.as_bytes());
    let _ = stdout.flush();

    let Some(gb) = require else {
        return ExitCode::SUCCESS;
    };
    let fits = last.targets(&[gb], headroom).first().map(|t| t.fits);
    match fits {
        Some(true) => ExitCode::SUCCESS,
        Some(false) => ExitCode::FAILURE,
        None => usage_error("--require needs a capture with a process memory total"),
    }
}

fn run_lldb(command: LldbCommand) -> ExitCode {
    let probe = SystemProbe;
    let Some(home) = probe.home() else {
        return usage_error("can't find your home folder");
    };
    let layout = lldb::Layout::for_user(home.clone(), probe.os(), |key| probe.env(key));
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let installing = matches!(command, LldbCommand::Install);
    let outcome = match command {
        LldbCommand::Install => lldb::install(&layout, stamp),
        LldbCommand::Uninstall => lldb::uninstall(&layout, stamp),
    };
    let outcome = match outcome {
        Ok(outcome) => outcome,
        Err(error) => {
            let mut stderr = anstream::stderr();
            let _ = writeln!(stderr, "{} {error}", paint(BAD, "salvage:"));
            return ExitCode::FAILURE;
        }
    };

    let tilde = |path: &std::path::Path| match path.strip_prefix(&home) {
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    };
    let done = paint(GOOD, "✓");
    let mut out = String::from("\n");
    if installing {
        let verb = if outcome.script_changed {
            "Wrote"
        } else {
            "Up to date:"
        };
        out += &format!("  {done} {verb} {}\n", tilde(&outcome.script));
    } else if outcome.script_changed {
        out += &format!("  {done} Removed {}\n", tilde(&outcome.script));
    }
    for edit in &outcome.edits {
        let file = tilde(&edit.file);
        let line = match edit.change {
            lldb::Change::Added => format!("Added the import line to {file}"),
            lldb::Change::Updated => {
                format!("Pointed the import line in {file} at the current script")
            }
            lldb::Change::Unchanged => format!("{file} already loads it"),
            lldb::Change::Removed => format!("Removed Salvage's lines from {file}"),
            lldb::Change::Absent => format!("{file} had nothing of Salvage's"),
        };
        out += &format!("  {done} {line}\n");
        if let Some(backup) = &edit.backup {
            out += &format!("    {}\n", paint(DIM, format!("backup: {}", tilde(backup))));
        }
    }
    out += "\n";
    out += if installing {
        "  Restart LLDB or Xcode to pick them up. FString, TSharedPtr, TObjectPtr, FText and\n  friends will show their values.\n\n"
    } else {
        "  Restart LLDB or Xcode for it to take effect.\n\n"
    };
    let mut stdout = anstream::stdout();
    let _ = stdout.write_all(out.as_bytes());
    ExitCode::SUCCESS
}

fn usage_error(message: &str) -> ExitCode {
    let mut stderr = anstream::stderr();
    let _ = writeln!(stderr, "{} {message}", paint(BAD, "salvage:"));
    ExitCode::from(2)
}
