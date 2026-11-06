//! Embeds the commit a binary was built from, for `salvage --version`.

use std::process::Command;

fn main() {
    println!("cargo:rerun-if-env-changed=SALVAGE_COMMIT");
    println!("cargo:rerun-if-env-changed=GITHUB_SHA");

    let commit = std::env::var("SALVAGE_COMMIT")
        .ok()
        .or_else(|| std::env::var("GITHUB_SHA").ok())
        .filter(|sha| !sha.trim().is_empty())
        .or_else(git_head);
    let short = commit
        .map(|sha| sha.trim().chars().take(12).collect::<String>())
        .unwrap_or_default();
    println!("cargo:rustc-env=SALVAGE_COMMIT={short}");
}

/// The current commit, watching the files that change when it does.
fn git_head() -> Option<String> {
    let git = |args: &[&str]| -> Option<String> {
        let out = Command::new("git").args(args).output().ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
    };
    let mut watch = vec!["HEAD".to_string(), "packed-refs".to_string()];
    watch.extend(git(&["symbolic-ref", "-q", "HEAD"]));
    for name in watch {
        if let Some(path) = git(&["rev-parse", "--git-path", &name]) {
            println!("cargo:rerun-if-changed={path}");
        }
    }
    git(&["rev-parse", "HEAD"])
}
