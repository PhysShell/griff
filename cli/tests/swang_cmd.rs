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

// Reason: this binary uses only the runner and the golden comparator; the
// shared module's MIDI fixture builders belong to the S0 suite in cli.rs.
#[allow(dead_code)]
mod common;

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::{env, fs};

use common::{assert_golden, fixture_path, griff};

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
fn griff_raw(args: &[&str]) -> Output {
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

// ── expand: law 1 and the layered locations (spec §3.5, §1.5) ──────────────

/// A program around the expansion knobs the tests turn. Line 7 is
/// `map_rhythm` — the unit-location test counts on it.
fn expand_program(kernel: &str, fractalize_args: &str, unit_and_tail: &str, bars: u64) -> String {
    let source = fixture_path("simple_4_4");
    expand_program_for(kernel, fractalize_args, unit_and_tail, bars, &source)
}

fn expand_program_for(
    kernel: &str,
    fractalize_args: &str,
    unit_and_tail: &str,
    bars: u64,
    source: &Path,
) -> String {
    format!(
        r#"swang 1

pattern p {{
    ascii "{kernel}"
    |> fractalize {fractalize_args}
    |> linearize snake
    |> map_rhythm {unit_and_tail}
    |> generate {{
        source "{}"
        bars {bars}
        seed 42
        candidates 2
        strategy auto
    }}
    |> export midi "out.mid"
}}
"#,
        source.display()
    )
}

#[test]
fn swang_expand_matches_the_transport_artifact_byte_for_byte() {
    // Law 1: the program equivalent to a Phase-2 CLI command produces a
    // byte-identical expansion JSON — same compiler, same artifact.
    let out = env::temp_dir().join("griff_s16_swang_expand_parity.mid");
    let art = env::temp_dir().join("griff_s16_swang_expand_parity.json");
    let src = fixture_path("simple_4_4");
    let transport = griff_raw(&[
        "generate",
        src.to_str().unwrap(),
        out.to_str().unwrap(),
        "--bars",
        "8",
        "--seed",
        "42",
        "--candidates",
        "2",
        "--rhythm-kernel",
        "X.X/XX./.XX",
        "--rhythm-fractal-depth",
        "1",
        "--rhythm-density-bps",
        "9500",
        "--rhythm-seed",
        "4",
        "--rhythm-traversal",
        "snake",
        "--rhythm-unit",
        "1/16",
        "--rhythm-max-cells",
        "4096",
        "--rhythm-tail",
        "rest-pad",
        "--emit-rhythm-expansion",
        art.to_str().unwrap(),
    ]);
    assert!(
        transport.status.success(),
        "the transport command must succeed: {}",
        String::from_utf8_lossy(&transport.stderr)
    );
    let transport_artifact = fs::read(&art).expect("the transport wrote its artifact");
    fs::remove_file(&out).ok();
    fs::remove_file(&art).ok();

    let program = expand_program(
        "X.X/XX./.XX",
        "depth 1 max_cells 4096 density 9500bps seed 4",
        "unit 1/16 tail rest_pad",
        8,
    );
    let path = script("expand_parity", &program);
    let expanded = griff_raw(&["swang", "expand", path.to_str().unwrap()]);
    fs::remove_file(&path).ok();
    assert!(
        expanded.status.success(),
        "{}",
        String::from_utf8_lossy(&expanded.stderr)
    );
    assert_eq!(
        expanded.stdout, transport_artifact,
        "one compiler, one artifact, byte for byte"
    );
}

#[test]
fn swang_expand_reports_structural_budget_breaches_by_node_path() {
    // §1.5: a structural error's location is its NodePath — the whole-grid
    // budget check breaks at the root.
    let program = expand_program(
        "X.X/XX./.XX",
        "depth 2 max_cells 80",
        "unit 1/16 tail rest_pad",
        8,
    );
    let path = script("expand_budget", &program);
    let out = griff_raw(&["swang", "expand", path.to_str().unwrap()]);
    fs::remove_file(&path).ok();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("error[SWG0201]"), "{stderr}");
    assert!(
        stderr.contains("node root"),
        "the up-front check breaks at the root: {stderr}"
    );
}

