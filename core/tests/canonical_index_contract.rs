//! SWG-CORE-01, step B: the fixed-width contract.
//!
//! Red before the migration, and red by **failing to compile** — which is the
//! right failure mode for a type contract. `usize` and `u64` are distinct
//! types in Rust even where they have the same size, so these assertions
//! cannot be satisfied on any target while the fields are `usize`, and cannot
//! be accidentally satisfied on a 64-bit host once they are `u64`.
//!
//! Kept in its own file so that step A's characterization keeps compiling and
//! keeps passing through this commit. A red file that also holds the green
//! evidence would make "the characterization was green before and after"
//! unverifiable at exactly the moment it matters.
//!
//! Two claims, and they are different:
//!
//! 1. the three canonical index fields are `u64`;
//! 2. a value above `u32::MAX` is representable **on every target**, not just
//!    on a 64-bit host — the property step A could only assert behind a
//!    `cfg`, and the one H3 said the format did not have.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::missing_assert_message,
    clippy::indexing_slicing
)]

use griff_core::event::{Tempo, Ticks, TimeSignature};
use griff_core::score::{ImportWarning, MasterBar, RepeatMarker};
use griff_core::slice::TickRange;

/// Accepts a `u64` and nothing that merely happens to be the same size.
const fn require_u64(_: u64) {}

fn master_bar(index: u64) -> MasterBar {
    MasterBar {
        index,
        tick_range: TickRange::new(Ticks(0), Ticks(1920)).expect("ordered range"),
        time_signature: TimeSignature::new(4, 4).expect("4/4"),
        tempo: Tempo::from_bpm_integer(120).expect("120 BPM"),
        repeat: RepeatMarker::default(),
    }
}

// ── claim 1: the three fields are `u64` ────────────────────────────────────

#[test]
fn the_stored_bar_index_is_u64() {
    require_u64(master_bar(0).index);
}

#[test]
fn both_warning_payload_indices_are_u64() {
    let warnings = [
        ImportWarning::TrackNameInvalidUtf8 { track_index: 0 },
        ImportWarning::TempoApproximated {
            bar_index: 0,
            nearest_micros: 500_000,
        },
    ];
    for warning in warnings {
        match warning {
            ImportWarning::TrackNameInvalidUtf8 { track_index } => require_u64(track_index),
            ImportWarning::TempoApproximated { bar_index, .. } => require_u64(bar_index),
            ImportWarning::SmpteTimingUnsupported | ImportWarning::Other(_) => {
                panic!("no other variant is constructed here")
            }
        }
    }
}

/// `nearest_micros` is **not** an index and stays `u32`.
///
/// Stated because the migration passes right by it, and "widen the integers
/// near the ones we are widening" is the cheapest way for a mechanical change
/// to grow a scope it was never given.
#[test]
fn nearest_micros_is_left_alone() {
    const fn require_u32(_: u32) {}
    let ImportWarning::TempoApproximated { nearest_micros, .. } =
        (ImportWarning::TempoApproximated {
            bar_index: 0,
            nearest_micros: 500_000,
        })
    else {
        panic!("constructed variant")
    };
    require_u32(nearest_micros);
}

// ── claim 2: above `u32::MAX`, on every target ─────────────────────────────

/// No `cfg`. That absence is the deliverable.
#[test]
fn a_stored_bar_index_above_u32_max_is_representable_on_every_target() {
    let index: u64 = u64::from(u32::MAX) + 1;
    assert_eq!(master_bar(index).index, 4_294_967_296_u64);
}

/// No `cfg` here either, for the same reason.
#[test]
fn warning_payload_indices_above_u32_max_are_representable_on_every_target() {
    let beyond: u64 = u64::from(u32::MAX) + 1;

    let ImportWarning::TempoApproximated { bar_index, .. } = (ImportWarning::TempoApproximated {
        bar_index: beyond,
        nearest_micros: 500_000,
    }) else {
        panic!("constructed variant")
    };
    assert_eq!(bar_index, 4_294_967_296_u64);

    let ImportWarning::TrackNameInvalidUtf8 { track_index } =
        (ImportWarning::TrackNameInvalidUtf8 {
            track_index: beyond,
        })
    else {
        panic!("constructed variant")
    };
    assert_eq!(track_index, 4_294_967_296_u64);
}

// ── the ordinal → canonical conversion ─────────────────────────────────────

/// `index_from_ordinal` must widen, never narrow.
///
/// Added after the fact, because the migration's own falsification pass found
/// this hole: replacing the function's body with `(ordinal as u32) as u64`
/// left the entire suite green. Nothing else could catch it — reaching the
/// truncation through a real `Vec` needs four billion elements, which no test
/// can build.
///
/// Calling the conversion directly does reach it, because the input is a
/// `usize` rather than a collection. That is also why the test is gated: on a
/// 32-bit target the offending input does not exist, so there is nothing to
/// check and nothing to truncate.
#[cfg(target_pointer_width = "64")]
#[test]
fn the_ordinal_conversion_widens_rather_than_truncating() {
    use griff_core::score::index_from_ordinal;

    let beyond = usize::try_from(u64::from(u32::MAX) + 1).expect("64-bit host");
    assert_eq!(
        index_from_ordinal(beyond),
        4_294_967_296_u64,
        "an ordinal above u32::MAX must survive the widening whole"
    );
    assert_eq!(index_from_ordinal(0), 0_u64);
    assert_eq!(
        index_from_ordinal(usize::try_from(u32::MAX).expect("fits")),
        4_294_967_295_u64
    );
}
