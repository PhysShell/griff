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

/// A UI-level digest of a chunk record's curation state.
///
/// Plain strings in the schema's `snake_case` wire names, so renderers show
/// exactly what the corpus stores and no domain type crosses into the UI
/// (ADR-0016).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordSummary {
    /// The record's `title`.
    pub title: String,
    /// The prior reviewer decision in wire casing, if any.
    pub reviewer: Option<String>,
    /// The record's tags in wire casing, in stored order.
    pub tags: Vec<String>,
}

/// Errors the curation persistence seam can emit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CurationError {
    /// The input is not a parseable `ChunkMeta` record.
    ParseFailed,
}

/// Digests a serialized `ChunkMeta` record into a [`RecordSummary`].
///
/// The inspector shows the digest: title, prior reviewer decision, and tags,
/// all in the schema's wire casing (via serde, so the names cannot drift).
///
/// # Errors
/// [`CurationError::ParseFailed`] when `json` is not a `ChunkMeta` record.
pub fn summarize_record(json: &str) -> Result<RecordSummary, CurationError> {
    let meta: ChunkMeta = serde_json::from_str(json).map_err(|_| CurationError::ParseFailed)?;
    Ok(RecordSummary {
        reviewer: meta
            .reviewer
            .and_then(|d| wire_name(serde_json::to_value(d))),
        tags: meta
            .tags
            .iter()
            .filter_map(|t| wire_name(serde_json::to_value(t)))
            .collect(),
        title: meta.title,
    })
}

/// A serialized value's wire name (`snake_case` per the schema), if it is a
/// plain string on the wire.
fn wire_name(value: serde_json::Result<serde_json::Value>) -> Option<String> {
    match value {
        Ok(serde_json::Value::String(s)) => Some(s),
        _ => None,
    }
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
