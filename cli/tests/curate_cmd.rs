// TDD red phase for S5 curate command.
// Fails to compile until `griff curate` is implemented.
// S14 Phase 3 extends the suite: curate must persist structure metrics
// (fails to compile until `ChunkMeta.structure` exists).
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::missing_assert_message,
    clippy::absolute_paths
)]

use std::{
    io::Write as _,
    path::PathBuf,
    process::{Command, Stdio},
};

use griff_core::{corpus::ChunkMeta, midi, score::AtomEvent, structure};

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

/// S14 Phase 3: the written record carries the measured structure metrics of
/// the first note-bearing track, equal to what `measure_structure` reports.
#[test]
fn curate_records_structure_metrics_of_the_first_note_bearing_track() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/simple_4_4.mid");
    let out_path =
        std::env::temp_dir().join(format!("griff_curate_p3_{}.chunk.json", std::process::id()));

    let mut child = Command::new(griff_bin())
        .arg("curate")
        .arg(&fixture)
        .arg("--output")
        .arg(&out_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn griff curate");
    child
        .stdin
        .as_mut()
        .expect("piped stdin")
        // id, title, tuning (default), tags (none), flags (default), decision (none)
        .write_all(b"p3_001\nPhase Three\n\n\n\n\n")
        .expect("write curate answers");
    let out = child.wait_with_output().expect("wait for curate");
    assert!(
        out.status.success(),
        "curate must exit 0: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let json = std::fs::read_to_string(&out_path).expect("curate wrote the record");
    // Cleanup is best-effort; the named binding satisfies let-underscore lints.
    let _cleanup = std::fs::remove_file(&out_path);
    let meta: ChunkMeta = serde_json::from_str(&json).expect("record parses as ChunkMeta");

    let bytes = std::fs::read(&fixture).expect("fixture bytes");
    let score = midi::import_score(&bytes).expect("fixture imports");
    let track = score
        .tracks
        .iter()
        .position(|t| {
            t.voices
                .iter()
                .flat_map(|v| &v.event_groups)
                .flat_map(|g| &g.atoms)
                .any(|a| matches!(a, AtomEvent::Note(_)))
        })
        .expect("fixture has a note-bearing track");
    let expected = structure::measure_structure(&score, track).expect("metrics measure");

    assert_eq!(
        meta.structure,
        Some(expected),
        "curate persists the measured metrics"
    );
}