#[test]
fn swang_expand_locates_the_unit_at_its_word() {
    // 7/8 at PPQN 480 is a 1680-tick bar; a 1/4 unit (480) does not divide
    // it. The location is the unit value on line 7 of the program — the
    // word whose value must change — not a flag that no longer exists.
    let source = fixture_path("seven_eight");
    let program = expand_program_for(
        "X.X/XX./.XX",
        "depth 1 max_cells 4096",
        "unit 1/4 tail rest_pad",
        8,
        &source,
    );
    let path = script("expand_unit", &program);
    let text = griff(
        &["swang", "expand", path.to_str().unwrap()],
        Some(path.to_str().unwrap()),
    );
    fs::remove_file(&path).ok();
    assert!(text.contains("exit: 1"), "{text}");
    assert!(text.contains("error[SWG0301]"), "{text}");
    assert!(
        text.contains("<OUT>:7:"),
        "the unit word lives on line 7: {text}"
    );
    assert!(!text.contains("--rhythm"), "no flag vocabulary: {text}");
}

/// A two-bar MIDI whose meter changes 4/4 → 7/8 at bar 1 — the score-borne
/// `SWG0304` fixture, encoded independently of griff's own export path.
fn meter_change_midi() -> Vec<u8> {
    use midly::{
        num::{u15, u24, u28, u4, u7},
        Format, Header, MetaMessage, MidiMessage, Smf, Timing, TrackEvent, TrackEventKind,
    };
    let note = |delta: u32, key: u8, on: bool| TrackEvent {
        delta: u28::from_int_lossy(delta),
        kind: TrackEventKind::Midi {
            channel: u4::new(0),
            message: if on {
                MidiMessage::NoteOn {
                    key: u7::new(key),
                    vel: u7::new(90),
                }
            } else {
                MidiMessage::NoteOff {
                    key: u7::new(key),
                    vel: u7::new(0),
                }
            },
        },
    };
    let meta = |delta: u32, message: MetaMessage<'static>| TrackEvent {
        delta: u28::from_int_lossy(delta),
        kind: TrackEventKind::Meta(message),
    };
    let mut smf = Smf::new(Header {
        format: Format::SingleTrack,
        timing: Timing::Metrical(u15::new(480)),
    });
    smf.tracks = vec![vec![
        meta(0, MetaMessage::TimeSignature(4, 2, 24, 8)),
        meta(0, MetaMessage::Tempo(u24::from_int_lossy(500_000))),
        note(0, 40, true),
        note(480, 40, false),
        // Bar 1 begins at 1920 in a new meter: 7/8 = 1680 ticks.
        meta(1440, MetaMessage::TimeSignature(7, 3, 24, 8)),
        note(0, 42, true),
        note(480, 42, false),
        meta(1200, MetaMessage::EndOfTrack),
    ]];
    let mut bytes = Vec::new();
    smf.write_std(&mut bytes).expect("fixture must serialise");
    bytes
}

#[test]
fn swang_expand_locates_score_borne_facts_at_the_source_value() {
    // §1.5 via the spec's expand contract: a score-borne fact (the meter
    // changes inside the seed file) sits at the quoted source value's span
    // — line 9, column 16, the opening quote of the path literal. The
    // path identifies the offending score; the keyword never changes.
    let mid = env::temp_dir().join("griff_s16_swang_meter_change.mid");
    fs::write(&mid, meter_change_midi()).expect("fixture must write");
    let program = expand_program_for(
        "X.X/XX./.XX",
        "depth 1 max_cells 4096",
        "unit 1/16 tail rest_pad",
        2,
        &mid,
    );
    let path = script("expand_meter", &program);
    let text = griff(
        &["swang", "expand", path.to_str().unwrap()],
        Some(path.to_str().unwrap()),
    );
    fs::remove_file(&path).ok();
    fs::remove_file(&mid).ok();
    assert!(text.contains("exit: 1"), "{text}");
    assert!(text.contains("error[SWG0304]"), "{text}");
    assert!(
        text.contains("<OUT>:9:16"),
        "the opening quote of the source path literal: {text}"
    );
    assert!(
        !text.contains("INPUT") && !text.contains("--rhythm"),
        "no transport location classes in a program diagnostic: {text}"
    );
}

#[test]
fn swang_expand_speaks_program_vocabulary_for_the_silent_window() {
    // 17 cells rest-padded into 16-slot bars: template 0 is silent, and
    // with bars 1 the rotation never reaches the onset (#116's law). The
    // message speaks in program words — no retired flags.
    let program = expand_program(
        "................X",
        "depth 0 max_cells 32",
        "unit 1/16 tail rest_pad",
        1,
    );
    let path = script("expand_window", &program);
    let out = griff_raw(&["swang", "expand", path.to_str().unwrap()]);
    fs::remove_file(&path).ok();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("error[SWG0306]"), "{stderr}");
    assert!(stderr.contains("bars"), "{stderr}");
    assert!(
        !stderr.contains("--"),
        "no flag vocabulary in a program diagnostic: {stderr}"
    );
}

