//! Format-sniffing score import.
//!
//! [`import_score_auto`] routes raw bytes to the Guitar Pro or MIDI adapter by
//! content, so the product accepts either a `.gp3/.gp4/.gp5/.gpx` tab or a
//! `.mid` file through one entry point.

use crate::{
    gp::{self, GpImportError},
    midi::{self, MidiError},
    score::Score,
};

/// Error from [`import_score_auto`]: which adapter was tried and how it failed.
#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    /// The bytes looked like Guitar Pro, but the Guitar Pro adapter failed.
    #[error("Guitar Pro import failed: {0}")]
    Gp(#[from] GpImportError),
    /// The bytes were not Guitar Pro, and the MIDI adapter failed.
    #[error("MIDI import failed: {0}")]
    Midi(#[from] MidiError),
}

/// Imports a [`Score`] from raw bytes, detecting Guitar Pro vs MIDI by content.
///
/// Guitar Pro is tried first (it has a recognisable header); anything its
/// detector rejects falls through to the MIDI importer. A Guitar Pro *parse*
/// failure surfaces as [`ImportError::Gp`] rather than being masked by the MIDI
/// fallback.
pub fn import_score_auto(data: &[u8]) -> Result<Score, ImportError> {
    match gp::import_gp_score(data) {
        Ok(score) => Ok(score),
        Err(GpImportError::UnsupportedFormat) => {
            midi::import_score(data).map_err(ImportError::Midi)
        }
        Err(other) => Err(ImportError::Gp(other)),
    }
}
