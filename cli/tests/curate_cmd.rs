// TDD red phase for S5 curate command.
// Fails to compile until `griff curate` is implemented.
// S14 Phase 3 extends the suite: curate must persist structure metrics
// (fails to compile until `ChunkMeta.structure` exists).
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::missing_assert_message,
    clippy::absolute_paths,
    clippy::indexing_slicing,
    clippy::too_many_lines
)]

use std::{
    io::Write as _,
    path::PathBuf,
    process::{Command, Stdio},
};

use griff_core::{
    boundary,
    complement::measure_pair_axes,
    corpus::{
        Acquisition, ChunkId, ChunkMeta, CorpusManifest, EnsembleGroup, EnsembleRef, RightsInfo,
        RightsStatus, StyleCohort, SCHEMA_VERSION,
    },
    event::Ticks,
    gesture, midi,
    score::AtomEvent,
    structure,
};

/// Locate the compiled `griff` binary.
fn griff_bin() -> PathBuf {
    // When running under `cargo test`, CARGO_BIN_EXE_griff is set.
    std::env::var_os("CARGO_BIN_EXE_griff").map_or_else(
        // absolute path ok in test
        || PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/debug/griff"),
        Into::into,
    )
}

#[test]
fn curate_help_exits_zero() {
    let out = Command::new(griff_bin())
        .args(["curate", "--help"])
        .output()
        .expect("run griff curate --help");
    assert!(out.status.success(), "griff curate --help must exit 0");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("curate"),
        "help output must mention 'curate'"
    );
}

#[test]
fn curate_nonexistent_file_exits_nonzero() {
    let out = Command::new(griff_bin())
        .args(["curate", "nonexistent_file_that_does_not_exist.mid"])
        .output()
        .expect("run griff curate");
    assert!(
        !out.status.success(),
        "curate on missing file must exit non-zero"
    );
}

