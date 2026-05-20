// TDD red phase for S5 curate command.
// Fails to compile until `griff curate` is implemented.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::missing_assert_message
)]

use std::process::Command;

/// Locate the compiled `griff` binary.
fn griff_bin() -> std::path::PathBuf {
    // When running under `cargo test`, CARGO_BIN_EXE_griff is set.
    std::env::var_os("CARGO_BIN_EXE_griff")
        .map(Into::into)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/debug/griff")
        })
}

#[test]
fn curate_help_exits_zero() {
    let out = Command::new(griff_bin())
        .args(["curate", "--help"])
        .output()
        .expect("run griff curate --help");
    assert!(out.status.success(), "griff curate --help must exit 0");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("curate"),
        "help output must mention 'curate'"
    );
}

#[test]
fn curate_nonexistent_file_exits_nonzero() {
    let out = Command::new(griff_bin())
        .args(["curate", "nonexistent_file_that_does_not_exist.mid"])
        .output()
        .expect("run griff curate");
    assert!(
        !out.status.success(),
        "curate on missing file must exit non-zero"
    );
}
