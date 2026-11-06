//! `salvage doctor`: checks a machine for the things that make Unreal
//! development slow or broken, and says how to fix each one.

mod checks;
mod render;

use std::path::Path;

use serde::Serialize;

use crate::engine::Engine;
use crate::probe::{Os, Probe};
use crate::project::Project;

pub use render::{render_human, render_json};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Pass,
    /// Doesn't apply here (another OS, or nothing to check against).
    Skip,
    Warn,
    Fail,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Group {
    Engine,
    Toolchain,
    Disk,
    Memory,
    Debugging,
}

impl Group {
    pub const ORDER: [Group; 5] = [
        Group::Engine,
        Group::Toolchain,
        Group::Disk,
        Group::Memory,
        Group::Debugging,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Group::Engine => "Engine",
            Group::Toolchain => "Xcode and Metal",
            Group::Disk => "Disk",
            Group::Memory => "Memory",
            Group::Debugging => "Debugging",
        }
    }
}

/// How to fix a problem: a sentence, and optionally a command to run.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Fix {
    pub hint: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
}

impl Fix {
    pub fn hint(hint: impl Into<String>) -> Fix {
        Fix {
            hint: hint.into(),
            command: None,
        }
    }

    pub fn run(hint: impl Into<String>, command: impl Into<String>) -> Fix {
        Fix {
            hint: hint.into(),
            command: Some(command.into()),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Check {
    pub id: &'static str,
    pub group: Group,
    pub title: &'static str,
    pub status: Status,
    pub summary: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub details: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix: Option<Fix>,
}

impl Check {
    fn new(id: &'static str, group: Group, title: &'static str) -> Check {
        Check {
            id,
            group,
            title,
            status: Status::Pass,
            summary: String::new(),
            details: Vec::new(),
            fix: None,
        }
    }

    fn with(mut self, status: Status, summary: impl Into<String>) -> Check {
        self.status = status;
        self.summary = summary.into();
        self
    }

    fn pass(self, summary: impl Into<String>) -> Check {
        self.with(Status::Pass, summary)
    }

    fn warn(self, summary: impl Into<String>) -> Check {
        self.with(Status::Warn, summary)
    }

    fn fail(self, summary: impl Into<String>) -> Check {
        self.with(Status::Fail, summary)
    }

    fn skip(self, summary: impl Into<String>) -> Check {
        self.with(Status::Skip, summary)
    }

    fn detail(mut self, line: impl Into<String>) -> Check {
        self.details.push(line.into());
        self
    }

    fn fix(mut self, fix: Fix) -> Check {
        self.fix = Some(fix);
        self
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Platform {
    pub os: Os,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub os_version: Option<String>,
    pub arch: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<u64>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Counts {
    pub pass: usize,
    pub warn: usize,
    pub fail: usize,
    pub skip: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct Report {
    pub platform: Platform,
    pub engine: Option<Engine>,
    pub project: Option<Project>,
    pub checks: Vec<Check>,
}

impl Report {
    pub fn counts(&self) -> Counts {
        let mut counts = Counts::default();
        for check in &self.checks {
            match check.status {
                Status::Pass => counts.pass += 1,
                Status::Warn => counts.warn += 1,
                Status::Fail => counts.fail += 1,
                Status::Skip => counts.skip += 1,
            }
        }
        counts
    }

    pub fn ok(&self) -> bool {
        self.checks.iter().all(|c| c.status != Status::Fail)
    }

    pub fn check(&self, id: &str) -> Option<&Check> {
        self.checks.iter().find(|c| c.id == id)
    }
}

/// What the checks can see.
pub struct Context<'a> {
    pub probe: &'a dyn Probe,
    pub engine: Option<&'a Engine>,
    pub project: Option<&'a Project>,
}

impl Context<'_> {
    fn is_mac(&self) -> bool {
        self.probe.os() == Os::Macos
    }

    fn home(&self) -> Option<std::path::PathBuf> {
        self.probe.home()
    }

    fn tilde(&self, path: &Path) -> String {
        match self.home() {
            Some(home) => match path.strip_prefix(&home) {
                Ok(rest) if rest.as_os_str().is_empty() => "~".to_string(),
                Ok(rest) => format!("~/{}", rest.display()),
                Err(_) => path.display().to_string(),
            },
            None => path.display().to_string(),
        }
    }
}

/// Runs every check.
pub fn run(probe: &dyn Probe, engine: Option<Engine>, project: Option<Project>) -> Report {
    let platform = Platform {
        os: probe.os(),
        os_version: probe.os_version(),
        arch: probe.arch(),
        cpu: probe.cpu(),
        memory: probe.memory().map(|m| m.total),
    };
    let checks = {
        let ctx = Context {
            probe,
            engine: engine.as_ref(),
            project: project.as_ref(),
        };
        checks::all(&ctx)
    };
    Report {
        platform,
        engine,
        project,
        checks,
    }
}

#[cfg(test)]
mod tests;
