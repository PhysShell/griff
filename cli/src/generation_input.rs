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

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use griff_core::corpus::{source_sha256, ChunkMeta};
use griff_core::generation_input::{corpus_material, prepare_chunk, LoadedChunk};
use griff_core::import;
use griff_core::score::Score;

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
    load_corpus_material_with(dir, |bytes| import::import_score_auto(bytes).ok())
}

/// The load loop with the source importer injected, so a test can count how
/// often a source is parsed. The corpus is ~25x redundant (≈400 sources behind
/// ≈9,900 chunks), so parsing each tab once instead of once per chunk is the
/// difference between a ~112 s and a ~4.5 s full load — measured, not guessed.
fn load_corpus_material_with(
    dir: &Path,
    mut import: impl FnMut(&[u8]) -> Option<Score>,
) -> Result<CorpusMaterial, GenerationInputError> {
    let entries = fs::read_dir(dir).map_err(|e| {
        GenerationInputError::Corpus(format!("cannot read corpus dir {}: {e}", dir.display()))
    })?;
    let mut record_names: Vec<String> = entries
        .filter_map(Result::ok)
        .filter_map(|e| e.file_name().to_str().map(ToOwned::to_owned))
        .filter(|n| n.ends_with(".chunk.json"))
        .collect();
    record_names.sort_unstable();

    let mut cache: HashMap<String, Score> = HashMap::new();
    let mut loaded = Vec::new();
    let mut skipped = Vec::new();
    for name in record_names {
        match load_chunk(dir, &name, &mut cache, &mut import) {
            Some(chunk) => loaded.push(chunk),
            None => skipped.push(name),
        }
    }
    Ok(corpus_material(loaded, skipped))
}

/// Reads one chunk record and prepares it through core, importing its source
/// tab (or reusing an already-parsed one from `cache`). `None` when the record
/// does not parse, the source is missing/unimportable/hash-mismatched, or the
/// prepared slice carries no sounding track — the caller reports it as skipped.
fn load_chunk(
    dir: &Path,
    record_name: &str,
    cache: &mut HashMap<String, Score>,
    import: &mut impl FnMut(&[u8]) -> Option<Score>,
) -> Option<LoadedChunk> {
    let meta: ChunkMeta =
        serde_json::from_str(&fs::read_to_string(dir.join(record_name)).ok()?).ok()?;
    // Key the parsed source by its content hash (v9) — falling back to the
    // filename for pre-v9 records — so every chunk of one tab reuses a single
    // parse. Determinism is unchanged: `prepare_chunk` slices an immutable
    // `&Score`, so a shared parse yields exactly the per-chunk-parse result.
    let key = meta
        .source
        .sha256
        .clone()
        .unwrap_or_else(|| meta.source.filename.clone());
    if !cache.contains_key(&key) {
        let bytes = fs::read(dir.join(&meta.source.filename)).ok()?;
        // A filename is not an identity: when the record pins the source's hash
        // (schema v9), a same-named but different file must not silently supply
        // the notes. A mismatch is a load failure — and a cache miss, so the
        // check runs for every distinct expected hash, never bypassed by reuse.
        if let Some(expected) = &meta.source.sha256 {
            if &source_sha256(&bytes) != expected {
                return None;
            }
        }
        cache.insert(key.clone(), import(&bytes)?);
    }
    prepare_chunk(meta, cache.get(&key)?)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::{import, load_corpus_material_with, source_sha256, Score};
    use griff_core::corpus::{ChunkId, ChunkMeta, SourceFormat, SourceRef};
    use griff_core::event::{NoteMarks, Pitch, Tempo, Ticks, TimeSignature, Tuning, Velocity};
    use griff_core::midi;
    use griff_core::score::{
        AtomEvent, AtomNote, EventGroup, EventGroupKind, LossReport, MasterBar, RepeatMarker,
        Track, Voice,
    };
    use griff_core::slice::TickRange;
    use std::cell::Cell;
    use std::{env, fs, process};

    fn note(start: u32, pitch: u8) -> AtomEvent {
        AtomEvent::Note(AtomNote {
            absolute_start: Ticks(start),
            duration: Ticks(480),
            pitch: Pitch::new(pitch).unwrap(),
            velocity: Velocity::new(90).unwrap(),
            marks: NoteMarks::empty(),
            position: None,
        })
    }

    fn two_bar_source() -> Score {
        let master_bars = (0..2usize)
            .map(|i| {
                let start = u32::try_from(i).unwrap().saturating_mul(1920);
                MasterBar {
                    index: i,
                    tick_range: TickRange::new(Ticks(start), Ticks(start.saturating_add(1920)))
                        .unwrap(),
                    time_signature: TimeSignature {
                        numerator: 4,
                        denominator: 4,
                    },
                    tempo: Tempo::new(120.0).unwrap(),
                    repeat: RepeatMarker::default(),
                }
            })
            .collect();
        let atoms = [
            note(0, 40),
            note(480, 43),
            note(960, 45),
            note(1440, 47),
            note(1920, 50),
            note(2400, 47),
            note(2880, 45),
            note(3360, 43),
        ];
        Score {
            ticks_per_quarter: 480,
            master_bars,
            tracks: vec![Track {
                name: None,
                channel: 0,
                voices: vec![Voice {
                    id: 0,
                    event_groups: atoms
                        .into_iter()
                        .map(|a| EventGroup {
                            kind: EventGroupKind::Single,
                            atoms: vec![a],
                            technique_spans: Vec::new(),
                        })
                        .collect(),
                }],
                tuning: Tuning::standard_e(),
            }],
            source_meta: None,
            loss: LossReport::new(),
        }
    }

    fn chunk_meta(id: &str, sha: &str, bars: (u32, u32)) -> ChunkMeta {
        ChunkMeta {
            id: ChunkId(id.to_owned()),
            title: String::new(),
            source: SourceRef {
                filename: "s.mid".to_owned(),
                format: SourceFormat::Midi,
                bar_range: Some(bars),
                track_index: Some(0),
                sha256: Some(sha.to_owned()),
            },
            tempo_bpm: 120.0,
            ticks_per_quarter: 480,
            time_signature: (4, 4),
            tuning: "standard_e".to_owned(),
            tags: Vec::new(),
            boundaries: Vec::new(),
            techniques: Vec::new(),
            quality_flags: Vec::new(),
            reviewer: None,
            structure: None,
            gesture: None,
            complexity: None,
            duplicate: None,
            style_cohort: None,
            ensemble: None,
            rights: None,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    #[test]
    fn one_source_is_parsed_once_for_all_its_chunks() {
        let dir = env::temp_dir().join(format!("griff_loadcache_{}", process::id()));
        fs::create_dir_all(&dir).unwrap();
        let bytes = midi::export_score(&two_bar_source()).unwrap();
        let sha = source_sha256(&bytes);
        fs::write(dir.join("s.mid"), &bytes).unwrap();
        for (id, bars) in [("a", (0_u32, 0_u32)), ("b", (1, 1))] {
            fs::write(
                dir.join(format!("{id}.chunk.json")),
                serde_json::to_string(&chunk_meta(id, &sha, bars)).unwrap(),
            )
            .unwrap();
        }

        let imports = Cell::new(0_usize);
        let material = load_corpus_material_with(&dir, |b| {
            imports.set(imports.get().saturating_add(1));
            import::import_score_auto(b).ok()
        })
        .expect("corpus loads");

        assert_eq!(material.references.len(), 2, "both chunks load");
        assert_eq!(
            imports.get(),
            1,
            "the shared source is parsed once, not once per chunk"
        );

        fs::remove_dir_all(&dir).ok();
    }
}
