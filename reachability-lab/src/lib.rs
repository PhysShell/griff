//! Offline Reachability Lab — the holdout-filtering boundary (ADR-0032).
//!
//! Holdout must run while provenance still exists on `LoadedChunk`, **before**
//! [`corpus_material`] converts chunks into anonymous rhythm templates, novelty
//! references, and aggregate gesture data. This crate owns that boundary and the
//! mode/target/error vocabulary; production CLI, cockpit, generation, and
//! reranking acquire no holdout policy.
//!
//! Implemented (ADR-0032), fail-closed over a single-authority [`LoadedCorpus`]:
//! **song mode** (`HoldoutTargetSong`), **source-file mode**
//! (`HoldoutTargetSourceFile`, exact `sha256`), and **fragment mode**
//! (`HoldoutTargetFragment`, same source with inclusive `bar_range` overlap).
//! The two hash-based modes are independent of song curation. Any measurement
//! axes are separate later slices.

use griff_core::corpus::{
    song_holdout_preflight, ChunkId, ChunkMeta, CorpusManifest, SongHoldoutRefusal, SongId,
};
use griff_core::generation_input::{corpus_material, CorpusMaterial, LoadedChunk};
use std::collections::{BTreeMap, BTreeSet};

/// A corpus bound into one authority: the manifest (the `song_id` facts the
/// preflight trusts, plus the optional `songs` map) and the loaded records the
/// filter acts on, which must describe the **same** corpus (ADR-0032).
#[derive(Debug)]
pub struct LoadedCorpus {
    /// The authoritative manifest — every chunk, and the optional songs map.
    pub manifest: CorpusManifest,
    /// Records prepared for material construction.
    pub loaded: Vec<LoadedChunk>,
    /// Names that could not be loaded (reported, never silently dropped).
    pub skipped: Vec<String>,
}

/// Which experiment a run is. `NoCorpus` (no corpus at all) and
/// `LeakyDiagnostic` (a corpus deliberately supplied *unfiltered*) are distinct
/// experiments, and neither is a "no-holdout" alias for the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorpusMode {
    /// No corpus — a corpus-free run.
    NoCorpus,
    /// A corpus supplied deliberately unfiltered; explicitly named so it can
    /// never be mistaken for a holdout.
    LeakyDiagnostic,
    /// Exclude every source representation of the target song, fail-closed.
    HoldoutTargetSong,
    /// Exclude every chunk cut from the target source file (by exact `sha256`),
    /// fail-closed. Independent of song curation.
    HoldoutTargetSourceFile,
    /// Exclude every chunk from the target source (`sha256`) whose inclusive
    /// `bar_range` overlaps or contains the target range, fail-closed.
    /// Independent of song curation and of track.
    HoldoutTargetFragment,
}

/// The identity a holdout targets, with the field per mode kept explicit. Song
/// mode uses `song_id`; source-file mode uses `source_sha256`; fragment mode uses
/// `source_sha256` + `bar_range`. `track_index` is provenance only. Measurement
/// fields (`projection`, `eligibility`) join with their own later slices.
#[derive(Debug, Clone, Default)]
pub struct TargetIdentity {
    /// The source file to hold out, by content hash (required by
    /// `HoldoutTargetSourceFile` and `HoldoutTargetFragment`).
    pub source_sha256: Option<String>,
    /// The inclusive `(first, last)` fragment range within the source
    /// (`HoldoutTargetFragment`). `None` means the **whole source** — not a
    /// missing field.
    pub bar_range: Option<(u32, u32)>,
    /// Provenance only in fragment mode — it never narrows exclusion, since an
    /// overlapping chunk from another track can still leak the target.
    pub track_index: Option<u32>,
    /// The composition to hold out (required by `HoldoutTargetSong`).
    pub song_id: Option<SongId>,
}

/// A reason the boundary refuses a run, fail-closed (ADR-0032). Never a silent
/// pick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HoldoutError {
    /// The loaded records and the manifest do not describe the same corpus.
    CorpusBinding(Vec<BindingRefusal>),
    /// `song_holdout_preflight` refused the corpus.
    Preflight(Vec<SongHoldoutRefusal>),
    /// Song mode requires a target `song_id`, and none was supplied.
    MissingTargetSongId,
    /// The target `song_id` is carried by no *loaded* chunk, so no holdout would
    /// actually happen — returning the corpus unchanged would relabel an absent
    /// holdout as valid (e.g. the target's only source failed to load).
    TargetSongAbsent(SongId),
    /// Source-file mode requires a target `source_sha256`, and none was supplied.
    MissingTargetSourceSha256,
    /// The source-file preflight refused the corpus (a participating chunk had
    /// no `sha256`, so file identity is incomplete).
    SourcePreflight(Vec<SourceHoldoutRefusal>),
    /// The target `sha256` is carried by no *loaded* chunk, so no holdout would
    /// actually happen (e.g. the target file failed to load).
    TargetSourceAbsent(String),
    /// The target fragment range is malformed (`first_bar > last_bar`). Never
    /// silently swapped, treated as disjoint, or degraded to `TargetFragmentAbsent`.
    InvalidTargetBarRange { first_bar: u32, last_bar: u32 },
    /// The fragment preflight refused the corpus (a target-source manifest chunk
    /// carried a malformed range).
    FragmentPreflight(Vec<FragmentHoldoutRefusal>),
    /// No *loaded* chunk of the target source overlaps the target range, so no
    /// holdout would actually happen. `bar_range` `None` means whole-source.
    TargetFragmentAbsent {
        source_sha256: String,
        bar_range: Option<(u32, u32)>,
    },
}

/// A reason the source-file preflight refuses a corpus (ADR-0032): source-file
/// holdout is conditional on **complete** hash identity, with no basename
/// fallback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceHoldoutRefusal {
    /// A participating manifest chunk carries no `sha256`, so its source file
    /// cannot be identified by content.
    UnidentifiedSource(ChunkId),
}

/// A reason the fragment preflight refuses a corpus (ADR-0032).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FragmentHoldoutRefusal {
    /// A target-source manifest chunk carries a malformed range
    /// (`first_bar > last_bar`).
    InvalidBarRange {
        chunk_id: ChunkId,
        first_bar: u32,
        last_bar: u32,
    },
}

/// A single-authority violation between the manifest and the loaded records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BindingRefusal {
    /// The same `ChunkId` appears twice in `manifest.chunks`, so no single
    /// authoritative entry exists for a loaded record to bind to.
    DuplicateManifest(ChunkId),
    /// The same `ChunkId` appears twice among the loaded records.
    DuplicateLoaded(ChunkId),
    /// A loaded record whose `ChunkId` is not in the authoritative manifest.
    LoadedNotInManifest(ChunkId),
    /// A loaded record whose **full** `ChunkMeta` disagrees with the manifest
    /// chunk of the same `ChunkId` — the stale-manifest leak. Every
    /// material-defining field must match, not just `song_id` / `sha256`.
    ProvenanceMismatch(ChunkId),
}

