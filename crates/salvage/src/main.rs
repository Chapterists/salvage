use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};

use salvage::clangd;
use salvage::doctor;
use salvage::engine;
use salvage::lldb;
use salvage::probe::Probe;
use salvage::probe::SystemProbe;
use salvage::project::Project;
use salvage::soundings::{self, Sounding};
use salvage::trim;
use salvage::ui::{self, BAD, BOLD, DIM, GOOD, WARN, paint};

/// `0.1.0 (1a2b3c4d5e6f)`: the crate version and the commit it was built from.
const VERSION: &str = if env!("SALVAGE_COMMIT").is_empty() {
    env!("CARGO_PKG_VERSION")
} else {
    concat!(env!("CARGO_PKG_VERSION"), " (", env!("SALVAGE_COMMIT"), ")")
};

/// The salvage command, from Salvage: Chapterists' open-source toolbox, the
/// tools we use to make our stories. Checks, measurements and setup for
/// Unreal Engine work.
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

    /// Write memory tiers for 8, 12 and 16 GB machines into a project's config.
    ///
    /// Adds marked blocks to the project's Config files (backing each file up
    /// first) with Salvage's settings for each machine size, every value with
    /// its reason. --undo takes them out again.
    Trim {
        /// Project to change: a .uproject file or its folder. Defaults to the
        /// .uproject in the current folder.
        #[arg(long, value_name = "PATH")]
        project: Option<PathBuf>,

        /// Also write the low-RAM editor preset (shader compile workers,
        /// virtual texture pools, Content Browser thumbnails).
        #[arg(long)]
        editor: bool,

        /// Remove everything salvage trim added.
        #[arg(long, conflicts_with = "editor")]
        undo: bool,

        /// Show which files would change and what would be written, and
        /// change nothing.
        #[arg(long)]
        dry_run: bool,
    },

    /// Set up clangd for a project's C++: a fast IDE without Xcode's indexer.
    ///
    /// Runs UnrealBuildTool to write compile_commands.json into the project,
    /// then writes a .clangd tuned for Unreal code. Existing files are backed
    /// up before they're replaced.
    Clangd {
        /// Project to set up: a .uproject file or its folder. Defaults to the
        /// .uproject in the current folder.
        #[arg(long, value_name = "PATH")]
        project: Option<PathBuf>,

        /// Unreal Engine install to use. Found from the project when omitted.
        #[arg(long, env = "SALVAGE_ENGINE", value_name = "PATH")]
        engine: Option<PathBuf>,

        /// Build target to index. Defaults to the editor target.
        #[arg(long, value_name = "NAME")]
        target: Option<String>,

        /// Build configuration.
        #[arg(long, value_name = "NAME", default_value = "Development")]
        configuration: String,

        /// Also write .vscode/settings.json for the clangd extension.
        #[arg(long)]
        vscode: bool,

        /// Also write .nvim.lua, read by Neovim when 'exrc' is on.
        #[arg(long)]
        neovim: bool,

        /// Only run UnrealBuildTool if build rules or the set of source files changed.
        #[arg(long)]
        refresh: bool,

        /// Show what would run, and change nothing.
        #[arg(long)]
        dry_run: bool,
    },
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
        Command::Trim {
            project,
            editor,
            undo,
            dry_run,
        } => run_trim(project, editor, undo, dry_run),
        Command::Clangd {
            project,
            engine,
            target,
            configuration,
            vscode,
            neovim,
            refresh,
            dry_run,
        } => run_clangd(ClangdArgs {
            project,
            engine,
            target,
            configuration,
            vscode,
            neovim,
            refresh,
            dry_run,
        }),
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

