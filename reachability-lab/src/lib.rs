//! Offline Reachability Lab — the holdout-filtering boundary (ADR-0032).
//!
//! Holdout must run while provenance still exists on `LoadedChunk`, **before**
//! [`corpus_material`] converts chunks into anonymous rhythm templates, novelty
//! references, and aggregate gesture data. This crate owns that boundary and the
//! mode/target/error vocabulary; production CLI, cockpit, generation, and
//! reranking acquire no holdout policy.
//!
//! This first slice (ADR-0032 §7) implements **song mode only**, fail-closed,
//! over a single-authority [`LoadedCorpus`]. File and fragment modes, and any
//! measurement axes, are separate later slices.

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
}

/// The identity a holdout targets. Song mode uses `song_id`; the file / fragment
/// / measurement fields (`source_sha256`, `bar_range`, `track_index`,
/// `projection`, `eligibility`) join with their own later slices.
#[derive(Debug, Clone, Default)]
pub struct TargetIdentity {
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
    /// A loaded record whose identity facts (`song_id` / `sha256`) disagree with
    /// the manifest chunk of the same `ChunkId` — the stale-manifest leak.
    ProvenanceMismatch(ChunkId),
}

/// Prepare a loaded corpus for a run mode, fail-closed (ADR-0032).
///
/// Returns `Ok(None)` for a genuinely corpus-free run (`NoCorpus`) and
/// `Ok(Some(_))` for a corpus-backed one. For `HoldoutTargetSong`: bind the
/// loaded records to the manifest (single authority), run
/// `song_holdout_preflight` over the whole manifest, require a target `song_id`,
/// exclude every `LoadedChunk` carrying it, and only then compile the rest.
///
/// # Errors
///
/// [`HoldoutError`] when `HoldoutTargetSong` cannot proceed fail-closed: a
/// single-authority binding violation ([`HoldoutError::CorpusBinding`]), a
/// preflight refusal ([`HoldoutError::Preflight`]), a missing target
/// ([`HoldoutError::MissingTargetSongId`]), or a target carried by no loaded
/// chunk ([`HoldoutError::TargetSongAbsent`]). `NoCorpus` and `LeakyDiagnostic`
/// never refuse.
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
    }
}

/// Enforce the ADR-0032 single-authority invariant: every loaded record maps by
/// `ChunkId` to exactly one manifest chunk whose identity facts (`song_id` /
/// `sha256`) agree with it, and no `ChunkId` is loaded twice. Otherwise the
/// preflight could validate one dataset while the filter executes another.
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
}