/// Prepare a loaded corpus for a run mode, fail-closed (ADR-0032).
///
/// Returns `Ok(None)` for a genuinely corpus-free run (`NoCorpus`) and
/// `Ok(Some(_))` for a corpus-backed one. `LeakyDiagnostic` compiles the corpus
/// unfiltered. Each holdout mode binds the loaded records to the manifest (single
/// authority), runs its mode preflight, requires its target identity, excludes
/// the matching `LoadedChunk`s, and only then compiles the survivors — refusing
/// if nothing was excluded:
///
/// - `HoldoutTargetSong` — `song_holdout_preflight`; target `song_id`; exclude
///   every representation of the song.
/// - `HoldoutTargetSourceFile` — source-file preflight (complete `sha256`);
///   target `source_sha256`; exclude every chunk of that file.
/// - `HoldoutTargetFragment` — source-file + fragment preflight; target
///   `source_sha256` + `bar_range`; exclude every same-source chunk whose
///   inclusive range overlaps the target (`None` on either side = whole source;
///   `track_index` never narrows it).
///
/// The two hash-based modes do not run the song preflight and do not require
/// `song_id`.
///
/// # Errors
///
/// [`HoldoutError`], fail-closed, when a holdout mode cannot proceed:
/// - any mode: a single-authority binding violation
///   ([`HoldoutError::CorpusBinding`]);
/// - song: [`HoldoutError::Preflight`], [`HoldoutError::MissingTargetSongId`],
///   [`HoldoutError::TargetSongAbsent`];
/// - source-file: [`HoldoutError::SourcePreflight`],
///   [`HoldoutError::MissingTargetSourceSha256`],
///   [`HoldoutError::TargetSourceAbsent`];
/// - fragment: [`HoldoutError::SourcePreflight`],
///   [`HoldoutError::MissingTargetSourceSha256`],
///   [`HoldoutError::InvalidTargetBarRange`], [`HoldoutError::FragmentPreflight`],
///   [`HoldoutError::TargetFragmentAbsent`].
///
/// `NoCorpus` and `LeakyDiagnostic` never refuse.
pub fn prepare_corpus_for_mode(
    corpus: LoadedCorpus,
    mode: CorpusMode,
    target: &TargetIdentity,
) -> Result<Option<CorpusMaterial>, HoldoutError> {
    match mode {
        // A genuinely corpus-free run — no rhythms, references, or gesture.
        CorpusMode::NoCorpus => Ok(None),
        // A corpus deliberately supplied unfiltered; never a holdout.
        CorpusMode::LeakyDiagnostic => Ok(Some(corpus_material(corpus.loaded, corpus.skipped))),
        CorpusMode::HoldoutTargetSong => {
            // Single authority first: the records the filter acts on must be the
            // corpus the preflight validated.
            check_binding(&corpus)?;
            // Fail-closed over the whole manifest (coverage + identity + songs map).
            song_holdout_preflight(&corpus.manifest).map_err(HoldoutError::Preflight)?;
            let target_song = target
                .song_id
                .as_ref()
                .ok_or(HoldoutError::MissingTargetSongId)?;
            // Partition into the target's representations and the survivors, so a
            // holdout that excludes nothing is a refusal, not a silent success.
            let (excluded, kept): (Vec<LoadedChunk>, Vec<LoadedChunk>) = corpus
                .loaded
                .into_iter()
                .partition(|chunk| chunk.meta.source.song_id.as_ref() == Some(target_song));
            if excluded.is_empty() {
                return Err(HoldoutError::TargetSongAbsent(target_song.clone()));
            }
            // Provenance exclusion is primary and happens before material
            // construction: only the survivors are compiled.
            Ok(Some(corpus_material(kept, corpus.skipped)))
        }
        CorpusMode::HoldoutTargetSourceFile => {
            check_binding(&corpus)?;
            // File identity must be complete — no basename fallback, no
            // "mostly identified" run. This does NOT run the song preflight and
            // does NOT require song_id: a hash-identified but song-uncurated
            // corpus is valid for file-mode experiments.
            source_file_preflight(&corpus.manifest).map_err(HoldoutError::SourcePreflight)?;
            let target_sha = target
                .source_sha256
                .as_ref()
                .ok_or(HoldoutError::MissingTargetSourceSha256)?;
            let (excluded, kept): (Vec<LoadedChunk>, Vec<LoadedChunk>) = corpus
                .loaded
                .into_iter()
                .partition(|chunk| chunk.meta.source.sha256.as_ref() == Some(target_sha));
            if excluded.is_empty() {
                return Err(HoldoutError::TargetSourceAbsent(target_sha.clone()));
            }
            Ok(Some(corpus_material(kept, corpus.skipped)))
        }
        CorpusMode::HoldoutTargetFragment => {
            check_binding(&corpus)?;
            source_file_preflight(&corpus.manifest).map_err(HoldoutError::SourcePreflight)?;
            let target_sha = target
                .source_sha256
                .as_ref()
                .ok_or(HoldoutError::MissingTargetSourceSha256)?;
            // A malformed target range is a typed refusal — never swapped,
            // treated as disjoint, or degraded to TargetFragmentAbsent.
            if let Some((first, last)) = target.bar_range {
                if first > last {
                    return Err(HoldoutError::InvalidTargetBarRange {
                        first_bar: first,
                        last_bar: last,
                    });
                }
            }
            fragment_preflight(&corpus.manifest, target_sha)
                .map_err(HoldoutError::FragmentPreflight)?;
            // Exclude by exact sha256 AND inclusive range overlap; track_index is
            // provenance only and never narrows the exclusion.
            let (excluded, kept): (Vec<LoadedChunk>, Vec<LoadedChunk>) =
                corpus.loaded.into_iter().partition(|chunk| {
                    chunk.meta.source.sha256.as_ref() == Some(target_sha)
                        && ranges_overlap(target.bar_range, chunk.meta.source.bar_range)
                });
            if excluded.is_empty() {
                return Err(HoldoutError::TargetFragmentAbsent {
                    source_sha256: target_sha.clone(),
                    bar_range: target.bar_range,
                });
            }
            Ok(Some(corpus_material(kept, corpus.skipped)))
        }
    }
}

/// Inclusive range-overlap for holdout, applied **only after** exact `sha256`
/// equality. Either side being `None` means the whole source and overlaps any
/// range from that source; different source hashes never reach here.
fn ranges_overlap(target: Option<(u32, u32)>, chunk: Option<(u32, u32)>) -> bool {
    match (target, chunk) {
        (None, _) | (_, None) => true,
        (Some((target_first, target_last)), Some((chunk_first, chunk_last))) => {
            target_first <= chunk_last && chunk_first <= target_last
        }
    }
}

/// Fail-closed fragment preflight: a manifest chunk of the target source whose
/// range is malformed (`first > last`) is refused before any range filtering, so
/// a bad range can never silently exclude nothing. `bar_range` `None` is valid
/// (whole-source) and never refused.
fn fragment_preflight(
    manifest: &CorpusManifest,
    target_sha: &str,
) -> Result<(), Vec<FragmentHoldoutRefusal>> {
    let refusals: Vec<FragmentHoldoutRefusal> = manifest
        .chunks
        .iter()
        .filter(|chunk| chunk.source.sha256.as_deref() == Some(target_sha))
        .filter_map(|chunk| match chunk.source.bar_range {
            Some((first, last)) if first > last => Some(FragmentHoldoutRefusal::InvalidBarRange {
                chunk_id: chunk.id.clone(),
                first_bar: first,
                last_bar: last,
            }),
            _ => None,
        })
        .collect();
    if refusals.is_empty() {
        Ok(())
    } else {
        Err(refusals)
    }
}

/// Fail-closed source-file preflight: every participating manifest chunk must
/// carry a `sha256`, so file identity is complete across the corpus (ADR-0032 /
/// audit §3). A missing hash is refused, never trusted by basename.
fn source_file_preflight(manifest: &CorpusManifest) -> Result<(), Vec<SourceHoldoutRefusal>> {
    let refusals: Vec<SourceHoldoutRefusal> = manifest
        .chunks
        .iter()
        .filter(|chunk| chunk.source.sha256.is_none())
        .map(|chunk| SourceHoldoutRefusal::UnidentifiedSource(chunk.id.clone()))
        .collect();
    if refusals.is_empty() {
        Ok(())
    } else {
        Err(refusals)
    }
}

