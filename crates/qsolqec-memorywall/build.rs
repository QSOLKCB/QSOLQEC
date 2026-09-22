use std::path::{Path, PathBuf};
use std::process::Command;

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

fn reject_dirty_checkout(root: &Path) {
    let status = git_output(
        Some(root),
        &["status", "--porcelain=v1", "--untracked-files=all"],
    )
    .unwrap_or_else(|| panic!("cannot inspect QSOLQEC worktree cleanliness"));

    let dirty: Vec<&str> = status
        .lines()
        // Cargo currently generates an untracked workspace Cargo.lock in this
        // repository. It is build metadata, not an input to the benchmark
        // executable's source semantics, and is therefore ignored here.
        .filter(|line| line.trim() != "?? Cargo.lock")
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
    let mut command = Command::new("git");
    if let Some(root) = root {
        command.arg("-C").arg(root);
    }

    let output = command.args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }

    let value = String::from_utf8(output.stdout).ok()?;
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

fn is_full_sha(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}
