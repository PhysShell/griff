//! SWG-4A-03 contract tests: the exact-text writer's transport and master
//! timeline, committed failing before any implementation.
//!
//! Scope is deliberately the first writer slice only — header, `ppqn`, and
//! `master_bar` with its `ticks`, `meter`, `tempo`, and `repeat`. Tracks,
//! strings, and everything downstream belong to SWG-4A-04 and later, and a
//! score carrying them must be refused rather than half-written.
//!
//! Every expectation here is a decision the accepted census already made
//! (`docs/swang/exact-score-text.md`); none of them is invented by this
//! test. Where a test looks pedantic — `tempo 7/1`, a stored `index` that
//! disagrees with its position — it is guarding a rule that a previous
//! draft got wrong.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use griff_core::event::{Tempo, Ticks, TimeSignature};
use griff_core::score::{LossReport, MasterBar, RepeatMarker, Score};
use griff_core::slice::TickRange;
use griff_swang::exact::{write_score, ExactWriteError};

// ── fixtures ───────────────────────────────────────────────────────────────

fn range(start: u32, end: u32) -> TickRange {
    TickRange::new(Ticks(start), Ticks(end)).expect("ordered range")
}

fn bar(index: usize, start: u32, end: u32, tempo: Tempo) -> MasterBar {
    MasterBar {
        index,
        tick_range: range(start, end),
        time_signature: TimeSignature::new(4, 4).expect("4/4"),
        tempo,
        repeat: RepeatMarker::default(),
    }
}

fn bpm(n: u32) -> Tempo {
    Tempo::from_bpm_integer(n).expect("a positive integer BPM")
}

/// A score with transport only — the whole of this slice's domain.
fn transport(ppqn: u16, master_bars: Vec<MasterBar>) -> Score {
    Score {
        ticks_per_quarter: ppqn,
        master_bars,
        tracks: Vec::new(),
        source_meta: None,
        loss: LossReport::new(),
    }
}

fn write(score: &Score) -> String {
    write_score(score).expect("this score is inside the writer domain")
}

// ── the header and the empty document ──────────────────────────────────────

#[test]
fn the_header_is_exactly_the_level_two_line() {
    let text = write(&transport(960, Vec::new()));
    assert!(
        text.starts_with("swang 2\n"),
        "the document opens with the frozen §1.1 header at level 2: {text:?}"
    );
}

#[test]
fn an_empty_timeline_has_one_canonical_spelling() {
    let text = write(&transport(960, Vec::new()));
    // No master_bar blocks at all — §6.2 omits a structural repeated block
    // when it is empty, and omission is the only spelling of empty.
    assert!(
        !text.contains("master_bar"),
        "an empty timeline writes no master_bar block: {text:?}"
    );
    assert!(text.contains("ppqn 960"), "ppqn is a required word: {text:?}");
}

// ── line discipline ────────────────────────────────────────────────────────

#[test]
fn output_is_lf_only_with_exactly_one_trailing_newline() {
    let text = write(&transport(480, vec![bar(0, 0, 1920, bpm(120))]));
    assert!(!text.contains('\r'), "no CR anywhere: {text:?}");
    assert!(text.ends_with('\n'), "the document ends with LF");
    assert!(
        !text.ends_with("\n\n"),
        "exactly one trailing newline, not a blank line: {text:?}"
    );
}

#[test]
fn writing_is_deterministic_across_runs() {
    let score = transport(480, vec![bar(0, 0, 1920, bpm(120))]);
    assert_eq!(write(&score), write(&score), "two runs, identical bytes");
}

// ── the master bar ─────────────────────────────────────────────────────────

#[test]
fn the_stored_index_is_written_not_the_vector_position() {
    // H4: `master_bars[0].index` need not be 0, and the text carries the
    // stored value. Deriving it from the position would make a disagreeing
    // import unrepresentable — a silent normalization.
    let text = write(&transport(480, vec![bar(17, 0, 1920, bpm(120))]));
    assert!(
        text.contains("index 17"),
        "the stored index 17 is written, not the position 0: {text:?}"
    );
    assert!(
        !text.contains("index 0\n"),
        "the vector position must not appear as the index: {text:?}"
    );
}

#[test]
fn bars_are_written_in_vector_order() {
    let score = transport(
        480,
        vec![
            bar(2, 0, 1920, bpm(120)),
            bar(1, 1920, 3840, bpm(100)),
            bar(0, 3840, 5760, bpm(90)),
        ],
    );
    let text = write(&score);
    let positions: Vec<usize> = ["index 2", "index 1", "index 0"]
        .iter()
        .map(|needle| text.find(needle).expect("every bar is written"))
        .collect();
    assert!(
        positions.windows(2).all(|w| w[0] < w[1]),
        "vector order is semantics and is never sorted: {text:?}"
    );
}

#[test]
fn tick_ranges_and_meters_are_written_from_their_stored_values() {
    let mut only = bar(0, 480, 2400, bpm(120));
    only.time_signature = TimeSignature::new(7, 8).expect("7/8");
    let text = write(&transport(480, vec![only]));
    assert!(text.contains("ticks 480..2400"), "{text:?}");
    assert!(text.contains("meter 7/8"), "{text:?}");
}

// ── tempo: the H1 regression witnesses ─────────────────────────────────────

#[test]
fn an_integer_tempo_keeps_its_denominator() {
    // `7/1` is the witness that killed a draft builder: `60_000_000 % 7 != 0`,
    // so an algorithm testing divisibility before branching rejects a tempo
    // `from_bpm_integer` builds happily. The writer must spell it plainly.
    let text = write(&transport(480, vec![bar(0, 0, 1920, bpm(7))]));
    assert!(
        text.contains("tempo 7/1"),
        "an integer tempo writes its denominator: {text:?}"
    );
}

