// TDD red phase: corpus-fed candidate-set generation on the CLI — the wiring
// of the melodic-closure note (§7.2 rerank, §7.3 novelty measure) and the S6
// stage doc's promised candidate *set* into `griff generate`:
//
// - `--corpus <DIR>` points at a directory of curated `*.chunk.json` records
//   sitting next to their source tabs. The chunks supply per-bar rhythm
//   templates (instead of the input's first bar), novelty reference scores,
//   and an aggregated gesture ask.
// - `--candidates <N>` sets the seed variants per strategy in the candidate
//   set; every set is reranked under the `generation_rerank` v1 policy and
//   the winner is written to the output file.
// - `--no-gesture` opts out of gesture carving even when the corpus provides
//   stats.
//
// The flags do not exist yet, so the suite fails until the green step.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::missing_assert_message,
    clippy::absolute_paths
)]

use std::{
    fs,
    io::Write as _,
    path::PathBuf,
    process::{Command, Output, Stdio},
};

/// Locate the compiled `griff` binary.
fn griff_bin() -> PathBuf {
    std::env::var_os("CARGO_BIN_EXE_griff").map_or_else(
        || PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/debug/griff"),
        Into::into,
    )
}

/// The committed `two_phrases.mid` characterization fixture.
fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/two_phrases.mid")
}

/// Builds a corpus directory under the OS temp dir: one curated chunk record
/// of the `two_phrases` fixture plus the source file itself, named as the
/// record's `source.filename` expects.
fn build_corpus_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "griff_generate_corpus_{tag}_{}",
        std::process::id()
    ));
    fs::remove_dir_all(&dir).ok();
    fs::create_dir_all(&dir).expect("create corpus dir");

    let mut child = Command::new(griff_bin())
        .arg("curate")
        .arg(fixture())
        .arg("--output")
        .arg(dir.join("a.chunk.json"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn curate");
    child
        .stdin
        .as_mut()
        .expect("piped stdin")
        .write_all(b"a_001\nChunk a\n\n\n\n\n\n")
        .expect("write curate answers");
    assert!(
        child
            .wait_with_output()
            .expect("wait for curate")
            .status
            .success(),
        "curate must exit 0"
    );

    fs::copy(fixture(), dir.join("two_phrases.mid")).expect("copy source tab into corpus dir");
    dir
}

/// Runs `griff generate` with the given extra args, returning the raw output.
fn run_generate(out_file: &PathBuf, extra: &[&str]) -> Output {
    let mut cmd = Command::new(griff_bin());
    cmd.arg("generate")
        .arg(fixture())
        .arg(out_file)
        .args(["--seed", "7", "--bars", "4"]);
    cmd.args(extra);
    cmd.output().expect("run griff generate")
}

#[test]
fn generate_help_mentions_corpus_and_candidates() {
    let out = Command::new(griff_bin())
        .args(["generate", "--help"])
        .output()
        .expect("run griff generate --help");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    for flag in ["--corpus", "--candidates", "--no-gesture"] {
        assert!(stdout.contains(flag), "help must mention {flag}: {stdout}");
    }
}

#[test]
fn generate_with_corpus_ranks_candidates_and_writes_winner() {
    let dir = build_corpus_dir("rank");
    let out_file = dir.join("out.mid");

    let out = run_generate(&out_file, &["--corpus", dir.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "generate --corpus must exit 0: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("corpus:"),
        "stdout reports what the corpus supplied: {stdout}"
    );
    assert!(
        stdout.contains("ranked under generation_rerank v1"),
        "stdout names the rerank policy and version (ADR-0017 provenance): {stdout}"
    );
    assert!(
        !fs::read(&out_file).expect("winner written").is_empty(),
        "the top-ranked candidate lands in the output file"
    );

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn generate_with_corpus_is_deterministic() {
    let dir = build_corpus_dir("det");
    let out_a = dir.join("out_a.mid");
    let out_b = dir.join("out_b.mid");

    assert!(run_generate(&out_a, &["--corpus", dir.to_str().unwrap()])
        .status
        .success());
    assert!(run_generate(&out_b, &["--corpus", dir.to_str().unwrap()])
        .status
        .success());
    assert_eq!(
        fs::read(&out_a).expect("first run output"),
        fs::read(&out_b).expect("second run output"),
        "same seed + same corpus => byte-identical winner (SPEC §6)"
    );

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn generate_no_gesture_changes_the_carve_not_the_determinism() {
    let dir = build_corpus_dir("plain");
    let out_file = dir.join("out.mid");

    let out = run_generate(
        &out_file,
        &["--corpus", dir.to_str().unwrap(), "--no-gesture"],
    );
    assert!(
        out.status.success(),
        "generate --corpus --no-gesture must exit 0: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        !fs::read(&out_file).expect("winner written").is_empty(),
        "the un-carved winner still lands in the output file"
    );

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn generate_rejects_a_missing_corpus_dir() {
    let out_file = std::env::temp_dir().join(format!(
        "griff_generate_corpus_missing_{}.mid",
        std::process::id()
    ));
    let out = run_generate(
        &out_file,
        &["--corpus", "nonexistent_corpus_dir_that_does_not_exist"],
    );
    assert!(
        !out.status.success(),
        "a missing corpus dir is an error, not a silent fallback"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("corpus"),
        "the error names the corpus: {stderr}"
    );
    fs::remove_file(&out_file).ok();
}
