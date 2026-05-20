//! Curation actions that persist into the S5 corpus schema (S8).

use std::{
    fs,
    io::{self, BufWriter, Write as _},
    path::Path,
};

use griff_core::corpus::{ChunkMeta, QualityFlag, SwancoreTag};
use griff_core::corpus::ReviewerDecision;
use thiserror::Error;

// ── error ─────────────────────────────────────────────────────────────────────

/// Error produced by curation persistence operations.
#[derive(Debug, Error)]
pub enum CurationError {
    /// An I/O error reading or writing the chunk file.
    #[error("IO error: {0}")]
    Io(#[from] io::Error),
    /// JSON serialisation or deserialisation failed.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

// ── actions ───────────────────────────────────────────────────────────────────

/// A single curation mutation applied to a [`ChunkMeta`].
#[derive(Debug, Clone)]
pub enum CurationAction {
    /// Accept the chunk for training / eval.
    Approve,
    /// Reject the chunk.
    Reject,
    /// Add a swancore tag (idempotent — no-op if already present).
    AddTag(SwancoreTag),
    /// Remove a swancore tag (no-op if absent).
    RemoveTag(SwancoreTag),
    /// Add a quality flag (idempotent — no-op if already present).
    AddQualityFlag(QualityFlag),
    /// Update the human-readable title of the chunk.
    SetTitle(String),
}

// ── apply ─────────────────────────────────────────────────────────────────────

/// Applies `action` to `chunk` in place.
pub fn apply_curation(chunk: &mut ChunkMeta, action: CurationAction) {
    match action {
        CurationAction::Approve => {
            chunk.reviewer = Some(ReviewerDecision::Accepted);
        }
        CurationAction::Reject => {
            chunk.reviewer = Some(ReviewerDecision::Rejected);
        }
        CurationAction::AddTag(tag) => {
            if !chunk.tags.contains(&tag) {
                chunk.tags.push(tag);
            }
        }
        CurationAction::RemoveTag(tag) => {
            chunk.tags.retain(|&t| t != tag);
        }
        CurationAction::AddQualityFlag(flag) => {
            if !chunk.quality_flags.contains(&flag) {
                chunk.quality_flags.push(flag);
            }
        }
        CurationAction::SetTitle(title) => {
            chunk.title = title;
        }
    }
}

// ── persistence ───────────────────────────────────────────────────────────────

/// Serialises `meta` as pretty-printed JSON to `path`, creating or overwriting
/// the file.
pub fn save_chunk_meta(path: &Path, meta: &ChunkMeta) -> Result<(), CurationError> {
    let file = fs::File::create(path)?;
    let mut writer = BufWriter::new(file);
    serde_json::to_writer_pretty(&mut writer, meta)?;
    writer.flush()?;
    Ok(())
}

/// Deserialises a [`ChunkMeta`] from the JSON file at `path`.
pub fn load_chunk_meta(path: &Path) -> Result<ChunkMeta, CurationError> {
    let data = fs::read(path)?;
    let meta = serde_json::from_slice(&data)?;
    Ok(meta)
}
