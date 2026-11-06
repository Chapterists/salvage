//! Everything doctor learns about the machine goes through [`Probe`], so the
//! checks can be tested against invented machines.

use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Os {
    Macos,
    Linux,
    Windows,
    Other,
}

impl Os {
    pub fn current() -> Self {
        match std::env::consts::OS {
            "macos" => Os::Macos,
            "linux" => Os::Linux,
            "windows" => Os::Windows,
            _ => Os::Other,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Os::Macos => "macOS",
            Os::Linux => "Linux",
            Os::Windows => "Windows",
            Os::Other => "this platform",
        }
    }
}

/// What a finished command printed.
#[derive(Clone, Debug, Default)]
pub struct Output {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

impl Output {
    /// Both streams, for matching error text wherever the tool put it.
    pub fn text(&self) -> String {
        format!("{}\n{}", self.stdout, self.stderr)
    }

    /// The first non-empty line of stdout, else of stderr.
    pub fn first_line(&self) -> String {
        [&self.stdout, &self.stderr]
            .iter()
            .flat_map(|s| s.lines())
            .map(str::trim)
            .find(|l| !l.is_empty())
            .unwrap_or("")
            .to_string()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Memory {
    pub total: u64,
    pub swap_total: u64,
    pub swap_used: u64,
}

pub trait Probe {
    fn os(&self) -> Os;
    /// CPU architecture as Rust names it: `aarch64`, `x86_64`...
    fn arch(&self) -> String;
    fn os_version(&self) -> Option<String>;
    fn cpu(&self) -> Option<String>;
    /// Runs a program to completion. `None` if it could not be started.
    fn run(&self, program: &str, args: &[&str]) -> Option<Output>;
    fn env(&self, key: &str) -> Option<String>;
    fn home(&self) -> Option<PathBuf>;
    fn free_space(&self, path: &Path) -> Option<u64>;
    fn memory(&self) -> Option<Memory>;
    fn rosetta_installed(&self) -> bool;

    /// Folders the Epic Games Launcher installs engines into by default.
    fn install_roots(&self) -> Vec<PathBuf> {
        match self.os() {
            Os::Macos => vec![PathBuf::from("/Users/Shared/Epic Games")],
            Os::Windows => {
                let program_files = self
                    .env("ProgramFiles")
                    .unwrap_or_else(|| r"C:\Program Files".to_string());
                vec![Path::new(&program_files).join("Epic Games")]
            }
            _ => Vec::new(),
        }
    }

    /// The Launcher's list of installed apps, engines included.
    fn launcher_manifests(&self) -> Vec<PathBuf> {
        let manifest = Path::new("UnrealEngineLauncher").join("LauncherInstalled.dat");
        match self.os() {
            Os::Macos => self
                .home()
                .map(|h| h.join("Library/Application Support/Epic").join(&manifest))
                .into_iter()
                .collect(),
            Os::Windows => self
                .env("ProgramData")
                .map(|d| Path::new(&d).join("Epic").join(&manifest))
                .into_iter()
                .collect(),
            _ => Vec::new(),
        }
    }

    /// Where source-built engines register themselves (`[Installations]`).
    fn source_build_registries(&self) -> Vec<PathBuf> {
        let Some(home) = self.home() else {
            return Vec::new();
        };
        match self.os() {
            Os::Macos => {
                vec![home.join("Library/Application Support/Epic/UnrealEngine/Install.ini")]
            }
            Os::Linux => vec![home.join(".config/Epic/UnrealEngine/Install.ini")],
            _ => Vec::new(),
        }
    }
}

/// The real machine.
pub struct SystemProbe;

impl Probe for SystemProbe {
    fn os(&self) -> Os {
        Os::current()
    }

    fn arch(&self) -> String {
        std::env::consts::ARCH.to_string()
    }

    fn os_version(&self) -> Option<String> {
        if self.os() == Os::Macos {
            let out = self.run("sw_vers", &["-productVersion"])?;
            return out.success.then(|| out.first_line());
        }
        sysinfo::System::long_os_version().ok()
    }

    fn cpu(&self) -> Option<String> {
        if self.os() == Os::Macos {
            let out = self.run("sysctl", &["-n", "machdep.cpu.brand_string"])?;
            return out.success.then(|| out.first_line());
        }
        let refresh = sysinfo::RefreshKind::nothing().with_cpu(sysinfo::CpuRefreshKind::nothing());
        let system = sysinfo::System::new_with_specifics(refresh).ok()?;
        let brand = system.cpus().first()?.brand().trim().to_string();
        (!brand.is_empty()).then_some(brand)
    }

    fn run(&self, program: &str, args: &[&str]) -> Option<Output> {
        let out = Command::new(program).args(args).output().ok()?;
        Some(Output {
            success: out.status.success(),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        })
    }

    fn env(&self, key: &str) -> Option<String> {
        std::env::var(key).ok().filter(|v| !v.is_empty())
    }

    fn home(&self) -> Option<PathBuf> {
        let key = if self.os() == Os::Windows {
            "USERPROFILE"
        } else {
            "HOME"
        };
        self.env(key).map(PathBuf::from)
    }

    fn free_space(&self, path: &Path) -> Option<u64> {
        fs4::available_space(path).ok()
    }

    fn memory(&self) -> Option<Memory> {
        let refresh =
            sysinfo::RefreshKind::nothing().with_memory(sysinfo::MemoryRefreshKind::everything());
        let system = sysinfo::System::new_with_specifics(refresh).ok()?;
        Some(Memory {
            total: system.total_memory(),
            swap_total: system.total_swap(),
            swap_used: system.used_swap(),
        })
    }

    fn rosetta_installed(&self) -> bool {
        Path::new("/Library/Apple/usr/libexec/oah/libRosettaRuntime").exists()
    }
}
