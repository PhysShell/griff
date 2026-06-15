//! `import_score_auto` routes raw bytes to the Guitar Pro or MIDI adapter.

#![allow(clippy::expect_used, clippy::missing_assert_message)]

use griff_core::import::{import_score_auto, ImportError};

/// A committed MIDI fixture (shared with the CLI suite).
const SIMPLE_MID: &[u8] = include_bytes!("../../cli/tests/fixtures/simple_4_4.mid");

#[test]
fn imports_valid_midi() {
    let score = import_score_auto(SIMPLE_MID).expect("a valid MIDI file imports");
    assert!(!score.tracks.is_empty());
}

#[test]
fn routes_gp_header_to_gp_adapter() {
    // A GP5 header with no body: the Guitar Pro adapter is tried and fails, so
    // the error is a Guitar Pro error — not a silent fall-through to MIDI.
    let mut data = vec![30_u8];
    data.extend_from_slice(b"FICHIER GUITAR PRO v5.00      ");
    assert!(matches!(import_score_auto(&data), Err(ImportError::Gp(_))));
}

#[test]
fn non_gp_falls_back_to_midi() {
    // Neither Guitar Pro nor valid MIDI: Guitar Pro reports an unsupported
    // format, then the MIDI adapter is tried and fails.
    assert!(matches!(
        import_score_auto(b"definitely not a music file"),
        Err(ImportError::Midi(_))
    ));
}
