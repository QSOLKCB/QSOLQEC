use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{self, Command};

fn main() {
    let root = git_output(None, &["rev-parse", "--show-toplevel"])
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            panic!(
                "qsolqec-memorywall benchmark builds require a Git checkout so source provenance can be verified"
            )
        });

    emit_git_rerun_guards(&root);
    reject_dirty_checkout(&root);

    let revision = git_output(Some(&root), &["rev-parse", "HEAD"]).unwrap_or_else(|| {
        panic!("cannot determine QSOLQEC source revision from the clean Git checkout")
    });

    if !is_full_sha(&revision) {
        panic!("Git HEAD is not a full 40-hex commit SHA: {revision:?}");
    }

    verify_public_revision(&revision);

    println!("cargo:rustc-env=QSOLQEC_BUILD_SOURCE_SHA={revision}");
}

fn emit_git_rerun_guards(root: &Path) {
    // Cargo's build-script cache must be invalidated by source changes and by
    // Git ref movement even when the same target directory is reused.
    let tracked = git_output(Some(root), &["ls-files"]).unwrap_or_else(|| {
        panic!("cannot enumerate tracked QSOLQEC files for provenance rerun guards")
    });

    for relative in tracked.lines().filter(|line| !line.trim().is_empty()) {
        println!("cargo:rerun-if-changed={}", root.join(relative).display());
    }

    let git_dir = git_output(Some(root), &["rev-parse", "--absolute-git-dir"])
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("cannot resolve QSOLQEC Git directory"));

    println!("cargo:rerun-if-changed={}", git_dir.join("HEAD").display());
    println!("cargo:rerun-if-changed={}", git_dir.join("index").display());
    println!(
        "cargo:rerun-if-changed={}",
        git_dir.join("packed-refs").display()
    );

    if let Some(symbolic_ref) = git_output(Some(root), &["symbolic-ref", "-q", "HEAD"]) {
        println!(
            "cargo:rerun-if-changed={}",
            git_dir.join(symbolic_ref).display()
        );
    }
}

fn verify_public_revision(revision: &str) {
    const PUBLIC_REPOSITORY: &str = "https://github.com/QSOLKCB/QSOLQEC.git";

    // Verify against an empty object database. Fetching from the source
    // checkout itself is insufficient because Git may already have a local-only
    // object and therefore would not prove the public server can supply it.
    let probe = env::temp_dir().join(format!(
        "qsolqec-public-revision-{}-{revision}",
        process::id()
    ));
    let _ = fs::remove_dir_all(&probe);

    let initialized = Command::new("git")
        .args(["init", "--bare", "--quiet"])
        .arg(&probe)
        .status()
        .unwrap_or_else(|error| {
            panic!(
                "cannot initialize temporary Git repository for public revision verification: {error}"
            )
        });

    if !initialized.success() {
        panic!("cannot initialize temporary Git repository for public revision verification");
    }

    let status = Command::new("git")
        .arg("-C")
        .arg(&probe)
        .args([
            "fetch",
            "--quiet",
            "--no-tags",
            "--depth=1",
            "--no-write-fetch-head",
            PUBLIC_REPOSITORY,
            revision,
        ])
        .env("GIT_TERMINAL_PROMPT", "0")
        .status()
        .unwrap_or_else(|error| {
            panic!("cannot invoke Git to verify public QSOLQEC revision {revision}: {error}")
        });

    let _ = fs::remove_dir_all(&probe);

    if !status.success() {
        panic!(
            "refusing memory-wall benchmark build because revision {revision} is not retrievable from {PUBLIC_REPOSITORY}"
        );
    }
}

fn reject_dirty_checkout(root: &Path) {
    let status = git_output_allow_empty(
        Some(root),
        &["status", "--porcelain=v1", "--untracked-files=all"],
    )
    .unwrap_or_else(|| panic!("cannot inspect QSOLQEC worktree cleanliness"));

    let dirty: Vec<&str> = status
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();

    if !dirty.is_empty() {
        panic!(
            "refusing memory-wall benchmark build from a dirty checkout; commit or revert all source changes first:\n{}",
            dirty.join("\n")
        );
    }
}

fn git_output(root: Option<&Path>, args: &[&str]) -> Option<String> {
    let value = git_output_allow_empty(root, args)?;
    (!value.is_empty()).then_some(value)
}

fn git_output_allow_empty(root: Option<&Path>, args: &[&str]) -> Option<String> {
    let mut command = Command::new("git");
    if let Some(root) = root {
        command.arg("-C").arg(root);
    }

    let output = command.args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }

    let value = String::from_utf8(output.stdout).ok()?;
    Some(value.trim().to_owned())
}

fn is_full_sha(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}
