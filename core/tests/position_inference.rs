//! Red → tests for ADR-0019 P1: fretboard position inference (candidate mapper
//! + monophonic local DP).
//!
//! Pins `Tuning::candidates` (pitch → playable `(string,fret)` under the tuning)
//! and `fretboard::infer_positions` (a small deterministic DP that minimises
//! hand movement / string changes, prefers open strings, and reports
//! out-of-range pitches as `None`). References API that does not exist yet, so
//! the suite fails to compile until the green step. Pure algorithm — no `Score`
//! mutation and no evidence wiring (that is P1b).

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::missing_assert_message,
    clippy::indexing_slicing,
    clippy::missing_const_for_fn
)]

use griff_core::{
    event::{FretboardPosition, Pitch, Tuning},
    fretboard::{infer_positions, measure_playability, FingeringWeights},
};

const MAX_FRET: u8 = 24;

fn pos(string: u8, fret: u8) -> FretboardPosition {
    FretboardPosition { string, fret }
}

#[test]
fn candidates_enumerate_every_playable_string() {
    // E4 (64) on Standard E: open high-E, plus one fret per lower string.
    let t = Tuning::standard_e();
    let got = t.candidates(Pitch(64), MAX_FRET);
    assert_eq!(
        got,
        vec![
            pos(1, 0),
            pos(2, 5),
            pos(3, 9),
            pos(4, 14),
            pos(5, 19),
            pos(6, 24),
        ],
        "candidates ordered by string, one fret per string within range",
    );
}

#[test]
fn candidates_empty_when_below_lowest_string() {
    // 30 is below the low-E open string (40): unplayable.
    let t = Tuning::standard_e();
    assert!(t.candidates(Pitch(30), MAX_FRET).is_empty());
}

#[test]
fn infer_prefers_open_low_strings() {
    // E2, A2, D3 all sit open on strings 6, 5, 4 — minimal movement, open bias.
    let t = Tuning::standard_e();
    let got = infer_positions(
        &[Pitch(40), Pitch(45), Pitch(50)],
        &t,
        &FingeringWeights::v1(),
        MAX_FRET,
    );
    assert_eq!(got, vec![Some(pos(6, 0)), Some(pos(5, 0)), Some(pos(4, 0))]);
}

#[test]
fn infer_stays_on_one_string_to_avoid_jumps() {
    // E4, F4, G4: cheaper to walk up the high-E string (0,1,3) than change strings.
    let t = Tuning::standard_e();
    let got = infer_positions(
        &[Pitch(64), Pitch(65), Pitch(67)],
        &t,
        &FingeringWeights::v1(),
        MAX_FRET,
    );
    assert_eq!(got, vec![Some(pos(1, 0)), Some(pos(1, 1)), Some(pos(1, 3))]);
}

#[test]
fn infer_reports_out_of_range_as_none_and_resets() {
    // The unplayable middle note is None; the run after it starts fresh.
    let t = Tuning::standard_e();
    let got = infer_positions(
        &[Pitch(64), Pitch(30), Pitch(67)],
        &t,
        &FingeringWeights::v1(),
        MAX_FRET,
    );
    assert_eq!(got, vec![Some(pos(1, 0)), None, Some(pos(1, 3))]);
}

// ── part playability (S13 backlog: the validator's per-part filter) ───────────
//
// TDD red phase: `measure_playability` — playability facts measured on the
// optimal fingering path (ADR-0019), the per-part filter the S13 pair
// validator and the S6 stage doc's "string/fret playability filter" call
// for, and the seed of the S7 `playability` / `fret_jump_penalty` cost
// terms. The verdict is reachability (every line note has a playable
// `(string, fret)` under the tuning); fret travel is reported as a fact,
// not a verdict — jump thresholds are calibration data, not code.
// References API that does not exist yet, so the suite fails until green.

#[test]
fn playability_clean_line_is_playable() {
    // E2, F#2, G2: each fits the low-E string only (frets 0, 2, 3), so the
    // optimal path is forced and the largest fret travel is 0 → 2.
    let report = measure_playability(
        &[Pitch(40), Pitch(42), Pitch(43)],
        &Tuning::standard_e(),
        &FingeringWeights::v1(),
        MAX_FRET,
    );
    assert_eq!(report.note_count, 3);
    assert_eq!(report.unpositionable, 0);
    assert!(report.is_playable());
    assert_eq!(report.max_fret_jump, 2);
}

#[test]
fn playability_counts_out_of_range_notes() {
    // 30 sits below the low E (40), 100 above the 24th fret of the high E
    // (88): neither has a playable string, and the part is not playable.
    let report = measure_playability(
        &[Pitch(40), Pitch(30), Pitch(100), Pitch(64)],
        &Tuning::standard_e(),
        &FingeringWeights::v1(),
        MAX_FRET,
    );
    assert_eq!(report.note_count, 4);
    assert_eq!(report.unpositionable, 2);
    assert!(!report.is_playable());
}

#[test]
fn playability_jump_resets_across_unplayable_gaps() {
    // An unplayable note resets the DP context (infer_positions), so fret
    // travel is not measured across the gap: 40 and 52 are never adjacent.
    let report = measure_playability(
        &[Pitch(40), Pitch(30), Pitch(52)],
        &Tuning::standard_e(),
        &FingeringWeights::v1(),
        MAX_FRET,
    );
    assert_eq!(report.unpositionable, 1);
    assert_eq!(
        report.max_fret_jump, 0,
        "no two consecutively positioned notes"
    );
}

#[test]
fn playability_of_an_empty_line_is_vacuous() {
    let report = measure_playability(
        &[],
        &Tuning::standard_e(),
        &FingeringWeights::v1(),
        MAX_FRET,
    );
    assert_eq!(report.note_count, 0);
    assert_eq!(report.unpositionable, 0);
    assert!(report.is_playable(), "an empty line is vacuously playable");
    assert_eq!(report.max_fret_jump, 0);
}
