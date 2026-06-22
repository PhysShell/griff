//! Corpus manifest assembly (`griff manifest`): fold curated chunks into a
//! versioned [`CorpusManifest`].
//!
//! The shared seam the cockpit's in-wasm manifest build and the CLI both use, so
//! a phone-built manifest and a desktop one are byte-identical (ADR-0027 §3).

use griff_core::corpus::{ChunkMeta, CorpusManifest, SCHEMA_VERSION};

/// Folds `chunks` into a [`CorpusManifest`] at the current [`SCHEMA_VERSION`],
/// sorted by id so the output is deterministic (matching the CLI's `manifest`).
#[must_use]
pub fn build_manifest(mut chunks: Vec<ChunkMeta>) -> CorpusManifest {
    chunks.sort_by(|a, b| a.id.0.cmp(&b.id.0));
    CorpusManifest {
        schema_version: SCHEMA_VERSION,
        chunks,
        groups: Vec::new(),
    }
}

/// Deserializes a set of `*.chunk.json` strings and folds them into a manifest —
/// the in-wasm equivalent of `griff manifest` over an OPFS corpus tree.
///
/// # Errors
/// Returns a message if any string is not a valid [`ChunkMeta`].
pub fn manifest_from_jsons(jsons: &[String]) -> Result<CorpusManifest, String> {
    let mut chunks = Vec::with_capacity(jsons.len());
    for json in jsons {
        chunks.push(serde_json::from_str::<ChunkMeta>(json).map_err(|err| err.to_string())?);
    }
    Ok(build_manifest(chunks))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::indexing_slicing)]

    use super::*;
    use crate::capture::{build_chunk, CaptureInputs};
    use griff_core::import::import_score_auto;

    fn chunk_json(id: &str) -> String {
        let score = import_score_auto(include_bytes!("../../cli/tests/fixtures/two_phrases.mid"))
            .expect("two_phrases.mid imports");
        let input = CaptureInputs { id, created_at: "t", updated_at: "t", ..CaptureInputs::default() };
        let chunk = build_chunk(&score, 0, &input).expect("builds a chunk");
        serde_json::to_string(&chunk).expect("serializes")
    }

    #[test]
    fn manifest_folds_and_sorts_chunks() {
        let manifest = manifest_from_jsons(&[chunk_json("zeta"), chunk_json("alpha")]).expect("folds");
        assert_eq!(manifest.schema_version, SCHEMA_VERSION);
        assert_eq!(manifest.chunks.len(), 2);
        assert_eq!(manifest.chunks[0].id.0, "alpha", "chunks are sorted by id");
        assert_eq!(manifest.chunks[1].id.0, "zeta");
        assert!(manifest.groups.is_empty(), "no ensemble groups from single-track capture");

        // Byte-compatible with what `griff` reads back.
        let json = serde_json::to_string(&manifest).expect("serializes");
        assert!(json.contains("\"schema_version\""));
    }

    #[test]
    fn manifest_from_jsons_rejects_a_bad_chunk() {
        manifest_from_jsons(&["not a chunk".to_owned()]).expect_err("garbage must not fold");
    }
}
