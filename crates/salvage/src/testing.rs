//! Invented machines and engine installs for tests. Every fixture here is
//! written by hand for Salvage; none is copied from an engine.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::probe::{Memory, Os, Output, Probe};
use crate::units::GIB;

/// A machine whose every answer is set by the test.
#[derive(Clone)]
pub struct FakeProbe {
    pub os: Os,
    pub arch: String,
    pub home: Option<PathBuf>,
    pub env: HashMap<String, String>,
    /// Command line (`"xcrun metal --version"`) to its output; absent = can't start.
    pub commands: HashMap<String, Output>,
    pub free_space: Option<u64>,
    pub memory: Option<Memory>,
    pub rosetta: bool,
    pub install_roots: Vec<PathBuf>,
}

impl FakeProbe {
    /// A healthy Apple silicon Mac with Xcode 30.1 and the Metal Toolchain.
    pub fn healthy_mac(home: &Path) -> FakeProbe {
        let developer = home.join("Xcode.app/Contents/Developer");
        std::fs::create_dir_all(&developer).unwrap();
        let mut probe = FakeProbe {
            os: Os::Macos,
            arch: "aarch64".into(),
            home: Some(home.to_path_buf()),
            env: HashMap::new(),
            commands: HashMap::new(),
            free_space: Some(200 * GIB),
            memory: Some(Memory {
                total: 32 * GIB,
                swap_total: 2 * GIB,
                swap_used: 0,
            }),
            rosetta: true,
            install_roots: Vec::new(),
        };
        probe.ok("xcodebuild -version", "Xcode 30.1\nBuild version 30A100\n");
        probe.ok("xcode-select -p", &format!("{}\n", developer.display()));
        probe.ok(
            "xcrun metal --version",
            "Apple metal version 99999.1 (example)\n",
        );
        probe
    }

    pub fn linux(home: &Path) -> FakeProbe {
        FakeProbe {
            os: Os::Linux,
            arch: "x86_64".into(),
            home: Some(home.to_path_buf()),
            env: HashMap::new(),
            commands: HashMap::new(),
            free_space: Some(200 * GIB),
            memory: Some(Memory {
                total: 16 * GIB,
                swap_total: 0,
                swap_used: 0,
            }),
            rosetta: false,
            install_roots: Vec::new(),
        }
    }

    pub fn ok(&mut self, command: &str, stdout: &str) {
        let out = Output {
            success: true,
            stdout: stdout.into(),
            stderr: String::new(),
        };
        self.commands.insert(command.into(), out);
    }

    pub fn err(&mut self, command: &str, stderr: &str) {
        let out = Output {
            success: false,
            stdout: String::new(),
            stderr: stderr.into(),
        };
        self.commands.insert(command.into(), out);
    }
}

impl Probe for FakeProbe {
    fn os(&self) -> Os {
        self.os
    }
    fn arch(&self) -> String {
        self.arch.clone()
    }
    fn os_version(&self) -> Option<String> {
        Some("99.0".into())
    }
    fn cpu(&self) -> Option<String> {
        Some("Example CPU".into())
    }
    fn run(&self, program: &str, args: &[&str]) -> Option<Output> {
        let line = std::iter::once(program)
            .chain(args.iter().copied())
            .collect::<Vec<_>>()
            .join(" ");
        self.commands.get(&line).cloned()
    }
    fn env(&self, key: &str) -> Option<String> {
        self.env.get(key).cloned()
    }
    fn home(&self) -> Option<PathBuf> {
        self.home.clone()
    }
    fn free_space(&self, _path: &Path) -> Option<u64> {
        self.free_space
    }
    fn memory(&self) -> Option<Memory> {
        self.memory
    }
    fn rosetta_installed(&self) -> bool {
        self.rosetta
    }
    fn install_roots(&self) -> Vec<PathBuf> {
        self.install_roots.clone()
    }
}

/// Our own Apple SDK fixture: same keys the engine uses, invented versions.
pub const APPLE_SDK_FIXTURE: &str = r#"{
    "// note": "Salvage test fixture with invented versions",
    "MainVersion": "30.1.0",
    "MinVersion": "29.0.0",
    "MaxVersion": "31.9.0",
}"#;

/// Our own engine-config fixture: a Mac section targeting two shader formats.
pub const BASE_ENGINE_FIXTURE: &str = "\
; Salvage test fixture
[/Script/MacTargetPlatform.MacTargetSettings]
+TargetedRHIs=SF_METAL_SM5
+TargetedRHIs=SF_METAL_SM6
";

/// Lays out a minimal fake engine install under `root` and returns `root`.
pub fn fake_engine(root: &Path, version: (u64, u64, u64)) -> PathBuf {
    let engine = root.join("Engine");
    let write = |rel: &str, text: &str| {
        let path = engine.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    };
    write(
        "Build/Build.version",
        &format!(
            r#"{{ "MajorVersion": {}, "MinorVersion": {}, "PatchVersion": {}, "Changelist": 1 }}"#,
            version.0, version.1, version.2
        ),
    );
    write("Build/InstalledBuild.txt", "fixture\n");
    write("Config/Apple/Apple_SDK.json", APPLE_SDK_FIXTURE);
    write("Config/BaseEngine.ini", BASE_ENGINE_FIXTURE);
    write(
        "Extras/LLDBDataFormatters/UEDataFormatters_2ByteChars.py",
        "# fixture\n",
    );
    root.to_path_buf()
}
