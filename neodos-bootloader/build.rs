//! Build script: embed the short commit id so the bootloader banner identifies
//! the exact build. Falls back to `"unknown"` when git is unavailable.

/// Short commit id (with a `-dirty` suffix on tracked changes), or `"unknown"`
/// outside a git repository.
fn git_revision() -> String {
    let Some(sha) = git_stdout(&["rev-parse", "--short", "HEAD"]) else {
        return "unknown".to_string();
    };
    // `git diff-index --quiet HEAD --` exits non-zero when the working tree
    // differs from HEAD (staged or unstaged tracked changes).
    let dirty = std::process::Command::new("git")
        .args(["diff-index", "--quiet", "HEAD", "--"])
        .status()
        .map(|s| !s.success())
        .unwrap_or(false);
    if dirty { format!("{sha}-dirty") } else { sha }
}

/// Run `git` with `args`, returning trimmed stdout on success.
fn git_stdout(args: &[&str]) -> Option<String> {
    let out = std::process::Command::new("git").args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() { None } else { Some(s) }
}

fn main() {
    let git_rev = git_revision();
    println!("cargo:rustc-env=NEODOS_GIT_REV={git_rev}");

    // Re-run when HEAD or refs move (new commit, branch switch, new tag).
    if std::path::Path::new("../.git/HEAD").exists() {
        println!("cargo:rerun-if-changed=../.git/HEAD");
        println!("cargo:rerun-if-changed=../.git/refs");
    }
}