fn run_trim(project: Option<PathBuf>, editor: bool, undo: bool, dry_run: bool) -> ExitCode {
    let project = match project {
        Some(path) => match Project::open(&path) {
            Ok(project) => project,
            Err(error) => return usage_error(&error),
        },
        None => match std::env::current_dir()
            .ok()
            .and_then(|dir| Project::find_in(&dir))
        {
            Some(project) => project,
            None => {
                return usage_error(
                    "no .uproject here; run this in a project folder or pass --project",
                );
            }
        },
    };
    let mut out = anstream::stdout();
    if dry_run {
        return trim_dry_run(&mut out, &project, editor, undo);
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let result = if undo {
        trim::undo(&project, stamp)
    } else {
        trim::apply(&project, editor, stamp)
    };
    let results = match result {
        Ok(results) => results,
        Err(error) => {
            let mut stderr = anstream::stderr();
            let _ = writeln!(stderr, "{} {error}", paint(BAD, "salvage:"));
            return ExitCode::FAILURE;
        }
    };
    let relative = |path: &std::path::Path| {
        path.strip_prefix(&project.dir)
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| path.display().to_string())
    };
    let done = paint(GOOD, "✓");
    let _ = writeln!(out, "\n  {}  {}", paint(BOLD, "salvage trim"), project.name);
    if results.is_empty() {
        let _ = writeln!(
            out,
            "  Nothing to undo: no salvage trim blocks in {}.\n",
            project.dir.join("Config").display()
        );
        return ExitCode::SUCCESS;
    }
    for (path, written) in &results {
        let line = match (undo, written) {
            (true, salvage::fsutil::Written::Replaced { backup }) if !path.exists() => {
                format!(
                    "Removed {} (it only held Salvage's block; backup: {})",
                    relative(path),
                    relative(backup)
                )
            }
            (true, salvage::fsutil::Written::Replaced { backup }) => {
                format!(
                    "Took Salvage's block out of {} (backup: {})",
                    relative(path),
                    relative(backup)
                )
            }
            (_, salvage::fsutil::Written::Created) => format!("Wrote {}", relative(path)),
            (_, salvage::fsutil::Written::Removed) => {
                format!("Removed {} (it only held Salvage's block)", relative(path))
            }
            (_, salvage::fsutil::Written::Unchanged) => {
                format!("{} is already up to date", relative(path))
            }
            (false, salvage::fsutil::Written::Replaced { backup }) => {
                format!("Updated {} (backup: {})", relative(path), relative(backup))
            }
        };
        let _ = writeln!(out, "  {done} {line}");
    }
    let _ = writeln!(
        out,
        "\n  {}\n",
        if undo {
            "Restart the editor or game for the stock settings to come back."
        } else {
            "Restart the editor or game to pick these up. Each value has its reason next to it;\n  `salvage trim --undo` takes them all out."
        }
    );
    ExitCode::SUCCESS
}

/// `salvage trim --dry-run`: which files would change, and the blocks that
/// would be written into them.
fn trim_dry_run(out: &mut impl Write, project: &Project, editor: bool, undo: bool) -> ExitCode {
    let result = if undo {
        trim::preview_undo(project)
    } else {
        trim::preview(project, editor)
    };
    let changes = match result {
        Ok(changes) => changes,
        Err(error) => {
            let mut stderr = anstream::stderr();
            let _ = writeln!(stderr, "{} {error}", paint(BAD, "salvage:"));
            return ExitCode::FAILURE;
        }
    };
    let relative = |path: &std::path::Path| {
        path.strip_prefix(&project.dir)
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| path.display().to_string())
    };
    let _ = writeln!(
        out,
        "\n  {}  {} {}",
        paint(BOLD, "salvage trim"),
        project.name,
        paint(DIM, "(dry run: nothing is changed)")
    );
    if changes.is_empty() {
        let _ = writeln!(
            out,
            "  Nothing to undo: no salvage trim blocks in Config.\n"
        );
        return ExitCode::SUCCESS;
    }
    for (path, change) in &changes {
        let file = relative(path);
        let line = match (undo, change) {
            (_, trim::Change::Create) => format!("Would write {file}"),
            (_, trim::Change::Unchanged) => format!("{file} is already up to date"),
            (_, trim::Change::Delete) => {
                format!("Would remove {file} (it only holds Salvage's block)")
            }
            (true, trim::Change::Update) => {
                format!("Would take Salvage's block out of {file}, after backing it up")
            }
            (false, trim::Change::Update) => format!("Would update {file}, after backing it up"),
        };
        let mark = if *change == trim::Change::Unchanged {
            paint(DIM, "·")
        } else {
            paint(WARN, "~")
        };
        let _ = writeln!(out, "  {mark} {line}");
    }
    if !undo {
        for (file, block) in trim::plan(editor) {
            let changes_file = changes.iter().any(|(path, change)| {
                relative(path) == file
                    && matches!(change, trim::Change::Create | trim::Change::Update)
            });
            if changes_file {
                let _ = writeln!(
                    out,
                    "\n{}\n{}\n{block}{}",
                    paint(BOLD, &file),
                    trim::BEGIN,
                    trim::END
                );
            }
        }
    }
    let _ = writeln!(out);
    ExitCode::SUCCESS
}

struct ClangdArgs {
    project: Option<PathBuf>,
    engine: Option<PathBuf>,
    target: Option<String>,
    configuration: String,
    vscode: bool,
    neovim: bool,
    refresh: bool,
    dry_run: bool,
}

