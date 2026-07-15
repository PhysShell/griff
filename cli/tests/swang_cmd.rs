//! S16 Phase 3: `griff swang check | fmt` — the grammar's CLI edge.
//!
//! `check` parses a `.swg` script and renders its diagnostics as
//! `error[SWG____] (<path>:<line>:<col>): <message>` — the transport's
//! rendering with a source span where the flag used to be (spec §1.5,
//! §3.5 law 4). `fmt` prints the canonical text to stdout and refuses to
//! output anything for a program that does not parse. Neither command
//! touches music: Phase 3 adds no semantics.

// Reason: integration-test code. `unwrap`/`expect`/`panic` abort loudly with
// a clear message, which is exactly what a test harness wants.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::missing_assert_message
)]

mod common;

use std::path::PathBuf;
use std::process::Command;
use std::{env, fs};

use common::{assert_golden, griff};

/// The spec §3.1 reference program — the canonical text `fmt` must treat as
/// its fixed point (same fixture as the library's syntax tests).
const CANONICAL: &str = r#"swang 1

pattern dgd_fractal {
    ascii "X.X/XX./.XX"
    |> fractalize depth 1 max_cells 4096 density 9500bps seed 4
    |> linearize snake
    |> map_rhythm unit 1/16 tail rest_pad
    |> generate {
        source "corpus/Dance Gavin Dance - The Robot With Human Hair Part 2.gp5"
        bars 8
        seed 42
        candidates 2
        strategy repeat_variation
        corpus "corpus"
    }
    |> export midi "dgd_fractal_dense.mid"
}
"#;

/// The same program with scrambled word order, ragged indentation, and a
/// single-line generate block — one canonical text must come out.
const MESSY: &str = "swang 1\n\n\npattern   dgd_fractal {\n  ascii \"X.X/XX./.XX\"\n      |> fractalize max_cells 4096 seed 4 density 9500bps depth 1\n  |> linearize snake\n    |> map_rhythm tail rest_pad unit 1/16\n  |> generate { bars 8 source \"corpus/Dance Gavin Dance - The Robot With Human Hair Part 2.gp5\" strategy repeat_variation seed 42 corpus \"corpus\" candidates 2 }\n  |> export midi \"dgd_fractal_dense.mid\"\n}\n";

/// A program whose only flaw is a seedless density on line 5 — the parser
/// must locate `SWG0303` by line and column in the script, not by a flag.
const SEEDLESS_DENSITY: &str = r#"swang 1

pattern p {
    ascii "X.X/XX./.XX"
    |> fractalize depth 1 max_cells 4096 density 9500bps
    |> linearize snake
    |> map_rhythm unit 1/16 tail rest_pad
    |> generate {
        source "seed.gp5"
        bars 8
        seed 42
        candidates 2
        strategy auto
    }
    |> export midi "out.mid"
}
"#;

/// Writes a script to a scrubbed temp path.
fn script(name: &str, text: &str) -> PathBuf {
    let path = env::temp_dir().join(format!("griff_s16_swang_{name}.swg"));
    fs::write(&path, text).expect("temp script must write");
    path
}

/// Runs the binary raw, for byte-exact stdout assertions.
fn griff_raw(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_griff"))
        .args(args)
        .output()
        .expect("griff binary must run")
}

#[test]
fn swang_check_accepts_the_reference_program() {
    let path = script("check_ok", CANONICAL);
    let out = griff_raw(&["swang", "check", path.to_str().unwrap()]);
    fs::remove_file(&path).ok();
    assert!(
        out.status.success(),
        "check must accept the reference program: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.stdout.is_empty(), "a clean check says nothing");
    assert!(out.stderr.is_empty(), "a clean check warns about nothing");
}

#[test]
fn swang_check_locates_a_seedless_density_by_line_and_column() {
    let path = script("check_seedless", SEEDLESS_DENSITY);
    let text = griff(
        &["swang", "check", path.to_str().unwrap()],
        Some(path.to_str().unwrap()),
    );
    fs::remove_file(&path).ok();
    assert!(text.contains("exit: 1"), "{text}");
    assert!(
        text.contains("error[SWG0303]"),
        "the semantic code keeps its transport number: {text}"
    );
    assert!(
        text.contains("<OUT>:5:"),
        "the location is a source line in the script, not a flag: {text}"
    );
    assert_golden("swang__check_seedless_density", &text);
}

#[test]
fn swang_check_rejects_a_bom_at_one_one() {
    let path = script("check_bom", "\u{feff}swang 1\n");
    let out = griff_raw(&["swang", "check", path.to_str().unwrap()]);
    fs::remove_file(&path).ok();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("error[SWG0003]"), "{stderr}");
    assert!(stderr.contains(":1:1"), "the BOM sits at 1:1: {stderr}");
}

#[test]
fn swang_fmt_normalizes_to_the_canonical_text() {
    let path = script("fmt_messy", MESSY);
    let out = griff_raw(&["swang", "fmt", path.to_str().unwrap()]);
    fs::remove_file(&path).ok();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        CANONICAL,
        "one canonical text per program, byte for byte"
    );
}

#[test]
fn swang_fmt_is_a_fixed_point_on_canonical_text() {
    let path = script("fmt_canonical", CANONICAL);
    let out = griff_raw(&["swang", "fmt", path.to_str().unwrap()]);
    fs::remove_file(&path).ok();
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        CANONICAL,
        "fmt(fmt(s)) == fmt(s), at the CLI too"
    );
}

#[test]
fn swang_fmt_refuses_what_check_refuses() {
    let path = script("fmt_seedless", SEEDLESS_DENSITY);
    let out = griff_raw(&["swang", "fmt", path.to_str().unwrap()]);
    fs::remove_file(&path).ok();
    assert_eq!(out.status.code(), Some(1));
    assert!(
        out.stdout.is_empty(),
        "fmt never emits text for a program that does not parse"
    );
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("error[SWG0303]"),
        "fmt fails with check's own diagnostic"
    );
}
