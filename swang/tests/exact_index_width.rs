//! SWG-CORE-01, step A: how the exact writer spells a stored bar index today.
//!
//! `swang/tests/exact_writer_transport.rs` already pins that the writer emits
//! the **stored** `MasterBar.index` rather than the vector position. What it
//! does not pin is the range: every index it uses is small, so the whole suite
//! would stay green if the field's width changed underneath it.
//!
//! These characterization tests close that gap before the `usize` → `u64`
//! migration, so "the writer's bytes did not move" is checked rather than
//! assumed. They must not be edited to make the migration pass.

// The allowances the repository's other test files take.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::missing_assert_message
)]

use griff_core::event::{Tempo, Ticks, TimeSignature};
use griff_core::score::{LossReport, MasterBar, RepeatMarker, Score};
use griff_core::slice::TickRange;
use griff_swang::exact::write_score;

fn score_with_index(index: u64) -> Score {
    Score {
        ticks_per_quarter: 480,
        master_bars: vec![MasterBar {
            index,
            tick_range: TickRange::new(Ticks(0), Ticks(1920)).expect("ordered range"),
            time_signature: TimeSignature::new(4, 4).expect("4/4"),
            tempo: Tempo::from_bpm_integer(120).expect("120 BPM"),
            repeat: RepeatMarker::default(),
        }],
        tracks: Vec::new(),
        source_meta: None,
        loss: LossReport::new(),
    }
}

fn write(index: u64) -> String {
    write_score(&score_with_index(index)).expect("inside the writer domain")
}

#[test]
fn a_small_index_is_written_as_plain_decimal() {
    assert!(write(0).contains("index 0\n"));
    assert!(write(42).contains("index 42\n"));
}

/// The upper end of a `u32`, written whole.
///
/// Nothing in the writer narrows here today, and nothing may start to.
#[test]
fn an_index_at_u32_max_is_written_whole() {
    let text = write(u64::from(u32::MAX));
    assert!(
        text.contains("index 4294967295\n"),
        "the stored index is written in full: {text:?}"
    );
}

/// Above `u32::MAX` — inhabited before the migration on a 64-bit host, and
/// the reason it targets `u64` rather than `u32`.
///
/// Written under `#[cfg(target_pointer_width = "64")]` while the field was
/// `usize`, because that is the only place this score could be built — the
/// portability claim H3 called false. SWG-CORE-01 removed the `cfg`'s reason
/// to exist and the `cfg` with it; the assertion is unchanged and now runs on
/// every target.
#[test]
fn an_index_above_u32_max_is_written_whole() {
    let index = u64::from(u32::MAX) + 1;
    let text = write(index);
    assert!(
        text.contains("index 4294967296\n"),
        "no truncation to 32 bits anywhere on the way out: {text:?}"
    );
}

/// The byte-golden the migration must not move.
#[test]
fn a_large_index_document_matches_its_canonical_bytes() {
    assert_eq!(
        write(u64::from(u32::MAX)),
        "\
swang 2

score {
    ppqn 480

    master_bar {
        index 4294967295
        ticks 0..1920
        meter 4/4
        tempo 120/1
    }
}
"
    );
}