fn run_clangd(args: ClangdArgs) -> ExitCode {
    let probe = SystemProbe;
    let project = match &args.project {
        Some(path) => match Project::open(path) {
            Ok(project) => project,
            Err(error) => return usage_error(&error),
        },
        None => match std::env::current_dir()
            .ok()
            .and_then(|dir| Project::find_in(&dir))
        {
            Some(project) => project,
            None => {
                return usage_error(
                    "no .uproject here; run this in a project folder or pass --project",
                );
            }
        },
    };
    let engine = match engine::discover(args.engine.as_deref(), Some(&project), &probe) {
        Ok(Some(engine)) => engine,
        Ok(None) => {
            return usage_error("couldn't find an Unreal Engine install; pass --engine <path>");
        }
        Err(error) => return usage_error(&error),
    };
    let Some(platform) = clangd::host_platform(probe.os()) else {
        return usage_error("clangd setup supports macOS, Linux and Windows");
    };
    let target = match clangd::choose_target(&project, args.target.as_deref()) {
        Ok(target) => target,
        Err(error) => return usage_error(&error),
    };

    let mut out = anstream::stdout();
    let command =
        clangd::UbtCommand::new(&engine, &project, &target, platform, &args.configuration);
    let _ = writeln!(
        out,
        "\n  {}  {} with {}",
        paint(BOLD, "salvage clangd"),
        project.name,
        paint(
            DIM,
            format!("{} at {}", engine.label(), engine.root.display())
        )
    );
    if args.dry_run {
        let _ = writeln!(out, "\n  Would run:\n    {}", command.display());
        let _ = writeln!(
            out,
            "  Then write {} into {}{}{}.\n",
            clangd::CLANGD_FILE,
            project.dir.display(),
            if args.vscode {
                ", plus .vscode/settings.json"
            } else {
                ""
            },
            if args.neovim { ", plus .nvim.lua" } else { "" }
        );
        return ExitCode::SUCCESS;
    }

    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let request = clangd::Request {
        target: target.clone(),
        platform: platform.to_string(),
        configuration: args.configuration.clone(),
        vscode: args.vscode,
        neovim: args.neovim,
        refresh: args.refresh,
        os: probe.os(),
        stamp,
    };
    let mut run_ubt = |command: &clangd::UbtCommand| -> Result<(), String> {
        let mut out = anstream::stdout();
        let _ = writeln!(
            out,
            "\n  Running UnrealBuildTool for {target} ({platform} {}). This usually takes under a minute.\n",
            args.configuration
        );
        let _ = out.flush();
        let status = std::process::Command::new(&command.program)
            .args(&command.args)
            .status()
            .map_err(|e| format!("can't run {}: {e}", command.program.display()))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!(
                "UnrealBuildTool failed ({status}). The command was:\n  {}",
                command.display()
            ))
        }
    };
    let outcome = match clangd::setup(&project, &engine, &request, &mut run_ubt) {
        Ok(outcome) => outcome,
        Err(error) => {
            let mut stderr = anstream::stderr();
            let _ = writeln!(stderr, "{} {error}", paint(BAD, "salvage:"));
            return ExitCode::FAILURE;
        }
    };

    let done = paint(GOOD, "✓");
    let relative = |path: &std::path::Path| {
        path.strip_prefix(&project.dir)
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| path.display().to_string())
    };
    let _ = writeln!(out);
    match (&outcome.skipped, &outcome.reason) {
        (Some(why), _) => {
            let _ = writeln!(out, "  {done} {} is up to date: {why}", clangd::DATABASE);
        }
        (None, reason) => {
            let entries = outcome
                .entries
                .map(|n| format!(" ({n} files)"))
                .unwrap_or_default();
            let because = reason
                .as_ref()
                .map(|r| format!(", because {r}"))
                .unwrap_or_default();
            let _ = writeln!(out, "  {done} Wrote {}{entries}{because}", clangd::DATABASE);
        }
    }
    for (path, written) in &outcome.files {
        let line = match written {
            salvage::fsutil::Written::Created => format!("Wrote {}", relative(path)),
            salvage::fsutil::Written::Unchanged => format!("{} is already set up", relative(path)),
            salvage::fsutil::Written::Removed => format!("Removed {}", relative(path)),
            salvage::fsutil::Written::Replaced { backup } => {
                format!("Updated {} (backup: {})", relative(path), relative(backup))
            }
        };
        let _ = writeln!(out, "  {done} {line}");
    }
    let generated = &outcome.generated;
    if !generated.missing.is_empty() {
        let list: Vec<&str> = generated
            .missing
            .iter()
            .take(5)
            .map(String::as_str)
            .collect();
        let more = if generated.missing.len() > 5 {
            ", ..."
        } else {
            ""
        };
        let _ = writeln!(
            out,
            "  {} {} of {} generated headers are missing ({}{more}).\n      Build {target} once so UnrealHeaderTool writes them; until then clangd flags UCLASS code.",
            paint(WARN, "!"),
            generated.missing.len(),
            generated.expected,
            list.join(", ")
        );
    }
    let _ = writeln!(
        out,
        "\n  Open the project folder in an editor with clangd. Run `salvage clangd --refresh`\n  after adding source files or changing Build.cs files.\n"
    );
    ExitCode::SUCCESS
}

fn usage_error(message: &str) -> ExitCode {
    let mut stderr = anstream::stderr();
    let _ = writeln!(stderr, "{} {message}", paint(BAD, "salvage:"));
    ExitCode::from(2)
}