#[test]
fn swang_expand_speaks_program_vocabulary_for_the_rejected_tail() {
    // 9 slots into a 16-slot 4/4 bar under tail reject: SWG0302, advising
    // the program's own `rest_pad`, not the transport's `rest-pad`.
    let program = expand_program(
        "X.X/XX./.XX",
        "depth 0 max_cells 32",
        "unit 1/16 tail reject",
        8,
    );
    let path = script("expand_tail", &program);
    let out = griff_raw(&["swang", "expand", path.to_str().unwrap()]);
    fs::remove_file(&path).ok();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("error[SWG0302]"), "{stderr}");
    assert!(stderr.contains("rest_pad"), "{stderr}");
    assert!(
        !stderr.contains("rest-pad"),
        "no transport spelling: {stderr}"
    );
}

// ── build: law 5 (spec §3.5) ────────────────────────────────────────────────

/// A program around the strategy policy, exporting to `export`.
fn build_program(strategy: &str, export: &Path) -> String {
    let source = fixture_path("simple_4_4");
    format!(
        r#"swang 1

pattern p {{
    ascii "X.X/XX./.XX"
    |> fractalize depth 1 max_cells 4096 density 9500bps seed 4
    |> linearize snake
    |> map_rhythm unit 1/16 tail rest_pad
    |> generate {{
        source "{}"
        bars 4
        seed 42
        candidates 2
        strategy {strategy}
    }}
    |> export midi "{}"
}}
"#,
        source.display(),
        export.display()
    )
}

#[test]
fn swang_build_under_auto_matches_griff_generate_byte_for_byte() {
    // Law 5, the auto half: under `strategy auto` and the same seeds, build
    // produces the same result as the existing `griff generate`.
    let transport_out = env::temp_dir().join("griff_s16_swang_build_transport.mid");
    let src = fixture_path("simple_4_4");
    let transport = griff_raw(&[
        "generate",
        src.to_str().unwrap(),
        transport_out.to_str().unwrap(),
        "--bars",
        "4",
        "--seed",
        "42",
        "--candidates",
        "2",
        "--rhythm-kernel",
        "X.X/XX./.XX",
        "--rhythm-fractal-depth",
        "1",
        "--rhythm-density-bps",
        "9500",
        "--rhythm-seed",
        "4",
        "--rhythm-traversal",
        "snake",
        "--rhythm-unit",
        "1/16",
        "--rhythm-max-cells",
        "4096",
        "--rhythm-tail",
        "rest-pad",
    ]);
    assert!(
        transport.status.success(),
        "the transport command must succeed: {}",
        String::from_utf8_lossy(&transport.stderr)
    );
    let expected = fs::read(&transport_out).expect("the transport wrote its MIDI");
    fs::remove_file(&transport_out).ok();

    let export = env::temp_dir().join("griff_s16_swang_build_auto.mid");
    let path = script("build_auto", &build_program("auto", &export));
    let built = griff_raw(&["swang", "build", path.to_str().unwrap()]);
    fs::remove_file(&path).ok();
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );
    let bytes = fs::read(&export).expect("build wrote the program's export");
    fs::remove_file(&export).ok();
    assert_eq!(bytes, expected, "auto parity, byte for byte");
}

#[test]
fn swang_build_selects_the_named_strategy_and_is_deterministic() {
    // Law 5, the named half at the CLI: the run says which strategy was
    // selected, and the same program builds the same bytes twice. The
    // selection-only law itself is pinned at the core seam
    // (core/tests/strategy_selection.rs).
    let export = env::temp_dir().join("griff_s16_swang_build_named.mid");
    let path = script("build_named", &build_program("repeat_variation", &export));

    let first = griff_raw(&["swang", "build", path.to_str().unwrap()]);
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    assert!(
        String::from_utf8_lossy(&first.stdout).contains("RepeatVariation"),
        "the run names the selected strategy: {}",
        String::from_utf8_lossy(&first.stdout)
    );
    let first_bytes = fs::read(&export).expect("build wrote the program's export");

    let second = griff_raw(&["swang", "build", path.to_str().unwrap()]);
    assert!(second.status.success());
    let second_bytes = fs::read(&export).expect("second build wrote too");
    fs::remove_file(&path).ok();
    fs::remove_file(&export).ok();
    assert_eq!(first_bytes, second_bytes, "deterministic by law");
}

#[test]
fn swang_build_takes_no_output_flag() {
    // The program is the output's single owner (spec §3.2): build has no
    // output flag to offer, so clap refuses one.
    let out = griff_raw(&["swang", "build", "riff.swg", "--output", "elsewhere.mid"]);
    assert_eq!(
        out.status.code(),
        Some(2),
        "an output flag must be a usage error"
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