/// S14 Phase 3: the written record carries the measured structure metrics of
/// the first note-bearing track, equal to what `measure_structure` reports.
#[test]
fn curate_records_structure_metrics_of_the_first_note_bearing_track() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/simple_4_4.mid");
    let out_path =
        std::env::temp_dir().join(format!("griff_curate_p3_{}.chunk.json", std::process::id()));

    let mut child = Command::new(griff_bin())
        .arg("curate")
        .arg(&fixture)
        .arg("--output")
        .arg(&out_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn griff curate");
    child
        .stdin
        .as_mut()
        .expect("piped stdin")
        // id, title, tuning (default), tags (none), flags (default), decision (none)
        .write_all(b"p3_001\nPhase Three\n\n\n\n\n")
        .expect("write curate answers");
    let out = child.wait_with_output().expect("wait for curate");
    assert!(
        out.status.success(),
        "curate must exit 0: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let json = std::fs::read_to_string(&out_path).expect("curate wrote the record");
    // Cleanup is best-effort; the named binding satisfies let-underscore lints.
    let _cleanup = std::fs::remove_file(&out_path);
    let meta: ChunkMeta = serde_json::from_str(&json).expect("record parses as ChunkMeta");

    let bytes = std::fs::read(&fixture).expect("fixture bytes");
    let score = midi::import_score(&bytes).expect("fixture imports");
    let track = score
        .tracks
        .iter()
        .position(|t| {
            t.voices
                .iter()
                .flat_map(|v| &v.event_groups)
                .flat_map(|g| &g.atoms)
                .any(|a| matches!(a, AtomEvent::Note(_)))
        })
        .expect("fixture has a note-bearing track");
    let expected = structure::measure_structure(&score, track).expect("metrics measure");

    assert_eq!(
        meta.structure,
        Some(expected),
        "curate persists the measured metrics"
    );
}

/// Corpus schema v3: the written record also carries the measured burst/rest
/// gesture statistics of the same track, equal to what `measure_gesture`
/// reports.
#[test]
fn curate_records_gesture_stats_of_the_first_note_bearing_track() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/simple_4_4.mid");
    let out_path =
        std::env::temp_dir().join(format!("griff_curate_v3_{}.chunk.json", std::process::id()));

    let mut child = Command::new(griff_bin())
        .arg("curate")
        .arg(&fixture)
        .arg("--output")
        .arg(&out_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn griff curate");
    child
        .stdin
        .as_mut()
        .expect("piped stdin")
        // id, title, tuning (default), tags (none), flags (default), decision (none)
        .write_all(b"v3_001\nSchema Three\n\n\n\n\n")
        .expect("write curate answers");
    let out = child.wait_with_output().expect("wait for curate");
    assert!(
        out.status.success(),
        "curate must exit 0: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let json = std::fs::read_to_string(&out_path).expect("curate wrote the record");
    // Cleanup is best-effort; the named binding satisfies let-underscore lints.
    let _cleanup = std::fs::remove_file(&out_path);
    let meta: ChunkMeta = serde_json::from_str(&json).expect("record parses as ChunkMeta");

    let bytes = std::fs::read(&fixture).expect("fixture bytes");
    let score = midi::import_score(&bytes).expect("fixture imports");
    let track = score
        .tracks
        .iter()
        .position(|t| {
            t.voices
                .iter()
                .flat_map(|v| &v.event_groups)
                .flat_map(|g| &g.atoms)
                .any(|a| matches!(a, AtomEvent::Note(_)))
        })
        .expect("fixture has a note-bearing track");
    let expected = gesture::measure_gesture(&score, track).expect("gesture stats measure");

    assert_eq!(
        meta.gesture,
        Some(expected),
        "curate persists the measured gesture stats"
    );
}

/// Schema v4: the cohort prompt (after tuning) is recorded; `1` = adjacent.
#[test]
fn curate_records_style_cohort() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/simple_4_4.mid");
    let out_path =
        std::env::temp_dir().join(format!("griff_curate_v4_{}.chunk.json", std::process::id()));

    let mut child = Command::new(griff_bin())
        .arg("curate")
        .arg(&fixture)
        .arg("--output")
        .arg(&out_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn griff curate");
    child
        .stdin
        .as_mut()
        .expect("piped stdin")
        // id, title, tuning (default), cohort (1 = adjacent), tags, flags, decision
        .write_all(b"uo_001\nAdjacent Chunk\n\n1\n\n\n\n")
        .expect("write curate answers");
    let out = child.wait_with_output().expect("wait for curate");
    assert!(
        out.status.success(),
        "curate must exit 0: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let json = std::fs::read_to_string(&out_path).expect("curate wrote the record");
    let _cleanup = std::fs::remove_file(&out_path);
    let meta: ChunkMeta = serde_json::from_str(&json).expect("record parses as ChunkMeta");

    assert_eq!(meta.style_cohort, Some(StyleCohort::Adjacent));
    assert!(meta.ensemble.is_none(), "single-mode chunks carry no link");
}

/// Schema v4 ensemble mode: every note-bearing track becomes a linked
/// single-part chunk with its own measured metrics, and the group record
/// carries the measured pairwise relation axes.
#[test]
fn curate_ensemble_links_parts_and_measures_relations() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/multi_track.mid");
    let stem = std::env::temp_dir().join(format!("griff_curate_ens_{}", std::process::id()));

    let mut child = Command::new(griff_bin())
        .arg("curate")
        .arg(&fixture)
        .arg("--ensemble")
        .arg("--output")
        .arg(&stem)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn griff curate --ensemble");
    child
        .stdin
        .as_mut()
        .expect("piped stdin")
        // group id, title, tuning (default), cohort (blank = core), tags, flags, decision
        .write_all(b"ens_001\nEnsemble Phrase\n\n\n\n\n\n")
        .expect("write curate answers");
    let out = child.wait_with_output().expect("wait for curate");
    assert!(
        out.status.success(),
        "ensemble curate must exit 0: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let read = |suffix: &str| {
        let path = PathBuf::from(format!("{}{suffix}", stem.display()));
        let json = std::fs::read_to_string(&path).expect("ensemble output file");
        // Cleanup is best-effort; the named binding satisfies let-underscore lints.
        let _cleanup = std::fs::remove_file(&path);
        json
    };
    let p0: ChunkMeta = serde_json::from_str(&read(".p0.chunk.json")).expect("part 0 parses");
    let p1: ChunkMeta = serde_json::from_str(&read(".p1.chunk.json")).expect("part 1 parses");
    let group: EnsembleGroup = serde_json::from_str(&read(".group.json")).expect("group parses");

    let bytes = std::fs::read(&fixture).expect("fixture bytes");
    let score = midi::import_score(&bytes).expect("fixture imports");
    let tracks: Vec<usize> = score
        .tracks
        .iter()
        .enumerate()
        .filter(|(_, t)| {
            t.voices
                .iter()
                .flat_map(|v| &v.event_groups)
                .flat_map(|g| &g.atoms)
                .any(|a| matches!(a, AtomEvent::Note(_)))
        })
        .map(|(i, _)| i)
        .collect();
    assert_eq!(tracks.len(), 2, "fixture has two note-bearing tracks");

    // Links, cohort default, ids.
    assert_eq!(p0.id, ChunkId("ens_001_p0".to_owned()));
    assert_eq!(p1.id, ChunkId("ens_001_p1".to_owned()));
    for (i, part) in [&p0, &p1].into_iter().enumerate() {
        assert_eq!(
            part.ensemble,
            Some(EnsembleRef {
                group_id: "ens_001".to_owned(),
                part_index: u32::try_from(i).unwrap(),
            })
        );
        assert_eq!(part.style_cohort, Some(StyleCohort::Core), "blank = core");
    }

    // Per-part metrics: each chunk measures its own track, not track 0 twice.
    assert_eq!(
        p0.structure,
        Some(structure::measure_structure(&score, tracks[0]).expect("p0 metrics"))
    );
    assert_eq!(
        p1.structure,
        Some(structure::measure_structure(&score, tracks[1]).expect("p1 metrics"))
    );
    assert_eq!(
        p1.gesture,
        Some(gesture::measure_gesture(&score, tracks[1]).expect("p1 gesture"))
    );

    // The group record: ordered members + measured pairwise relation axes.
    assert_eq!(group.id, "ens_001");
    assert_eq!(
        group.members,
        vec![
            ChunkId("ens_001_p0".to_owned()),
            ChunkId("ens_001_p1".to_owned()),
        ]
    );
    let expected = measure_pair_axes(&score, tracks[0], tracks[1]).expect("pair axes");
    assert_eq!(group.relations.len(), 1);
    assert_eq!(group.relations[0].parts, (0, 1));
    assert_eq!(group.relations[0].axes, expected);
}

/// Corpus schema v6: the written record also carries the measured per-axis
/// complexity profile of the same track, equal to what `measure_complexity`
/// reports. Fails to compile until `ChunkMeta.complexity` exists.
#[test]
fn curate_records_complexity_of_the_first_note_bearing_track() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/simple_4_4.mid");
    let out_path =
        std::env::temp_dir().join(format!("griff_curate_v6_{}.chunk.json", std::process::id()));

    let mut child = Command::new(griff_bin())
        .arg("curate")
        .arg(&fixture)
        .arg("--output")
        .arg(&out_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn griff curate");
    child
        .stdin
        .as_mut()
        .expect("piped stdin")
        // id, title, tuning (default), tags (none), flags (default), decision (none)
        .write_all(b"v6_001\nSchema Six\n\n\n\n\n")
        .expect("write curate answers");
    let out = child.wait_with_output().expect("wait for curate");
    assert!(
        out.status.success(),
        "curate must exit 0: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let json = std::fs::read_to_string(&out_path).expect("curate wrote the record");
    // Cleanup is best-effort; the named binding satisfies let-underscore lints.
    let _cleanup = std::fs::remove_file(&out_path);
    let meta: ChunkMeta = serde_json::from_str(&json).expect("record parses as ChunkMeta");

    let bytes = std::fs::read(&fixture).expect("fixture bytes");
    let score = midi::import_score(&bytes).expect("fixture imports");
    let track = score
        .tracks
        .iter()
        .position(|t| {
            t.voices
                .iter()
                .flat_map(|v| &v.event_groups)
                .flat_map(|g| &g.atoms)
                .any(|a| matches!(a, AtomEvent::Note(_)))
        })
        .expect("fixture has a note-bearing track");
    let expected = structure::measure_complexity(&score, track).expect("complexity measures");

    assert_eq!(
        meta.complexity,
        Some(expected),
        "curate persists the measured complexity profile"
    );
}

/// Schema v7: the rights prompt (after the reviewer decision) is recorded —
/// status, acquisition, redistributable flag, and free-form notes.
#[test]
fn curate_records_rights() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/simple_4_4.mid");
    let out_path =
        std::env::temp_dir().join(format!("griff_curate_v7_{}.chunk.json", std::process::id()));

    let mut child = Command::new(griff_bin())
        .arg("curate")
        .arg(&fixture)
        .arg("--output")
        .arg(&out_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn griff curate");
    child
        .stdin
        .as_mut()
        .expect("piped stdin")
        // id, title, tuning, cohort, tags, flags, decision (all blank/default),
        // then rights: status 0=public_domain, acquisition 2=self_transcribed,
        // redistributable 1=yes, notes.
        .write_all(b"rt_001\nRights Chunk\n\n\n\n\n\n0\n2\n1\npdmx.example/score, 2026-06-16\n")
        .expect("write curate answers");
    let out = child.wait_with_output().expect("wait for curate");
    assert!(
        out.status.success(),
        "curate must exit 0: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let json = std::fs::read_to_string(&out_path).expect("curate wrote the record");
    let _cleanup = std::fs::remove_file(&out_path);
    let meta: ChunkMeta = serde_json::from_str(&json).expect("record parses as ChunkMeta");

    assert_eq!(
        meta.rights,
        Some(RightsInfo {
            rights_status: RightsStatus::PublicDomain,
            acquisition: Acquisition::SelfTranscribed,
            redistributable: true,
            notes: "pdmx.example/score, 2026-06-16".to_owned(),
        })
    );
}

/// S4 wiring: curate persists the measured track's detected phrase boundaries
/// (no longer hardcoded empty), matching what the detector reports.
#[test]
fn curate_records_phrase_boundaries() {
    // two_phrases has detectable boundaries (unlike the single-phrase fixtures).
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/two_phrases.mid");
    let out_path = std::env::temp_dir().join(format!(
        "griff_curate_bnd_{}.chunk.json",
        std::process::id()
    ));

    let mut child = Command::new(griff_bin())
        .arg("curate")
        .arg(&fixture)
        .arg("--output")
        .arg(&out_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn griff curate");
    child
        .stdin
        .as_mut()
        .expect("piped stdin")
        .write_all(b"bnd_001\nBoundaries\n\n\n\n\n\n")
        .expect("write curate answers");
    let out = child.wait_with_output().expect("wait for curate");
    assert!(
        out.status.success(),
        "curate must exit 0: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let json = std::fs::read_to_string(&out_path).expect("curate wrote the record");
    let _cleanup = std::fs::remove_file(&out_path);
    let meta: ChunkMeta = serde_json::from_str(&json).expect("record parses as ChunkMeta");

    // Recompute with the same PPQN-scaled config the curate path applies.
    let bytes = std::fs::read(&fixture).expect("fixture bytes");
    let score = midi::import_score(&bytes).expect("fixture imports");
    let track = score
        .tracks
        .iter()
        .position(|t| {
            t.voices
                .iter()
                .flat_map(|v| &v.event_groups)
                .flat_map(|g| &g.atoms)
                .any(|a| matches!(a, AtomEvent::Note(_)))
        })
        .expect("fixture has a note-bearing track");
    let ppqn = u32::from(score.ticks_per_quarter);
    let config = boundary::BoundaryConfig {
        min_gap: Ticks(ppqn.saturating_mul(2)),
        quantize_ticks: Ticks(ppqn.checked_div(4).unwrap_or(1).max(1)),
        ..Default::default()
    };
    let expected: Vec<(u32, u32)> = boundary::detect_phrase_boundaries(&score, track, &config)
        .into_iter()
        .map(|b| (b.start_tick.0, b.end_tick.0))
        .collect();
    let got: Vec<(u32, u32)> = meta
        .boundaries
        .iter()
        .map(|b| (b.start_tick, b.end_tick))
        .collect();

    assert!(
        !got.is_empty(),
        "two_phrases fixture has detectable boundaries"
    );
    assert_eq!(got, expected, "curate persists the detector's boundaries");
}

/// `griff manifest` assembles a `CorpusManifest` from a directory of curated
/// chunk records at the current schema version.
#[test]
fn manifest_builds_from_curated_chunks() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/two_phrases.mid");
    let dir = std::env::temp_dir().join(format!("griff_manifest_{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create corpus dir");

    for id in ["m_001", "m_002"] {
        let mut child = Command::new(griff_bin())
            .arg("curate")
            .arg(&fixture)
            .arg("--output")
            .arg(dir.join(format!("{id}.chunk.json")))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn curate");
        child
            .stdin
            .as_mut()
            .expect("piped stdin")
            .write_all(format!("{id}\nChunk {id}\n\n\n\n\n\n").as_bytes())
            .expect("write curate answers");
        assert!(
            child
                .wait_with_output()
                .expect("wait for curate")
                .status
                .success(),
            "curate must exit 0"
        );
    }

    let out = Command::new(griff_bin())
        .arg("manifest")
        .arg(&dir)
        .output()
        .expect("run griff manifest");
    assert!(
        out.status.success(),
        "manifest must exit 0: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let manifest_json =
        std::fs::read_to_string(dir.join("manifest.json")).expect("manifest was written");
    let _cleanup = std::fs::remove_dir_all(&dir);
    let manifest: CorpusManifest =
        serde_json::from_str(&manifest_json).expect("manifest parses as CorpusManifest");

    assert_eq!(manifest.schema_version, SCHEMA_VERSION);
    assert_eq!(
        manifest.chunks.len(),
        2,
        "both curated chunks land in the manifest"
    );
}
