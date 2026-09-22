use std::env;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-env-changed=QSOLQEC_SOURCE_SHA");

    let revision = env::var("QSOLQEC_SOURCE_SHA")
        .ok()
        .filter(|value| is_full_sha(value.trim()))
        .map(|value| value.trim().to_owned())
        .or_else(git_head)
        .unwrap_or_else(|| {
            panic!(
                "cannot determine QSOLQEC source revision; build inside the Git checkout or set QSOLQEC_SOURCE_SHA to a 40-hex commit SHA"
            )
        });

    println!("cargo:rustc-env=QSOLQEC_BUILD_SOURCE_SHA={revision}");
}

fn git_head() -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let revision = String::from_utf8(output.stdout).ok()?;
    let revision = revision.trim();
    is_full_sha(revision).then(|| revision.to_owned())
}

fn is_full_sha(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}
