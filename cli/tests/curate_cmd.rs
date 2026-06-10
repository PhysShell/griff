// TDD red phase for S5 curate command.
// Fails to compile until `griff curate` is implemented.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::missing_assert_message,
    clippy::absolute_paths
)]

use std::{path::PathBuf, process::Command};

/// Locate the compiled `griff` binary.
fn griff_bin() -> PathBuf {
    // When running under `cargo test`, CARGO_BIN_EXE_griff is set.
    std::env::var_os("CARGO_BIN_EXE_griff").map_or_else(
        // absolute path ok in test
        || PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/debug/griff"),
        Into::into,
    )
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

#[test]
fn curate_persists_structure_metrics_in_the_chunk_json() {
    use std::io::Write as _;

    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/simple_4_4.mid");
    let out_path = std::env::temp_dir().join("griff_curate_structure_test.chunk.json");
    let _ = std::fs::remove_file(&out_path);

    let mut child = Command::new(griff_bin())
        .args([
            "curate",
            fixture.to_str().expect("fixture path"),
            "--output",
            out_path.to_str().expect("out path"),
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .spawn()
        .expect("spawn griff curate");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        // id, title, tuning (default), tags (none), flags (default), decision (none)
        .write_all(b"test_001\nStructure Test\n\n\n\n\n")
        .expect("write answers");
    let status = child.wait().expect("wait");
    assert!(status.success(), "curate must exit 0");

    let json = std::fs::read_to_string(&out_path).expect("output written");
    let value: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
    let structure = value
        .get("structure")
        .expect("chunk meta must carry a structure snapshot (S14 Phase 3)");
    assert!(
        structure.get("bar_count").and_then(serde_json::Value::as_u64) >= Some(1),
        "snapshot must describe at least one bar: {structure}"
    );
    let _ = std::fs::remove_file(&out_path);
}
