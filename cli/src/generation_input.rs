//! Filesystem shell over the shared corpus→generation compiler.
//!
//! The musical compiler itself lives in [`griff_core::generation_input`] — one
//! implementation every frontend shares (the CLI here, the cockpit's Generate
//! panel, any experimental A/B harness), so none of them can drift from it.
//! What stays here is the part that is genuinely the CLI's: walking a corpus
//! *directory*, reading the records and their source tabs off disk, and handing
//! the parsed material to core. The web cockpit does the same over OPFS.
//!
//! **Experimental, `#[doc(hidden)]`**: a stability-exempt seam for tooling, not
//! a public library surface.

use std::fs;
use std::path::Path;

use griff_core::corpus::ChunkMeta;
use griff_core::generation_input::{corpus_material, prepare_chunk, LoadedChunk};
use griff_core::import;

pub use griff_core::generation_input::{
    bar_rhythms, generation_request_from_score, gesture_control_from_chunks, CorpusMaterial,
    GenerationInputError,
};

/// Loads every `*.chunk.json` record in `dir` with its source tab.
///
/// Records are sorted by name (deterministic result — the order decides the
/// rhythm-template palette); each source tab is expected next to it under
/// `source.filename`, sliced to the record's `bar_range` provenance when one is
/// present. Group records (`*.group.json`) are curation metadata, not chunks,
/// and are ignored.
///
/// # Errors
/// [`GenerationInputError::Corpus`] when `dir` cannot be read.
pub fn load_corpus_material(dir: &Path) -> Result<CorpusMaterial, GenerationInputError> {
    let entries = fs::read_dir(dir).map_err(|e| {
        GenerationInputError::Corpus(format!("cannot read corpus dir {}: {e}", dir.display()))
    })?;
    let mut record_names: Vec<String> = entries
        .filter_map(Result::ok)
        .filter_map(|e| e.file_name().to_str().map(ToOwned::to_owned))
        .filter(|n| n.ends_with(".chunk.json"))
        .collect();
    record_names.sort_unstable();

    let mut loaded = Vec::new();
    let mut skipped = Vec::new();
    for name in record_names {
        match load_chunk(dir, &name) {
            Some(chunk) => loaded.push(chunk),
            None => skipped.push(name),
        }
    }
    Ok(corpus_material(loaded, skipped))
}

/// Reads one chunk record and its source tab off disk, then prepares it through
/// core. `None` when the record does not parse, the source is
/// missing/unimportable, or the prepared slice carries no sounding track — the
/// caller reports the record as skipped.
fn load_chunk(dir: &Path, record_name: &str) -> Option<LoadedChunk> {
    let meta: ChunkMeta =
        serde_json::from_str(&fs::read_to_string(dir.join(record_name)).ok()?).ok()?;
    let source =
        import::import_score_auto(&fs::read(dir.join(&meta.source.filename)).ok()?).ok()?;
    prepare_chunk(meta, &source)
}
