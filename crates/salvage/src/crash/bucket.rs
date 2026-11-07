//! A stable signature for a crash, so the same crash groups together across
//! runs: the top frames of the crashing thread inside your own modules,
//! skipping the engine's assert and crash plumbing and the system's.

/// Function names that belong to crash handling, asserts and ensures rather
/// than to what went wrong.
const PLUMBING: [&str; 24] = [
    "FDebug::",
    "FGenericPlatformMisc::RaiseException",
    "FMacPlatformMisc::RaiseException",
    "FApplePlatformMisc::RaiseException",
    "FMacErrorOutputDevice",
    "FOutputDevice::LogfImpl",
    "FOutputDevice::Logf",
    "FMacCrashContext",
    "FApplePlatformCrashContext",
    "FGenericCrashContext",
    "PlatformCrashHandler",
    "CrashHandler",
    "ReportAssert",
    "ReportEnsure",
    "AssertFailed",
    "CheckVerifyFailed",
    "EnsureFailed",
    "OptionallyLogFormattedEnsureMessageReturningFalse",
    "LowLevelFatalError",
    "FMacPlatformStackWalk",
    "FGenericPlatformStackWalk",
    "UE::Assert",
    "_sigtramp",
    "__pthread_kill",
];

/// One frame as bucketing sees it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignatureFrame {
    /// The module, without `lib` and `.dylib`: `UnrealEditor-Wreck`.
    pub module: String,
    /// The function, or `+0x1a2b` (offset in its image) when unsymbolicated.
    pub function: String,
    /// It's in your own code (not the engine or the system).
    pub own: bool,
    pub system: bool,
}

/// `libUnrealEditor-Wreck.dylib` as `UnrealEditor-Wreck`; other names as they are.
pub fn module_label(name: &str) -> String {
    let unreal = name.strip_prefix("lib").filter(|n| n.starts_with("Unreal"));
    let name = unreal.unwrap_or(name);
    if unreal.is_some() {
        name.strip_suffix(".dylib").unwrap_or(name).to_string()
    } else {
        name.to_string()
    }
}

pub fn is_plumbing(frame: &SignatureFrame) -> bool {
    frame.system || PLUMBING.iter().any(|p| frame.function.contains(p))
}

/// The signature: up to `count` frames, your own first; if none of the
/// crashing thread is yours, the top frames that aren't plumbing.
pub fn signature(frames: &[SignatureFrame], count: usize) -> Vec<String> {
    let useful: Vec<&SignatureFrame> = frames.iter().filter(|f| !is_plumbing(f)).collect();
    let own: Vec<&SignatureFrame> = useful.iter().copied().filter(|f| f.own).collect();
    let chosen = if own.is_empty() { useful } else { own };
    chosen
        .into_iter()
        .take(count)
        .map(|f| format!("{}!{}", f.module, f.function))
        .collect()
}

/// A short, stable id for a signature.
pub fn id(signature: &[String]) -> String {
    let hash = signature
        .join("\n")
        .bytes()
        .fold(0xcbf29ce484222325u64, |h, b| {
            (h ^ u64::from(b)).wrapping_mul(0x100000001b3)
        });
    format!("{:012x}", hash & 0xffff_ffff_ffff)
}