/// Enforce the ADR-0032 single-authority invariant: every loaded record maps by
/// `ChunkId` to exactly one manifest chunk whose **full** `ChunkMeta` matches it,
/// no `ChunkId` is duplicated in the manifest, and none is loaded twice.
/// Otherwise the preflight could validate one dataset while the filter executes
/// another.
fn check_binding(corpus: &LoadedCorpus) -> Result<(), HoldoutError> {
    let mut refusals = Vec::new();
    // Index the manifest, collecting (not silently overwriting) duplicate ids —
    // a duplicated ChunkId means no single authoritative entry.
    let mut by_id: BTreeMap<&ChunkId, &ChunkMeta> = BTreeMap::new();
    for chunk in &corpus.manifest.chunks {
        if by_id.insert(&chunk.id, chunk).is_some() {
            refusals.push(BindingRefusal::DuplicateManifest(chunk.id.clone()));
        }
    }
    let mut seen: BTreeSet<&ChunkId> = BTreeSet::new();
    for chunk in &corpus.loaded {
        let id = &chunk.meta.id;
        if !seen.insert(id) {
            refusals.push(BindingRefusal::DuplicateLoaded(id.clone()));
            continue;
        }
        match by_id.get(id) {
            None => refusals.push(BindingRefusal::LoadedNotInManifest(id.clone())),
            // Full metadata must match — not a hand-picked two-field imitation of
            // equality — so no material-defining field can diverge unseen.
            Some(manifest_chunk) if **manifest_chunk != chunk.meta => {
                refusals.push(BindingRefusal::ProvenanceMismatch(id.clone()));
            }
            Some(_) => {}
        }
    }
    if refusals.is_empty() {
        Ok(())
    } else {
        Err(HoldoutError::CorpusBinding(refusals))
    }
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::expect_used, clippy::missing_panics_doc)]
mod tests {
    use super::*;
    use griff_core::corpus::{
        ChunkMeta, CorpusManifest, QualityFlag, SongId, SourceFormat, SourceRef, SCHEMA_VERSION,
    };
    use griff_core::event::{NoteMarks, Pitch, Tempo, Ticks, TimeSignature, Tuning, Velocity};
    use griff_core::generation_input::corpus_material;
    use griff_core::gesture::GestureStats;
    use griff_core::score::{
        AtomEvent, AtomNote, EventGroup, EventGroupKind, LossReport, MasterBar, RepeatMarker,
        Score, Track, Voice,
    };
    use griff_core::slice::TickRange;
    use std::collections::BTreeMap;

    fn note(start: u32, pitch: u8) -> AtomEvent {
        AtomEvent::Note(AtomNote {
            absolute_start: Ticks(start),
            duration: Ticks(480),
            pitch: Pitch::new(pitch).expect("valid pitch"),
            velocity: Velocity::new(90).expect("valid velocity"),
            marks: NoteMarks::empty(),
            position: None,
        })
    }

    /// One 4/4 bar (480 ppq) with a single note at `onset` — a distinct onset or
    /// pitch yields a distinct rhythm template and a distinct sliced score.
    fn one_bar_score(onset: u32, pitch: u8) -> Score {
        Score {
            ticks_per_quarter: 480,
            master_bars: vec![MasterBar {
                index: 0,
                tick_range: TickRange::new(Ticks(0), Ticks(1920)).expect("ordered"),
                time_signature: TimeSignature {
                    numerator: 4,
                    denominator: 4,
                },
                tempo: Tempo::from_bpm_integer(120).expect("120 BPM"),
                repeat: RepeatMarker::default(),
            }],
            tracks: vec![Track {
                name: None,
                channel: 0,
                voices: vec![Voice {
                    id: 0,
                    event_groups: vec![EventGroup {
                        kind: EventGroupKind::Single,
                        atoms: vec![note(onset, pitch)],
                        technique_spans: Vec::new(),
                    }],
                }],
                tuning: Tuning::standard_e(),
            }],
            source_meta: None,
            loss: LossReport::new(),
        }
    }

    fn sample_gesture() -> GestureStats {
        GestureStats {
            note_count: 4,
            burst_count: 1,
            mean_burst_notes: 4.0,
            max_burst_notes: 4,
            rest_count: 0,
            mean_rest_quarters: 0.0,
            rest_on_grid_share: 1.0,
            modal_landing_share: 0.5,
            mean_final_lengthening: 0.5,
        }
    }

