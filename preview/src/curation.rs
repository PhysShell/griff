//! Curation persistence: a viewport decision lands in a chunk record (S8).
//!
//! The bridge between the UI-level [`CurationDecision`] (interaction core,
//! ADR-0016 — no domain types there) and the S5 corpus schema.
//!
//! [`decide_record`] parses a `ChunkMeta` JSON record, sets its `reviewer`
//! field (`Approve` → `Accepted`, `Reject` → `Rejected`), and re-serializes.
//! Every other field passes through untouched — the schema's lossless
//! round-trip guarantee carries them — and an earlier decision may be
//! overwritten (re-curation is the established healing path). Pure; the
//! frontend shell owns the file I/O.

use griff_core::corpus::{ChunkMeta, ReviewerDecision};

use crate::viewport::CurationDecision;

/// Errors the curation persistence seam can emit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CurationError {
    /// The input is not a parseable `ChunkMeta` record.
    ParseFailed,
}

/// Applies `decision` to a serialized `ChunkMeta` record and returns the
/// updated JSON; everything except `reviewer` is untouched.
///
/// # Errors
/// [`CurationError::ParseFailed`] when `json` is not a `ChunkMeta` record.
pub fn decide_record(json: &str, decision: CurationDecision) -> Result<String, CurationError> {
    let mut meta: ChunkMeta = serde_json::from_str(json).map_err(|_| CurationError::ParseFailed)?;
    meta.reviewer = Some(match decision {
        CurationDecision::Approve => ReviewerDecision::Accepted,
        CurationDecision::Reject => ReviewerDecision::Rejected,
    });
    serde_json::to_string(&meta).map_err(|_| CurationError::ParseFailed)
}
