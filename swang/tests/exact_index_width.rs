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

fn score_with_index(index: usize) -> Score {
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

fn write(index: usize) -> String {
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
    let text = write(usize::try_from(u32::MAX).expect("u32::MAX fits every supported usize"));
    assert!(
        text.contains("index 4294967295\n"),
        "the stored index is written in full: {text:?}"
    );
}

/// Above `u32::MAX` — inhabited today on a 64-bit host, and the reason the
/// migration targets `u64` rather than `u32`.
///
/// The `cfg` is the defect SWG-CORE-01 removes: this score cannot be built at
/// all on a 32-bit target while the field is `usize`, which is exactly the
/// portability claim H3 called false. Recording it here means the migration
/// can be shown to have changed the field's *width* without changing the
/// writer's *bytes* for any value that already fit.
#[cfg(target_pointer_width = "64")]
#[test]
fn an_index_above_u32_max_is_written_whole() {
    let index = usize::try_from(u64::from(u32::MAX) + 1).expect("64-bit host");
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
        write(usize::try_from(u32::MAX).expect("u32::MAX fits every supported usize")),
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