#[test]
fn a_rational_tempo_is_written_reduced() {
    // 100/7 BPM — reachable only through from_micros_per_quarter(4_200_000).
    let tempo = Tempo::from_micros_per_quarter(4_200_000).expect("100/7 BPM");
    assert_eq!(
        (tempo.bpm_numerator(), tempo.bpm_denominator()),
        (100, 7),
        "the fixture really is 100/7"
    );
    let text = write(&transport(480, vec![bar(0, 0, 1920, tempo)]));
    assert!(text.contains("tempo 100/7"), "{text:?}");
}

// ── repeat: omission only for the model's own default ──────────────────────

#[test]
fn a_default_repeat_marker_is_omitted() {
    let text = write(&transport(480, vec![bar(0, 0, 1920, bpm(120))]));
    assert!(
        !text.contains("repeat"),
        "RepeatMarker::default() is 'no repeat barline' and writes nothing: {text:?}"
    );
}

#[test]
fn a_degenerate_but_non_default_repeat_is_written() {
    // `play_count == 1` is degenerate — `closes()` is false — and still not
    // the default, so it must appear. Omitting it would lose a fact the
    // exact diff can see.
    let mut only = bar(0, 0, 1920, bpm(120));
    only.repeat = RepeatMarker {
        start: false,
        play_count: 1,
    };
    let text = write(&transport(480, vec![only]));
    assert!(text.contains("repeat"), "{text:?}");
    assert!(text.contains("play_count 1"), "{text:?}");
}

#[test]
fn an_opening_repeat_without_a_close_is_written() {
    let mut only = bar(0, 0, 1920, bpm(120));
    only.repeat = RepeatMarker {
        start: true,
        play_count: 0,
    };
    let text = write(&transport(480, vec![only]));
    assert!(text.contains("start true"), "{text:?}");
    assert!(text.contains("play_count 0"), "{text:?}");
}

// ── refusal happens before any byte is produced ────────────────────────────

#[test]
fn a_zero_ppqn_is_refused_not_written() {
    let err = write_score(&transport(0, Vec::new()))
        .expect_err("ppqn 0 violates InvalidTicksPerQuarter");
    assert!(
        matches!(err, ExactWriteError::OutsideWriterDomain { .. }),
        "the refusal names the writer domain, not a formatting problem: {err:?}"
    );
}

#[test]
fn a_non_power_of_two_meter_denominator_is_refused() {
    let mut only = bar(0, 0, 1920, bpm(120));
    // Bypasses TimeSignature::new deliberately — H5: the fields are public,
    // so the invariant is advisory and the writer must check it itself.
    only.time_signature = TimeSignature {
        numerator: 4,
        denominator: 3,
    };
    let err = write_score(&transport(480, vec![only])).expect_err("4/3 is not a valid meter");
    assert!(matches!(err, ExactWriteError::OutsideWriterDomain { .. }), "{err:?}");
}

#[test]
fn an_inverted_tick_range_is_refused() {
    let mut only = bar(0, 0, 1920, bpm(120));
    only.tick_range = TickRange {
        start: Ticks(1920),
        end: Ticks(0),
    };
    let err = write_score(&transport(480, vec![only])).expect_err("start > end");
    assert!(matches!(err, ExactWriteError::OutsideWriterDomain { .. }), "{err:?}");
}

#[test]
fn a_score_with_tracks_is_refused_as_not_yet_written() {
    // A build-stage limit, not a domain statement: tracks are SWG-4A-04.
    // The distinction matters — refusing them as "outside the writer domain"
    // would contradict the census, which says every inhabited state is
    // spellable.
    let mut score = transport(480, Vec::new());
    score.tracks.push(griff_core::score::Track {
        name: None,
        channel: 0,
        voices: Vec::new(),
        tuning: griff_core::event::Tuning::standard_e(),
    });
    let err = write_score(&score).expect_err("tracks are not in this slice");
    assert!(
        matches!(err, ExactWriteError::NotYetWritten { .. }),
        "a slice boundary is not a domain judgement: {err:?}"
    );
}

// ── mutation matrix: each transport fact is observable in the bytes ────────

#[test]
fn mutating_any_single_transport_fact_changes_the_text() {
    let base = transport(480, vec![bar(0, 0, 1920, bpm(120))]);
    let baseline = write(&base);

    let mut ppqn = base.clone();
    ppqn.ticks_per_quarter = 960;
    assert_ne!(write(&ppqn), baseline, "ppqn is observable");

    let mut index = base.clone();
    index.master_bars[0].index = 5;
    assert_ne!(write(&index), baseline, "the stored index is observable");

    let mut ticks = base.clone();
    ticks.master_bars[0].tick_range = range(0, 960);
    assert_ne!(write(&ticks), baseline, "the tick range is observable");

    let mut meter = base.clone();
    meter.master_bars[0].time_signature = TimeSignature::new(3, 4).expect("3/4");
    assert_ne!(write(&meter), baseline, "the meter is observable");

    let mut tempo = base.clone();
    tempo.master_bars[0].tempo = bpm(121);
    assert_ne!(write(&tempo), baseline, "the tempo is observable");

    let mut repeat = base.clone();
    repeat.master_bars[0].repeat = RepeatMarker {
        start: true,
        play_count: 2,
    };
    assert_ne!(write(&repeat), baseline, "the repeat marker is observable");
}
