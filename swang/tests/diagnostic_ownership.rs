//! The four diagnostic locations spec §3.5 released, checked end to end.
//!
//! `source_map_contract.rs` proves the **map** holds the right spans. That
//! is only half of the claim. Between the map and a rendered diagnostic sits
//! `eval::flaw_to_diagnostic`, which decides *which* span each flaw class
//! points at — and nothing in the map's own suite can see a mistake there.
//! Swap two arms of that match and every source-map test stays green while
//! a unit error starts underlining the seed path.
//!
//! So these witnesses drive the evaluator, take the `EvalDiagnostic` it
//! actually produces, and slice the source with the span it actually chose:
//!
//! ```text
//! kernel-related -> the quoted `ascii` literal
//! unit           -> the `unit` value
//! tail           -> the `tail` value
//! score-borne    -> the quoted `source` value
//! ```
//!
//! SWG-INF-04 widened what the parser can locate. It deliberately did not
//! widen what these four point at, and this file is where that stays true.

// Reason: integration-test code. `unwrap`/`expect`/`panic` abort loudly with
// a clear message, which is exactly what a test harness wants.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::string_slice,
    clippy::missing_assert_message
)]

use griff_core::event::{Tempo, Ticks, TimeSignature};
use griff_core::score::{LossReport, MasterBar, RepeatMarker, Score};
use griff_core::slice::TickRange;
use griff_swang::eval::{compile_program, expand_program, DiagLocation};

// ── seed scores ─────────────────────────────────────────────────────────────

fn bar(index: u64, start: u32, end: u32, numerator: u8, denominator: u8) -> MasterBar {
    MasterBar {
        index,
        tick_range: TickRange::new(Ticks(start), Ticks(end)).expect("ordered range"),
        time_signature: TimeSignature::new(numerator, denominator).expect("a meter"),
        tempo: Tempo::from_bpm_integer(120).expect("a positive integer BPM"),
        repeat: RepeatMarker::default(),
    }
}

fn score(ppqn: u16, master_bars: Vec<MasterBar>) -> Score {
    Score {
        ticks_per_quarter: ppqn,
        master_bars,
        tracks: Vec::new(),
        source_meta: None,
        loss: LossReport::new(),
    }
}

/// Two 4/4 bars at PPQN 480 — a 1920-tick bar, which a 1/16 unit divides.
fn four_four() -> Score {
    score(480, vec![bar(0, 0, 1920, 4, 4), bar(1, 1920, 3840, 4, 4)])
}

/// 7/8 at PPQN 480 is a 1680-tick bar, which a 1/4 unit (480) does not
/// divide — the shape the CLI suite already uses for `SWG0301`.
fn seven_eight() -> Score {
    score(480, vec![bar(0, 0, 1680, 7, 8), bar(1, 1680, 3360, 7, 8)])
}

/// A meter change: score-borne, and the fix lives in the seed file rather
/// than anywhere in the program.
fn meter_change() -> Score {
    score(480, vec![bar(0, 0, 1920, 4, 4), bar(1, 1920, 3600, 7, 8)])
}

// ── programs ────────────────────────────────────────────────────────────────

fn program(kernel: &str, fractalize: &str, unit: &str, tail: &str, bars: u64) -> String {
    format!(
        "swang 1\n\npattern p {{\n    ascii \"{kernel}\"\n    |> fractalize {fractalize}\n    \
         |> linearize snake\n    |> map_rhythm unit {unit} tail {tail}\n    |> generate {{\n        \
         source \"seed.gp5\"\n        bars {bars}\n        seed 42\n        candidates 2\n        \
         strategy auto\n    }}\n    |> export midi \"out.mid\"\n}}\n"
    )
}

/// The one diagnostic an expansion refused with, and the source it slices.
fn refusal(source: &str, seed: &Score) -> (&'static str, String) {
    let compiled = compile_program(source).expect("the program parses");
    let flaws = expand_program(&compiled, seed).expect_err("this expansion must refuse");
    let first = flaws.first().expect("a refusal carries a diagnostic");
    let DiagLocation::Span(span) = &first.location else {
        panic!("expected a span location, got {:?}", first.location);
    };
    (
        first.code,
        source[span.start as usize..span.end as usize].to_owned(),
    )
}

// ── the four released owners ────────────────────────────────────────────────

#[test]
fn a_score_borne_fact_points_at_the_quoted_source_value() {
    // The meter changes inside the seed file. Nothing in the program is
    // wrong; the location names the score that is.
    let source = program(
        "X.X/XX./.XX",
        "depth 1 max_cells 4096",
        "1/16",
        "rest_pad",
        8,
    );
    let (code, sliced) = refusal(&source, &meter_change());
    assert_eq!(code, "SWG0304");
    assert_eq!(
        sliced, "\"seed.gp5\"",
        "score-borne facts sit at the `source` value, quotes included"
    );
}

#[test]
fn a_unit_that_does_not_divide_the_bar_points_at_the_unit_value() {
    // 1/4 is 480 ticks; the 7/8 bar is 1680, which 480 does not divide.
    let source = program(
        "X.X/XX./.XX",
        "depth 1 max_cells 4096",
        "1/4",
        "rest_pad",
        8,
    );
    let (code, sliced) = refusal(&source, &seven_eight());
    assert_eq!(code, "SWG0301");
    assert_eq!(sliced, "1/4", "the unit value, not the `unit` word");
}

#[test]
fn an_incomplete_final_bar_points_at_the_tail_value() {
    // Nine cells into sixteen-slot bars, with a tail that refuses to pad.
    let source = program("X.X/XX./.XX", "depth 0 max_cells 32", "1/16", "reject", 8);
    let (code, sliced) = refusal(&source, &four_four());
    assert_eq!(code, "SWG0302");
    assert_eq!(
        sliced, "reject",
        "the tail value is what the author must change"
    );
}

#[test]
fn a_silent_expansion_points_at_the_kernel_literal() {
    // One onset, seventeen cells, one bar: the window never reaches it.
    let source = program(
        "................X",
        "depth 0 max_cells 32",
        "1/16",
        "rest_pad",
        1,
    );
    let (code, sliced) = refusal(&source, &four_four());
    assert_eq!(code, "SWG0306");
    assert_eq!(
        sliced, "\"................X\"",
        "kernel-related facts sit at the quoted literal, quotes included"
    );
}

// ── the ownership really is four *different* places ─────────────────────────

#[test]
fn the_four_owners_are_four_distinct_locations() {
    // The point of the file, stated as one assertion: swapping any two arms
    // of the evaluator's match makes two of these collide. A witness that
    // only checked "some span came back" would not notice.
    let mut sliced = vec![
        refusal(
            &program(
                "X.X/XX./.XX",
                "depth 1 max_cells 4096",
                "1/16",
                "rest_pad",
                8,
            ),
            &meter_change(),
        )
        .1,
        refusal(
            &program(
                "X.X/XX./.XX",
                "depth 1 max_cells 4096",
                "1/4",
                "rest_pad",
                8,
            ),
            &seven_eight(),
        )
        .1,
        refusal(
            &program("X.X/XX./.XX", "depth 0 max_cells 32", "1/16", "reject", 8),
            &four_four(),
        )
        .1,
        refusal(
            &program(
                "................X",
                "depth 0 max_cells 32",
                "1/16",
                "rest_pad",
                1,
            ),
            &four_four(),
        )
        .1,
    ];
    let before = sliced.len();
    sliced.sort();
    sliced.dedup();
    assert_eq!(
        sliced.len(),
        before,
        "two flaw classes resolved to the same word: {sliced:?}"
    );
}
