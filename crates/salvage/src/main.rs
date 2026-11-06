use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};

use salvage::doctor;
use salvage::engine;
use salvage::probe::SystemProbe;
use salvage::project::Project;
use salvage::ui::{self, BAD, paint};

/// Get more out of the hardware you already have: Unreal Engine tools for
/// lighter machines and macOS.
#[derive(Parser)]
#[command(name = "salvage", version, propagate_version = true)]
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

fn usage_error(message: &str) -> ExitCode {
    let mut stderr = anstream::stderr();
    let _ = writeln!(stderr, "{} {message}", paint(BAD, "salvage:"));
    ExitCode::from(2)
}