    fn meta(id: &str, sha: Option<&str>, song: Option<&str>, gesture: bool) -> ChunkMeta {
        ChunkMeta {
            id: ChunkId(id.to_owned()),
            title: id.to_owned(),
            source: SourceRef {
                filename: format!("{id}.gp5"),
                format: SourceFormat::Gp5,
                bar_range: Some((0, 0)),
                track_index: Some(0),
                sha256: sha.map(ToOwned::to_owned),
                song_id: song.map(|s| SongId(s.to_owned())),
            },
            tempo_bpm: 120.0,
            ticks_per_quarter: 480,
            time_signature: (4, 4),
            tuning: "standard_e".to_owned(),
            tags: Vec::new(),
            boundaries: Vec::new(),
            techniques: Vec::new(),
            quality_flags: vec![QualityFlag::Clean],
            reviewer: None,
            structure: None,
            gesture: if gesture {
                Some(sample_gesture())
            } else {
                None
            },
            complexity: None,
            duplicate: None,
            style_cohort: None,
            ensemble: None,
            rights: None,
            created_at: "2026-07-29T00:00:00Z".to_owned(),
            updated_at: "2026-07-29T00:00:00Z".to_owned(),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn loaded(
        id: &str,
        sha: &str,
        song: &str,
        onset: u32,
        pitch: u8,
        gesture: bool,
    ) -> LoadedChunk {
        LoadedChunk {
            meta: meta(id, Some(sha), Some(song), gesture),
            sliced: one_bar_score(onset, pitch),
            track: 0,
        }
    }

    fn manifest_of(metas: Vec<ChunkMeta>) -> CorpusManifest {
        CorpusManifest {
            schema_version: SCHEMA_VERSION,
            chunks: metas,
            groups: Vec::new(),
            songs: None,
        }
    }

    fn song_target(song: &str) -> TargetIdentity {
        TargetIdentity {
            song_id: Some(SongId(song.to_owned())),
            ..TargetIdentity::default()
        }
    }

    // ── mode distinctions ──────────────────────────────────────────────────

    #[test]
    fn no_corpus_returns_none() {
        let corpus = LoadedCorpus {
            manifest: manifest_of(vec![meta("a", Some("sha"), Some("song1"), false)]),
            loaded: vec![loaded("a", "sha", "song1", 0, 60, false)],
            skipped: Vec::new(),
        };
        let out = prepare_corpus_for_mode(corpus, CorpusMode::NoCorpus, &TargetIdentity::default())
            .expect("NoCorpus never refuses");
        assert!(out.is_none(), "NoCorpus is a corpus-free run");
    }

    #[test]
    fn leaky_diagnostic_compiles_all_unfiltered() {
        let metas = vec![
            meta("a", Some("shaA"), Some("song1"), false),
            meta("b", Some("shaB"), Some("song2"), false),
        ];
        let corpus = LoadedCorpus {
            manifest: manifest_of(metas),
            loaded: vec![
                loaded("a", "shaA", "song1", 0, 60, false),
                loaded("b", "shaB", "song2", 240, 62, false),
            ],
            skipped: Vec::new(),
        };
        let out =
            prepare_corpus_for_mode(corpus, CorpusMode::LeakyDiagnostic, &song_target("song1"))
                .expect("leaky never refuses")
                .expect("leaky is corpus-backed");
        // Unfiltered: both records' sources contribute references.
        assert_eq!(
            out.references.len(),
            2,
            "LeakyDiagnostic keeps every record"
        );
    }

    #[test]
    fn no_corpus_and_leaky_are_distinct() {
        let build = || LoadedCorpus {
            manifest: manifest_of(vec![meta("a", Some("shaA"), Some("song1"), false)]),
            loaded: vec![loaded("a", "shaA", "song1", 0, 60, false)],
            skipped: Vec::new(),
        };
        let none =
            prepare_corpus_for_mode(build(), CorpusMode::NoCorpus, &TargetIdentity::default())
                .expect("ok");
        let leaky = prepare_corpus_for_mode(
            build(),
            CorpusMode::LeakyDiagnostic,
            &TargetIdentity::default(),
        )
        .expect("ok");
        assert!(none.is_none() && leaky.is_some(), "different experiments");
    }

    // ── fail-closed refusals ───────────────────────────────────────────────

    #[test]
    fn song_mode_refuses_uncurated_source() {
        // 'b' carries no song_id → preflight UncuratedSource.
        let metas = vec![
            meta("a", Some("shaA"), Some("song1"), false),
            meta("b", Some("shaB"), None, false),
        ];
        let corpus = LoadedCorpus {
            manifest: manifest_of(metas),
            loaded: vec![
                loaded("a", "shaA", "song1", 0, 60, false),
                LoadedChunk {
                    meta: meta("b", Some("shaB"), None, false),
                    sliced: one_bar_score(240, 62),
                    track: 0,
                },
            ],
            skipped: Vec::new(),
        };
        let err =
            prepare_corpus_for_mode(corpus, CorpusMode::HoldoutTargetSong, &song_target("song1"))
                .expect_err("uncurated corpus must refuse");
        assert!(matches!(err, HoldoutError::Preflight(ref rs)
            if rs.iter().any(|r| matches!(r, SongHoldoutRefusal::UncuratedSource { sha256, .. } if sha256 == "shaB"))));
    }

    #[test]
    fn song_mode_refuses_unidentified_source() {
        // A chunk with no sha256 → preflight UnidentifiedSource.
        let metas = vec![meta("a", None, Some("song1"), false)];
        let corpus = LoadedCorpus {
            manifest: manifest_of(metas),
            loaded: vec![LoadedChunk {
                meta: meta("a", None, Some("song1"), false),
                sliced: one_bar_score(0, 60),
                track: 0,
            }],
            skipped: Vec::new(),
        };
        let err =
            prepare_corpus_for_mode(corpus, CorpusMode::HoldoutTargetSong, &song_target("song1"))
                .expect_err("unidentified source must refuse");
        assert!(matches!(err, HoldoutError::Preflight(ref rs)
            if rs.iter().any(|r| matches!(r, SongHoldoutRefusal::UnidentifiedSource { .. }))));
    }

    #[test]
    fn song_mode_refuses_missing_target_song_id() {
        // Curated + consistent + bound, but no target song_id.
        let metas = vec![meta("a", Some("shaA"), Some("song1"), false)];
        let corpus = LoadedCorpus {
            manifest: manifest_of(metas),
            loaded: vec![loaded("a", "shaA", "song1", 0, 60, false)],
            skipped: Vec::new(),
        };
        let err = prepare_corpus_for_mode(
            corpus,
            CorpusMode::HoldoutTargetSong,
            &TargetIdentity::default(),
        )
        .expect_err("no target must refuse");
        assert_eq!(err, HoldoutError::MissingTargetSongId);
    }

    #[test]
    fn song_mode_propagates_manifest_disagreement() {
        // Present songs map that omits a labelled source → ManifestLabelMissing.
        let mut songs = BTreeMap::new();
        songs.insert(SongId("song1".to_owned()), Vec::new());
        let manifest = CorpusManifest {
            schema_version: SCHEMA_VERSION,
            chunks: vec![meta("a", Some("shaA"), Some("song1"), false)],
            groups: Vec::new(),
            songs: Some(songs),
        };
        let corpus = LoadedCorpus {
            manifest,
            loaded: vec![loaded("a", "shaA", "song1", 0, 60, false)],
            skipped: Vec::new(),
        };
        let err =
            prepare_corpus_for_mode(corpus, CorpusMode::HoldoutTargetSong, &song_target("song1"))
                .expect_err("manifest disagreement must refuse");
        assert!(matches!(err, HoldoutError::Preflight(ref rs)
            if rs.iter().any(|r| matches!(r, SongHoldoutRefusal::ManifestLabelMissing { .. }))));
    }

    #[test]
    fn song_mode_propagates_inconsistent_source() {
        // One sha256 labelled with two SongIds → InconsistentSource.
        let metas = vec![
            meta("a", Some("shaA"), Some("song1"), false),
            meta("b", Some("shaA"), Some("song2"), false),
        ];
        let corpus = LoadedCorpus {
            manifest: manifest_of(metas),
            loaded: vec![
                loaded("a", "shaA", "song1", 0, 60, false),
                loaded("b", "shaA", "song2", 240, 62, false),
            ],
            skipped: Vec::new(),
        };
        let err =
            prepare_corpus_for_mode(corpus, CorpusMode::HoldoutTargetSong, &song_target("song1"))
                .expect_err("inconsistent source must refuse");
        assert!(matches!(err, HoldoutError::Preflight(ref rs)
            if rs.iter().any(|r| matches!(r, SongHoldoutRefusal::InconsistentSource { .. }))));
    }

    // ── single-authority binding ───────────────────────────────────────────

    #[test]
    fn binding_refuses_provenance_mismatch() {
        // Manifest says shaA -> song1; the loaded record says shaA -> song2.
        // Preflight would validate one dataset while the filter executes another;
        // binding must refuse before either.
        let manifest = manifest_of(vec![meta("a", Some("shaA"), Some("song1"), false)]);
        let corpus = LoadedCorpus {
            manifest,
            loaded: vec![loaded("a", "shaA", "song2", 0, 60, false)],
            skipped: Vec::new(),
        };
        let err =
            prepare_corpus_for_mode(corpus, CorpusMode::HoldoutTargetSong, &song_target("song1"))
                .expect_err("stale manifest must refuse");
        assert!(matches!(err, HoldoutError::CorpusBinding(ref rs)
            if rs.iter().any(|r| matches!(r, BindingRefusal::ProvenanceMismatch(id) if id.0 == "a"))));
    }

    #[test]
    fn binding_refuses_loaded_not_in_manifest() {
        let manifest = manifest_of(vec![meta("a", Some("shaA"), Some("song1"), false)]);
        let corpus = LoadedCorpus {
            manifest,
            loaded: vec![loaded("ghost", "shaG", "song1", 0, 60, false)],
            skipped: Vec::new(),
        };
        let err =
            prepare_corpus_for_mode(corpus, CorpusMode::HoldoutTargetSong, &song_target("song1"))
                .expect_err("record not in manifest must refuse");
        assert!(matches!(err, HoldoutError::CorpusBinding(ref rs)
            if rs.iter().any(|r| matches!(r, BindingRefusal::LoadedNotInManifest(id) if id.0 == "ghost"))));
    }

    #[test]
    fn binding_refuses_duplicate_loaded() {
        let manifest = manifest_of(vec![meta("a", Some("shaA"), Some("song1"), false)]);
        let corpus = LoadedCorpus {
            manifest,
            loaded: vec![
                loaded("a", "shaA", "song1", 0, 60, false),
                loaded("a", "shaA", "song1", 240, 62, false),
            ],
            skipped: Vec::new(),
        };
        let err =
            prepare_corpus_for_mode(corpus, CorpusMode::HoldoutTargetSong, &song_target("song1"))
                .expect_err("duplicate loaded id must refuse");
        assert!(matches!(err, HoldoutError::CorpusBinding(ref rs)
            if rs.iter().any(|r| matches!(r, BindingRefusal::DuplicateLoaded(id) if id.0 == "a"))));
    }

    // ── exclusion + zero leakage ───────────────────────────────────────────

    /// A curated corpus: song `work_x` has two source files (shaA, shaB), and a
    /// keeper `work_y` (shaC). shaA carries gesture stats.
    fn curated_corpus() -> LoadedCorpus {
        LoadedCorpus {
            manifest: manifest_of(vec![
                meta("a", Some("shaA"), Some("work_x"), true),
                meta("b", Some("shaB"), Some("work_x"), false),
                meta("c", Some("shaC"), Some("work_y"), false),
            ]),
            loaded: vec![
                loaded("a", "shaA", "work_x", 0, 60, true),
                loaded("b", "shaB", "work_x", 240, 62, false),
                loaded("c", "shaC", "work_y", 480, 64, false),
            ],
            skipped: Vec::new(),
        }
    }

    #[test]
    fn song_mode_excludes_every_representation_with_zero_leakage() {
        let held = prepare_corpus_for_mode(
            curated_corpus(),
            CorpusMode::HoldoutTargetSong,
            &song_target("work_x"),
        )
        .expect("curated corpus passes preflight")
        .expect("corpus-backed");

        // The material is exactly what the keeper alone would produce — so both
        // work_x representations contribute zero to every channel.
        let keeper_only = corpus_material(
            vec![loaded("c", "shaC", "work_y", 480, 64, false)],
            Vec::new(),
        );
        assert_eq!(
            held.references, keeper_only.references,
            "no held-out references leak"
        );
        assert_eq!(
            held.rhythms, keeper_only.rhythms,
            "no held-out rhythm templates leak"
        );
        assert_eq!(
            held.gesture, keeper_only.gesture,
            "no held-out gesture stats leak"
        );
        assert_eq!(
            held.references.len(),
            1,
            "both work_x source files excluded, keeper remains"
        );
        // The keeper carries no gesture; work_x's gesture must not survive.
        assert!(
            held.gesture.is_none(),
            "excluded song's gesture did not leak"
        );
    }

    #[test]
    fn song_mode_is_deterministic() {
        let a = prepare_corpus_for_mode(
            curated_corpus(),
            CorpusMode::HoldoutTargetSong,
            &song_target("work_x"),
        )
        .expect("ok")
        .expect("some");
        let b = prepare_corpus_for_mode(
            curated_corpus(),
            CorpusMode::HoldoutTargetSong,
            &song_target("work_x"),
        )
        .expect("ok")
        .expect("some");
        assert_eq!(a.references, b.references);
        assert_eq!(a.rhythms, b.rhythms);
        assert_eq!(a.gesture, b.gesture);
    }

    // ── target must actually be held out ───────────────────────────────────

    #[test]
    fn song_mode_refuses_target_absent_from_corpus() {
        // Target names a song no loaded chunk carries → returning the whole
        // corpus would relabel an absent holdout as valid.
        let err = prepare_corpus_for_mode(
            curated_corpus(),
            CorpusMode::HoldoutTargetSong,
            &song_target("work_absent"),
        )
        .expect_err("absent target must refuse");
        assert_eq!(
            err,
            HoldoutError::TargetSongAbsent(SongId("work_absent".to_owned()))
        );
    }

    #[test]
    fn song_mode_refuses_target_present_only_in_skipped_record() {
        // The manifest curates work_z, but its only source failed to load
        // (skipped). Holding out work_z must refuse, not quietly succeed because
        // an import failure removed it.
        let corpus = LoadedCorpus {
            manifest: manifest_of(vec![
                meta("a", Some("shaA"), Some("work_x"), false),
                meta("z", Some("shaZ"), Some("work_z"), false),
            ]),
            loaded: vec![loaded("a", "shaA", "work_x", 0, 60, false)],
            skipped: vec!["z.gp5".to_owned()],
        };
        let err = prepare_corpus_for_mode(
            corpus,
            CorpusMode::HoldoutTargetSong,
            &song_target("work_z"),
        )
        .expect_err("target only in a skipped record must refuse");
        assert_eq!(
            err,
            HoldoutError::TargetSongAbsent(SongId("work_z".to_owned()))
        );
    }

    #[test]
    fn song_mode_successful_holdout_excludes_at_least_one() {
        // work_x has two loaded representations; a successful run must have
        // actually dropped them (3 loaded -> 1 kept).
        let held = prepare_corpus_for_mode(
            curated_corpus(),
            CorpusMode::HoldoutTargetSong,
            &song_target("work_x"),
        )
        .expect("ok")
        .expect("some");
        assert_eq!(
            held.references.len(),
            1,
            "the two work_x representations were excluded"
        );
    }

    // ── single authority: full metadata, no ambiguous manifest ─────────────

    #[test]
    fn binding_refuses_duplicate_manifest_id() {
        // Two manifest chunks share a ChunkId → the loaded record maps to no
        // single authoritative entry.
        let corpus = LoadedCorpus {
            manifest: manifest_of(vec![
                meta("a", Some("shaA"), Some("song1"), false),
                meta("a", Some("shaA"), Some("song1"), false),
            ]),
            loaded: vec![loaded("a", "shaA", "song1", 0, 60, false)],
            skipped: Vec::new(),
        };
        let err =
            prepare_corpus_for_mode(corpus, CorpusMode::HoldoutTargetSong, &song_target("song1"))
                .expect_err("duplicate manifest id must refuse");
        assert!(matches!(err, HoldoutError::CorpusBinding(ref rs)
            if rs.iter().any(|r| matches!(r, BindingRefusal::DuplicateManifest(id) if id.0 == "a"))));
    }

    #[test]
    fn binding_refuses_matching_identity_but_differing_bar_range() {
        // song_id + sha256 agree, but a material-defining field (bar_range)
        // differs → not the same authoritative record.
        let manifest_meta = meta("a", Some("shaA"), Some("song1"), false);
        let mut loaded_meta = meta("a", Some("shaA"), Some("song1"), false);
        loaded_meta.source.bar_range = Some((0, 4));
        let corpus = LoadedCorpus {
            manifest: manifest_of(vec![manifest_meta]),
            loaded: vec![LoadedChunk {
                meta: loaded_meta,
                sliced: one_bar_score(0, 60),
                track: 0,
            }],
            skipped: Vec::new(),
        };
        let err =
            prepare_corpus_for_mode(corpus, CorpusMode::HoldoutTargetSong, &song_target("song1"))
                .expect_err("differing bar_range must refuse");
        assert!(matches!(err, HoldoutError::CorpusBinding(ref rs)
            if rs.iter().any(|r| matches!(r, BindingRefusal::ProvenanceMismatch(id) if id.0 == "a"))));
    }

    #[test]
    fn binding_refuses_matching_identity_but_differing_gesture() {
        // Same source identity, but the loaded record carries gesture stats the
        // manifest entry does not → full-metadata mismatch.
        let manifest_meta = meta("a", Some("shaA"), Some("song1"), false);
        let loaded_meta = meta("a", Some("shaA"), Some("song1"), true);
        let corpus = LoadedCorpus {
            manifest: manifest_of(vec![manifest_meta]),
            loaded: vec![LoadedChunk {
                meta: loaded_meta,
                sliced: one_bar_score(0, 60),
                track: 0,
            }],
            skipped: Vec::new(),
        };
        let err =
            prepare_corpus_for_mode(corpus, CorpusMode::HoldoutTargetSong, &song_target("song1"))
                .expect_err("differing gesture metadata must refuse");
        assert!(matches!(err, HoldoutError::CorpusBinding(ref rs)
            if rs.iter().any(|r| matches!(r, BindingRefusal::ProvenanceMismatch(id) if id.0 == "a"))));
    }

    // ── source-file mode (HoldoutTargetSourceFile) ──────────────────────────

    fn file_target(sha: &str) -> TargetIdentity {
        TargetIdentity {
            source_sha256: Some(sha.to_owned()),
            ..TargetIdentity::default()
        }
    }

    /// A hash-identified chunk whose song may be uncurated (`None`).
    #[allow(clippy::too_many_arguments)]
    fn hashed(
        id: &str,
        sha: Option<&str>,
        song: Option<&str>,
        onset: u32,
        pitch: u8,
        gesture: bool,
    ) -> (ChunkMeta, LoadedChunk) {
        let m = meta(id, sha, song, gesture);
        (
            m.clone(),
            LoadedChunk {
                meta: m,
                sliced: one_bar_score(onset, pitch),
                track: 0,
            },
        )
    }

    #[test]
    fn file_mode_refuses_missing_target_sha256() {
        let (m, lc) = hashed("a", Some("shaA"), None, 0, 60, false);
        let corpus = LoadedCorpus {
            manifest: manifest_of(vec![m]),
            loaded: vec![lc],
            skipped: Vec::new(),
        };
        // TargetIdentity::default() has no source_sha256.
        let err = prepare_corpus_for_mode(
            corpus,
            CorpusMode::HoldoutTargetSourceFile,
            &TargetIdentity::default(),
        )
        .expect_err("no target sha must refuse");
        assert_eq!(err, HoldoutError::MissingTargetSourceSha256);
    }

    #[test]
    fn file_mode_refuses_unidentified_manifest_chunk() {
        // A participating manifest chunk without sha256 → fail closed before material.
        let (m, lc) = hashed("a", None, None, 0, 60, false);
        let corpus = LoadedCorpus {
            manifest: manifest_of(vec![m]),
            loaded: vec![lc],
            skipped: Vec::new(),
        };
        let err = prepare_corpus_for_mode(
            corpus,
            CorpusMode::HoldoutTargetSourceFile,
            &file_target("shaX"),
        )
        .expect_err("sha256-less chunk must refuse");
        assert!(matches!(err, HoldoutError::SourcePreflight(ref rs)
            if rs.iter().any(|r| matches!(r, SourceHoldoutRefusal::UnidentifiedSource(id) if id.0 == "a"))));
    }

    #[test]
    fn file_mode_refuses_target_hash_absent() {
        let (ma, la) = hashed("a", Some("shaA"), None, 0, 60, false);
        let (mb, lb) = hashed("b", Some("shaB"), None, 240, 62, false);
        let corpus = LoadedCorpus {
            manifest: manifest_of(vec![ma, mb]),
            loaded: vec![la, lb],
            skipped: Vec::new(),
        };
        let err = prepare_corpus_for_mode(
            corpus,
            CorpusMode::HoldoutTargetSourceFile,
            &file_target("shaZ"),
        )
        .expect_err("absent target hash must refuse");
        assert_eq!(err, HoldoutError::TargetSourceAbsent("shaZ".to_owned()));
    }

    #[test]
    fn file_mode_refuses_target_present_only_in_skipped() {
        // The manifest curates shaZ, but its source failed to load.
        let (ma, la) = hashed("a", Some("shaA"), None, 0, 60, false);
        let mz = meta("z", Some("shaZ"), None, false);
        let corpus = LoadedCorpus {
            manifest: manifest_of(vec![ma, mz]),
            loaded: vec![la],
            skipped: vec!["z.gp5".to_owned()],
        };
        let err = prepare_corpus_for_mode(
            corpus,
            CorpusMode::HoldoutTargetSourceFile,
            &file_target("shaZ"),
        )
        .expect_err("target only in skipped must refuse");
        assert_eq!(err, HoldoutError::TargetSourceAbsent("shaZ".to_owned()));
    }

    #[test]
    fn file_mode_excludes_every_chunk_of_target_hash_regardless_of_range_or_track() {
        // Two chunks share shaX with differing bar_range/track; both excluded.
        let ma = meta("a", Some("shaX"), None, false);
        let mut mb = meta("b", Some("shaX"), None, false);
        mb.source.bar_range = Some((1, 1));
        mb.source.track_index = Some(1);
        let mc = meta("c", Some("shaY"), None, false);
        let corpus = LoadedCorpus {
            manifest: manifest_of(vec![ma.clone(), mb.clone(), mc.clone()]),
            loaded: vec![
                LoadedChunk {
                    meta: ma,
                    sliced: one_bar_score(0, 60),
                    track: 0,
                },
                LoadedChunk {
                    meta: mb,
                    sliced: one_bar_score(240, 62),
                    track: 0,
                },
                LoadedChunk {
                    meta: mc,
                    sliced: one_bar_score(480, 64),
                    track: 0,
                },
            ],
            skipped: Vec::new(),
        };
        let held = prepare_corpus_for_mode(
            corpus,
            CorpusMode::HoldoutTargetSourceFile,
            &file_target("shaX"),
        )
        .expect("ok")
        .expect("some");
        assert_eq!(
            held.references.len(),
            1,
            "both shaX chunks excluded regardless of range/track"
        );
    }

    #[test]
    fn file_mode_succeeds_on_song_uncurated_corpus() {
        // Complete hashes, but song_id is None everywhere — song mode would
        // refuse; file mode must succeed.
        let (ma, la) = hashed("a", Some("shaX"), None, 0, 60, false);
        let (mc, lc) = hashed("c", Some("shaY"), None, 480, 64, false);
        let corpus = LoadedCorpus {
            manifest: manifest_of(vec![ma, mc]),
            loaded: vec![la, lc],
            skipped: Vec::new(),
        };
        let held = prepare_corpus_for_mode(
            corpus,
            CorpusMode::HoldoutTargetSourceFile,
            &file_target("shaX"),
        )
        .expect("uncurated songs are fine for file mode")
        .expect("corpus-backed");
        assert_eq!(held.references.len(), 1);
    }

    fn file_corpus() -> LoadedCorpus {
        // shaX target (carries gesture), shaY keeper; one skipped name.
        let ma = meta("a", Some("shaX"), None, true);
        let mc = meta("c", Some("shaY"), None, false);
        LoadedCorpus {
            manifest: manifest_of(vec![ma.clone(), mc.clone()]),
            loaded: vec![
                LoadedChunk {
                    meta: ma,
                    sliced: one_bar_score(0, 60),
                    track: 0,
                },
                LoadedChunk {
                    meta: mc,
                    sliced: one_bar_score(480, 64),
                    track: 0,
                },
            ],
            skipped: vec!["gone.gp5".to_owned()],
        }
    }

    #[test]
    fn file_mode_zero_leakage_including_skipped() {
        let held = prepare_corpus_for_mode(
            file_corpus(),
            CorpusMode::HoldoutTargetSourceFile,
            &file_target("shaX"),
        )
        .expect("ok")
        .expect("some");
        let keeper_only = corpus_material(
            vec![LoadedChunk {
                meta: meta("c", Some("shaY"), None, false),
                sliced: one_bar_score(480, 64),
                track: 0,
            }],
            vec!["gone.gp5".to_owned()],
        );
        assert_eq!(held.references, keeper_only.references);
        assert_eq!(held.rhythms, keeper_only.rhythms);
        assert_eq!(held.gesture, keeper_only.gesture);
        assert_eq!(
            held.skipped, keeper_only.skipped,
            "skipped reporting preserved"
        );
        assert!(
            held.gesture.is_none(),
            "excluded source's gesture did not leak"
        );
    }

    #[test]
    fn file_mode_binding_runs_before_source_preflight() {
        // A binding mismatch must surface before the source preflight.
        let manifest_meta = meta("a", Some("shaA"), None, false);
        let mut loaded_meta = meta("a", Some("shaA"), None, false);
        loaded_meta.source.bar_range = Some((0, 4));
        let corpus = LoadedCorpus {
            manifest: manifest_of(vec![manifest_meta]),
            loaded: vec![LoadedChunk {
                meta: loaded_meta,
                sliced: one_bar_score(0, 60),
                track: 0,
            }],
            skipped: Vec::new(),
        };
        let err = prepare_corpus_for_mode(
            corpus,
            CorpusMode::HoldoutTargetSourceFile,
            &file_target("shaA"),
        )
        .expect_err("binding refuses first");
        assert!(matches!(err, HoldoutError::CorpusBinding(_)));
    }

    #[test]
    fn file_mode_is_deterministic() {
        let a = prepare_corpus_for_mode(
            file_corpus(),
            CorpusMode::HoldoutTargetSourceFile,
            &file_target("shaX"),
        )
        .expect("ok")
        .expect("some");
        let b = prepare_corpus_for_mode(
            file_corpus(),
            CorpusMode::HoldoutTargetSourceFile,
            &file_target("shaX"),
        )
        .expect("ok")
        .expect("some");
        assert_eq!(a.references, b.references);
        assert_eq!(a.rhythms, b.rhythms);
        assert_eq!(a.gesture, b.gesture);
    }

    // ── fragment mode (HoldoutTargetFragment) ───────────────────────────────

    fn fragment_target(sha: &str, range: Option<(u32, u32)>) -> TargetIdentity {
        TargetIdentity {
            source_sha256: Some(sha.to_owned()),
            bar_range: range,
            track_index: None,
            song_id: None,
        }
    }

    /// A hash-identified, song-uncurated chunk at `range` on `track`; its manifest
    /// twin is identical so binding holds.
    fn frag_tracked(
        id: &str,
        sha: &str,
        range: Option<(u32, u32)>,
        track: Option<u32>,
        gesture: bool,
    ) -> (ChunkMeta, LoadedChunk) {
        let mut m = meta(id, Some(sha), None, gesture);
        m.source.bar_range = range;
        m.source.track_index = track;
        let onset = range.map_or(0, |(first, _)| first.saturating_mul(120) % 1920);
        let lc = LoadedChunk {
            meta: m.clone(),
            sliced: one_bar_score(onset, 60),
            track: 0,
        };
        (m, lc)
    }

    fn frag(id: &str, sha: &str, range: Option<(u32, u32)>) -> (ChunkMeta, LoadedChunk) {
        frag_tracked(id, sha, range, None, false)
    }

    /// Build a corpus whose manifest exactly mirrors the loaded records, plus
    /// optional manifest-only chunks (curated but unloaded) and skipped names.
    fn frag_corpus(
        loaded: Vec<(ChunkMeta, LoadedChunk)>,
        manifest_only: Vec<ChunkMeta>,
        skipped: &[&str],
    ) -> LoadedCorpus {
        let mut manifest_chunks: Vec<ChunkMeta> = loaded.iter().map(|(m, _)| m.clone()).collect();
        manifest_chunks.extend(manifest_only);
        LoadedCorpus {
            manifest: manifest_of(manifest_chunks),
            loaded: loaded.into_iter().map(|(_, l)| l).collect(),
            skipped: skipped.iter().map(|s| (*s).to_owned()).collect(),
        }
    }

    #[test]
    fn ranges_overlap_predicate_is_inclusive_with_none_whole_source() {
        assert!(
            ranges_overlap(None, Some((10, 12))),
            "target None overlaps all"
        );
        assert!(
            ranges_overlap(Some((10, 12)), None),
            "chunk None overlaps all"
        );
        assert!(
            ranges_overlap(Some((0, 3)), Some((3, 5))),
            "endpoint touch overlaps"
        );
        assert!(
            ranges_overlap(Some((0, 3)), Some((1, 2))),
            "contained overlaps"
        );
        assert!(
            ranges_overlap(Some((1, 2)), Some((0, 5))),
            "containing overlaps"
        );
        assert!(
            ranges_overlap(Some((0, 3)), Some((2, 5))),
            "partial overlaps"
        );
        assert!(
            !ranges_overlap(Some((0, 2)), Some((3, 5))),
            "disjoint does not overlap"
        );
    }

    #[test]
    fn fragment_mode_refuses_missing_target_sha256() {
        let corpus = frag_corpus(vec![frag("a", "shaA", Some((0, 1)))], Vec::new(), &[]);
        let target = TargetIdentity {
            bar_range: Some((0, 1)),
            ..TargetIdentity::default()
        };
        let err = prepare_corpus_for_mode(corpus, CorpusMode::HoldoutTargetFragment, &target)
            .expect_err("no target sha must refuse");
        assert_eq!(err, HoldoutError::MissingTargetSourceSha256);
    }

    #[test]
    fn fragment_mode_refuses_unidentified_manifest_chunk() {
        let (m, lc) = frag("a", "shaA", Some((0, 1)));
        let mut unhashed_m = meta("u", None, None, false);
        unhashed_m.source.bar_range = Some((0, 1));
        let corpus = frag_corpus(vec![(m, lc)], vec![unhashed_m], &["u.gp5"]);
        let err = prepare_corpus_for_mode(
            corpus,
            CorpusMode::HoldoutTargetFragment,
            &fragment_target("shaA", None),
        )
        .expect_err("sha256-less chunk must refuse");
        assert!(matches!(err, HoldoutError::SourcePreflight(_)));
    }

    #[test]
    fn fragment_mode_refuses_inverted_target_range() {
        let corpus = frag_corpus(vec![frag("a", "shaA", Some((0, 3)))], Vec::new(), &[]);
        let err = prepare_corpus_for_mode(
            corpus,
            CorpusMode::HoldoutTargetFragment,
            &fragment_target("shaA", Some((5, 2))),
        )
        .expect_err("inverted target range must refuse");
        assert_eq!(
            err,
            HoldoutError::InvalidTargetBarRange {
                first_bar: 5,
                last_bar: 2
            }
        );
    }

    #[test]
    fn fragment_mode_refuses_inverted_manifest_range_on_target_source() {
        // A target-source manifest chunk with first > last, caught before filtering.
        let (mut m, _) = frag("a", "shaA", Some((5, 2)));
        m.source.bar_range = Some((5, 2));
        let lc = LoadedChunk {
            meta: m.clone(),
            sliced: one_bar_score(0, 60),
            track: 0,
        };
        let corpus = frag_corpus(vec![(m, lc)], Vec::new(), &[]);
        let err = prepare_corpus_for_mode(
            corpus,
            CorpusMode::HoldoutTargetFragment,
            &fragment_target("shaA", Some((0, 3))),
        )
        .expect_err("inverted manifest range must refuse");
        assert!(matches!(err, HoldoutError::FragmentPreflight(ref rs)
            if rs.iter().any(|r| matches!(r, FragmentHoldoutRefusal::InvalidBarRange { chunk_id, first_bar, last_bar }
                if chunk_id.0 == "a" && *first_bar == 5 && *last_bar == 2))));
    }

    #[test]
    fn fragment_target_none_excludes_every_chunk_of_target_hash() {
        let corpus = frag_corpus(
            vec![
                frag("a", "shaX", Some((0, 1))),
                frag("b", "shaX", Some((5, 6))),
                frag("c", "shaY", Some((0, 1))),
            ],
            Vec::new(),
            &[],
        );
        let held = prepare_corpus_for_mode(
            corpus,
            CorpusMode::HoldoutTargetFragment,
            &fragment_target("shaX", None),
        )
        .expect("ok")
        .expect("some");
        assert_eq!(
            held.references.len(),
            1,
            "target None is whole-source: both shaX chunks excluded"
        );
    }

    #[test]
    fn fragment_chunk_none_overlaps_every_target_range() {
        let corpus = frag_corpus(
            vec![frag("a", "shaX", None), frag("c", "shaY", Some((0, 1)))],
            Vec::new(),
            &[],
        );
        let held = prepare_corpus_for_mode(
            corpus,
            CorpusMode::HoldoutTargetFragment,
            &fragment_target("shaX", Some((10, 12))),
        )
        .expect("ok")
        .expect("some");
        assert_eq!(
            held.references.len(),
            1,
            "whole-source chunk overlaps any target range"
        );
    }

    #[test]
    fn fragment_excludes_exact_contained_containing_partial_and_touching() {
        let corpus = frag_corpus(
            vec![
                frag("exact", "shaX", Some((0, 3))),
                frag("contained", "shaX", Some((1, 2))),
                frag("containing", "shaX", Some((0, 9))),
                frag("partial", "shaX", Some((2, 5))),
                frag("touching", "shaX", Some((3, 6))),
                frag("keeper", "shaY", Some((0, 3))),
            ],
            Vec::new(),
            &[],
        );
        let held = prepare_corpus_for_mode(
            corpus,
            CorpusMode::HoldoutTargetFragment,
            &fragment_target("shaX", Some((0, 3))),
        )
        .expect("ok")
        .expect("some");
        assert_eq!(
            held.references.len(),
            1,
            "every overlapping shaX chunk excluded; only the other source remains"
        );
    }

    #[test]
    fn fragment_keeps_disjoint_same_source_range() {
        let corpus = frag_corpus(
            vec![
                frag("target", "shaX", Some((0, 2))),
                frag("disjoint", "shaX", Some((3, 5))),
            ],
            Vec::new(),
            &[],
        );
        let held = prepare_corpus_for_mode(
            corpus,
            CorpusMode::HoldoutTargetFragment,
            &fragment_target("shaX", Some((0, 2))),
        )
        .expect("ok")
        .expect("some");
        assert_eq!(
            held.references.len(),
            1,
            "disjoint same-source fragment survives"
        );
    }

    #[test]
    fn fragment_keeps_overlapping_range_from_other_source() {
        let corpus = frag_corpus(
            vec![
                frag("target", "shaX", Some((0, 3))),
                frag("other", "shaY", Some((0, 3))),
            ],
            Vec::new(),
            &[],
        );
        let held = prepare_corpus_for_mode(
            corpus,
            CorpusMode::HoldoutTargetFragment,
            &fragment_target("shaX", Some((0, 3))),
        )
        .expect("ok")
        .expect("some");
        assert_eq!(
            held.references.len(),
            1,
            "different source hash never overlaps for holdout"
        );
    }

    #[test]
    fn fragment_excludes_overlap_across_tracks() {
        let corpus = frag_corpus(
            vec![
                frag_tracked("t0", "shaX", Some((0, 3)), Some(0), false),
                frag_tracked("t1", "shaX", Some((0, 3)), Some(1), false),
                frag("keeper", "shaY", Some((0, 3))),
            ],
            Vec::new(),
            &[],
        );
        // target names track 0, but track must not narrow exclusion.
        let target = TargetIdentity {
            source_sha256: Some("shaX".to_owned()),
            bar_range: Some((0, 3)),
            track_index: Some(0),
            song_id: None,
        };
        let held = prepare_corpus_for_mode(corpus, CorpusMode::HoldoutTargetFragment, &target)
            .expect("ok")
            .expect("some");
        assert_eq!(
            held.references.len(),
            1,
            "both tracks' overlapping chunks excluded"
        );
    }

    #[test]
    fn fragment_refuses_when_nothing_overlaps() {
        let corpus = frag_corpus(vec![frag("a", "shaX", Some((5, 7)))], Vec::new(), &[]);
        let err = prepare_corpus_for_mode(
            corpus,
            CorpusMode::HoldoutTargetFragment,
            &fragment_target("shaX", Some((0, 2))),
        )
        .expect_err("no overlap must refuse");
        assert_eq!(
            err,
            HoldoutError::TargetFragmentAbsent {
                source_sha256: "shaX".to_owned(),
                bar_range: Some((0, 2))
            }
        );
    }

    #[test]
    fn fragment_refuses_target_present_only_in_skipped() {
        let (m, lc) = frag("keeper", "shaY", Some((0, 3)));
        let mut mz = meta("z", Some("shaX"), None, false);
        mz.source.bar_range = Some((0, 3));
        let corpus = frag_corpus(vec![(m, lc)], vec![mz], &["z.gp5"]);
        let err = prepare_corpus_for_mode(
            corpus,
            CorpusMode::HoldoutTargetFragment,
            &fragment_target("shaX", Some((0, 3))),
        )
        .expect_err("target only in skipped must refuse");
        assert_eq!(
            err,
            HoldoutError::TargetFragmentAbsent {
                source_sha256: "shaX".to_owned(),
                bar_range: Some((0, 3))
            }
        );
    }

    #[test]
    fn fragment_succeeds_on_song_uncurated_corpus() {
        let corpus = frag_corpus(
            vec![
                frag("a", "shaX", Some((0, 3))),
                frag("c", "shaY", Some((0, 3))),
            ],
            Vec::new(),
            &[],
        );
        let held = prepare_corpus_for_mode(
            corpus,
            CorpusMode::HoldoutTargetFragment,
            &fragment_target("shaX", Some((0, 3))),
        )
        .expect("uncurated songs are fine for fragment mode")
        .expect("corpus-backed");
        assert_eq!(held.references.len(), 1);
    }

    #[test]
    fn fragment_zero_leakage_including_skipped() {
        let (mx, lx) = frag_tracked("x", "shaX", Some((0, 3)), None, true);
        let (my, ly) = frag("y", "shaY", Some((5, 7)));
        let corpus = LoadedCorpus {
            manifest: manifest_of(vec![mx, my]),
            loaded: vec![lx, ly],
            skipped: vec!["gone.gp5".to_owned()],
        };
        let held = prepare_corpus_for_mode(
            corpus,
            CorpusMode::HoldoutTargetFragment,
            &fragment_target("shaX", Some((0, 3))),
        )
        .expect("ok")
        .expect("some");
        let keeper_only = corpus_material(
            vec![frag("y", "shaY", Some((5, 7))).1],
            vec!["gone.gp5".to_owned()],
        );
        assert_eq!(held.references, keeper_only.references);
        assert_eq!(held.rhythms, keeper_only.rhythms);
        assert_eq!(held.gesture, keeper_only.gesture);
        assert_eq!(held.skipped, keeper_only.skipped);
        assert!(
            held.gesture.is_none(),
            "excluded fragment's gesture did not leak"
        );
    }

    #[test]
    fn fragment_binding_runs_before_preflight() {
        let manifest_meta = meta("a", Some("shaA"), None, false);
        let mut loaded_meta = meta("a", Some("shaA"), None, false);
        loaded_meta.source.track_index = Some(7);
        let corpus = LoadedCorpus {
            manifest: manifest_of(vec![manifest_meta]),
            loaded: vec![LoadedChunk {
                meta: loaded_meta,
                sliced: one_bar_score(0, 60),
                track: 0,
            }],
            skipped: Vec::new(),
        };
        let err = prepare_corpus_for_mode(
            corpus,
            CorpusMode::HoldoutTargetFragment,
            &fragment_target("shaA", Some((0, 0))),
        )
        .expect_err("binding refuses first");
        assert!(matches!(err, HoldoutError::CorpusBinding(_)));
    }

    #[test]
    fn fragment_mode_is_deterministic() {
        let build = || {
            frag_corpus(
                vec![
                    frag("a", "shaX", Some((0, 3))),
                    frag("c", "shaY", Some((0, 3))),
                ],
                Vec::new(),
                &[],
            )
        };
        let a = prepare_corpus_for_mode(
            build(),
            CorpusMode::HoldoutTargetFragment,
            &fragment_target("shaX", Some((0, 3))),
        )
        .expect("ok")
        .expect("some");
        let b = prepare_corpus_for_mode(
            build(),
            CorpusMode::HoldoutTargetFragment,
            &fragment_target("shaX", Some((0, 3))),
        )
        .expect("ok")
        .expect("some");
        assert_eq!(a.references, b.references);
        assert_eq!(a.rhythms, b.rhythms);
        assert_eq!(a.gesture, b.gesture);
    }
}
