//! Slice-2 preregistered acceptance matrix — corpus completeness /
//! preservation (K1–K9, K12) and report/index publication (R1, R2, R3, R5),
//! plus the pure filesystem laws F3 and F5 (ADR-0033 Slice 2 contract §14).

mod common;

use common::{
    accept, batch_for, event, layout, lock_path_of, read_value, sha256_of_file, staging_path_of,
    walk_bytes, write_corpus, write_corpus_with_songs, write_empty_index, write_plan, Chunk,
    Layout,
};
use griff_core::corpus::{CorpusManifest, SongId};
use griff_song_curation::apply::{
    apply, report_digest, ApplicationIndex, ApplicationReport, AppliedReceipt, ApplyPaths,
    ApplyRefusal, ApplyRun, CURATED_MANIFEST_RELPATH, REPORT_RELPATH,
};
use serde_json::json;
use std::collections::BTreeMap;
use std::fs;

fn run(l: &Layout) -> ApplyRun {
    apply(&ApplyPaths {
        plan: l.plan.clone(),
        corpus: l.corpus.clone(),
        index: l.index.clone(),
        output: l.output.clone(),
    })
}

fn refuse(l: &Layout) -> ApplyRefusal {
    run(l).primary.expect_err("expected a refusal")
}

fn succeed(l: &Layout) -> AppliedReceipt {
    run(l).primary.expect("expected success")
}

const AB: &[Chunk<'static>] = &[
    Chunk {
        id: "a1",
        sha: Some("shaA"),
        song: None,
    },
    Chunk {
        id: "b1",
        sha: Some("shaB"),
        song: None,
    },
];

fn plan_accept_sha_a(l: &Layout) {
    let m = write_corpus(&l.corpus, AB);
    let b = batch_for(
        &m,
        "batch1",
        None,
        vec![event("ev0", 0, accept("g", &["shaA"], "song-000001", &[]))],
    );
    write_plan(&l.corpus, b, &l.plan);
    write_empty_index(&l.index);
}

// ── K. corpus completeness / preservation ──────────────────────────────────────

#[test]
fn k1_every_chunk_of_one_sha_updated_together() {
    let l = layout("k1");
    let m = write_corpus(
        &l.corpus,
        &[
            Chunk {
                id: "a1",
                sha: Some("shaA"),
                song: None,
            },
            Chunk {
                id: "a2",
                sha: Some("shaA"),
                song: None,
            },
        ],
    );
    let b = batch_for(
        &m,
        "batch1",
        None,
        vec![event("ev0", 0, accept("g", &["shaA"], "song-000001", &[]))],
    );
    write_plan(&l.corpus, b, &l.plan);
    write_empty_index(&l.index);
    succeed(&l);
    // Both chunk files AND both manifest records carry the label.
    for id in ["a1", "a2"] {
        let v = read_value(&l.output.join(format!("{id}.chunk.json")));
        assert_eq!(v["source"]["song_id"], json!("song-000001"), "{id} file");
    }
    let manifest = read_value(&l.output.join("manifest.json"));
    for record in manifest["chunks"].as_array().expect("chunks") {
        assert_eq!(
            record["source"]["song_id"],
            json!("song-000001"),
            "manifest record"
        );
    }
}

#[test]
fn k2_untouched_files_are_raw_byte_copies() {
    let l = layout("k2");
    plan_accept_sha_a(&l);
    // Extra corpus content the tool does not interpret.
    fs::write(l.corpus.join("g.group.json"), "{\"weird\":   [1,2 , 3]}").expect("group");
    fs::write(l.corpus.join("notes.txt"), b"\x00\xffraw bytes\n").expect("notes");
    // Non-canonical whitespace in the UNTOUCHED chunk file must survive.
    let b_text = fs::read_to_string(l.corpus.join("b1.chunk.json")).expect("read b1");
    let b_text = format!("{b_text}\n\n");
    fs::write(l.corpus.join("b1.chunk.json"), &b_text).expect("write b1");

    succeed(&l);

    for name in ["b1.chunk.json", "g.group.json", "notes.txt"] {
        assert_eq!(
            fs::read(l.corpus.join(name)).expect("in"),
            fs::read(l.output.join(name)).expect("out"),
            "{name} must be byte-identical"
        );
    }
}

#[test]
fn k3_touched_file_with_unknown_member_refuses() {
    let l = layout("k3");
    plan_accept_sha_a(&l);
    // Plant a member the core schema does not know into the TOUCHED file.
    let path = l.corpus.join("a1.chunk.json");
    let mut v = read_value(&path);
    v.as_object_mut()
        .expect("object")
        .insert("rogue_member".to_owned(), json!("smuggled"));
    fs::write(&path, v.to_string()).expect("write");

    let refusal = refuse(&l);
    assert!(
        matches!(&refusal, ApplyRefusal::NonCanonicalCorpusFile { path, .. }
            if path.contains("a1.chunk.json")),
        "got {refusal:?}"
    );
    assert!(!l.output.exists(), "nothing may be published");
}

#[test]
fn k4_root_manifest_with_songs_refuses() {
    let l = layout("k4");
    let mut songs = BTreeMap::new();
    songs.insert(SongId("song-1".to_owned()), vec!["shaA".to_owned()]);
    let m = write_corpus_with_songs(
        &l.corpus,
        &[Chunk {
            id: "a1",
            sha: Some("shaA"),
            song: Some("song-1"),
        }],
        Some(songs),
    );
    let b = batch_for(
        &m,
        "batch1",
        None,
        vec![event("ev0", 0, accept("g", &["shaA"], "song-1", &[]))],
    );
    write_plan(&l.corpus, b, &l.plan);
    write_empty_index(&l.index);
    let refusal = refuse(&l);
    assert!(
        matches!(refusal, ApplyRefusal::OrdinaryManifestCarriesSongs),
        "got {refusal:?}"
    );
}

#[test]
fn k5_tree_disagreement_and_missing_manifest_refuse() {
    // (i) the manifest lists a chunk that has no on-disk file.
    let l = layout("k5a");
    plan_accept_sha_a(&l);
    let path = l.corpus.join("manifest.json");
    let mut v = read_value(&path);
    let extra = read_value(&l.corpus.join("a1.chunk.json"));
    let mut extra = extra;
    extra["id"] = json!("ghost");
    v["chunks"].as_array_mut().expect("chunks").push(extra);
    fs::write(&path, v.to_string()).expect("write manifest");
    let refusal = refuse(&l);
    assert!(
        matches!(refusal, ApplyRefusal::CorpusTreeDisagreement { .. }),
        "got {refusal:?}"
    );

    // (ii) the root manifest is missing entirely.
    let l = layout("k5b");
    plan_accept_sha_a(&l);
    fs::remove_file(l.corpus.join("manifest.json")).expect("rm manifest");
    let refusal = refuse(&l);
    assert!(matches!(
        refusal,
        ApplyRefusal::CorpusTreeDisagreement { .. }
    ));
}

#[test]
fn k6_curated_songs_map_matches_labels_exactly() {
    let l = layout("k6");
    let m = write_corpus(
        &l.corpus,
        &[
            Chunk {
                id: "a1",
                sha: Some("shaA"),
                song: Some("song-old"),
            },
            Chunk {
                id: "b1",
                sha: Some("shaB"),
                song: None,
            },
        ],
    );
    let b = batch_for(
        &m,
        "batch1",
        None,
        vec![event("ev0", 0, accept("g", &["shaB"], "song-new", &[]))],
    );
    write_plan(&l.corpus, b, &l.plan);
    write_empty_index(&l.index);
    succeed(&l);

    let curated: CorpusManifest = serde_json::from_str(
        &fs::read_to_string(l.output.join(CURATED_MANIFEST_RELPATH)).expect("read curated"),
    )
    .expect("curated parses");
    let songs = curated.songs.expect("curated carries songs");
    let mut expected = BTreeMap::new();
    expected.insert(SongId("song-old".to_owned()), vec!["shaA".to_owned()]);
    expected.insert(SongId("song-new".to_owned()), vec!["shaB".to_owned()]);
    assert_eq!(songs, expected, "exact projection of the applied labels");
}

#[test]
fn k7_curated_manifest_at_protected_path_root_manifest_stays_songless() {
    let l = layout("k7");
    plan_accept_sha_a(&l);
    succeed(&l);
    assert!(l.output.join(CURATED_MANIFEST_RELPATH).exists());
    let root = read_value(&l.output.join("manifest.json"));
    assert!(
        root.get("songs").is_none(),
        "ordinary root manifest must never gain songs"
    );
    let curated = read_value(&l.output.join(CURATED_MANIFEST_RELPATH));
    assert!(curated.get("songs").is_some());
}

#[test]
fn k8_partial_curation_reports_not_holdout_ready() {
    let l = layout("k8");
    plan_accept_sha_a(&l); // labels only shaA; shaB stays uncurated
    let receipt = succeed(&l);
    assert!(!receipt.report.holdout_ready);
    assert_eq!(receipt.report.holdout_refusals.len(), 1);
    let refusal = &receipt.report.holdout_refusals[0];
    assert_eq!(refusal.kind, "uncurated_source");
    assert_eq!(refusal.sha256.as_deref(), Some("shaB"));
}

#[test]
fn k9_fully_curated_fixture_is_holdout_ready() {
    let l = layout("k9");
    let m = write_corpus(&l.corpus, AB);
    let b = batch_for(
        &m,
        "batch1",
        None,
        vec![
            event("ev0", 0, accept("g1", &["shaA"], "song-000001", &[])),
            event("ev1", 1, accept("g2", &["shaB"], "song-000002", &[])),
        ],
    );
    write_plan(&l.corpus, b, &l.plan);
    write_empty_index(&l.index);
    let receipt = succeed(&l);
    assert!(
        receipt.report.holdout_ready,
        "the real core preflight passes"
    );
    assert!(receipt.report.holdout_refusals.is_empty());
}

#[test]
fn k12_foreign_reserved_area_entry_refuses() {
    // (i) a foreign file directly at the reserved root.
    let l = layout("k12a");
    plan_accept_sha_a(&l);
    fs::create_dir_all(l.corpus.join("song-curation")).expect("mkdir");
    fs::write(l.corpus.join("song-curation/notes.txt"), "junk").expect("write");
    let refusal = refuse(&l);
    match &refusal {
        ApplyRefusal::CorpusTreeDisagreement { detail } => {
            assert!(detail.contains("notes.txt"), "must name the path: {detail}");
        }
        other => panic!("expected CorpusTreeDisagreement, got {other:?}"),
    }

    // (ii) a nested foreign entry — "only two files" is recursive.
    let l = layout("k12b");
    plan_accept_sha_a(&l);
    fs::create_dir_all(l.corpus.join("song-curation/extra")).expect("mkdir");
    fs::write(l.corpus.join("song-curation/extra/x.json"), "{}").expect("write");
    let refusal = refuse(&l);
    match &refusal {
        ApplyRefusal::CorpusTreeDisagreement { detail } => {
            assert!(detail.contains("extra"), "must name the path: {detail}");
        }
        other => panic!("expected CorpusTreeDisagreement, got {other:?}"),
    }
}

// ── R. report / index publication ──────────────────────────────────────────────

#[test]
fn r1_report_digest_recomputes_identically() {
    let l = layout("r1");
    plan_accept_sha_a(&l);
    let receipt = succeed(&l);
    assert_eq!(
        report_digest(&receipt.report),
        receipt.report.report_digest,
        "in-memory report digest law"
    );
    // The published artifact obeys the same law after a strict re-parse.
    let published: ApplicationReport = serde_json::from_str(
        &fs::read_to_string(l.output.join(REPORT_RELPATH)).expect("read report"),
    )
    .expect("report parses strictly");
    assert_eq!(published, receipt.report);
    assert_eq!(report_digest(&published), published.report_digest);
}

#[test]
fn r2_index_record_matches_report() {
    let l = layout("r2");
    plan_accept_sha_a(&l);
    let receipt = succeed(&l);
    let index: ApplicationIndex =
        serde_json::from_str(&fs::read_to_string(&l.index).expect("read index"))
            .expect("index parses");
    let record = &index.applications[0];
    assert_eq!(record.batch_id, receipt.report.batch_id);
    assert_eq!(record.report_digest, receipt.report.report_digest);
    assert_eq!(
        record.input_corpus_fingerprint,
        receipt.report.input_corpus_fingerprint
    );
    assert_eq!(
        record.output_corpus_fingerprint,
        receipt.report.output_corpus_fingerprint
    );
}

#[test]
fn r3_success_publishes_report_and_record_refusal_publishes_neither() {
    // Success half.
    let l = layout("r3a");
    plan_accept_sha_a(&l);
    let receipt = succeed(&l);
    assert!(l.output.join(REPORT_RELPATH).exists());
    let index: ApplicationIndex =
        serde_json::from_str(&fs::read_to_string(&l.index).expect("read")).expect("parse");
    assert_eq!(
        index.applications[0].report_digest,
        receipt.report.report_digest
    );

    // Pre-publication refusal half: neither a record nor a published tree.
    let l = layout("r3b");
    plan_accept_sha_a(&l);
    common::tamper_plan(&l.plan, |v| {
        v["plan_digest"] = json!("deadbeef");
    });
    let before = fs::read(&l.index).expect("index");
    let _ = refuse(&l);
    assert!(!l.output.exists());
    assert_eq!(fs::read(&l.index).expect("index"), before);
}

#[test]
fn r5_curated_manifest_digest_is_sha256_of_published_bytes() {
    let l = layout("r5");
    plan_accept_sha_a(&l);
    let receipt = succeed(&l);
    assert_eq!(
        sha256_of_file(&l.output.join(CURATED_MANIFEST_RELPATH)),
        receipt.report.curated_manifest_digest
    );
}

// ── F. pure filesystem laws ────────────────────────────────────────────────────

#[test]
fn f3_pre_staging_refusal_leaves_no_trace() {
    let l = layout("f3");
    let m = write_corpus(
        &l.corpus,
        &[Chunk {
            id: "a1",
            sha: Some("shaA"),
            song: Some("song-old"),
        }],
    );
    let b = batch_for(
        &m,
        "batch1",
        None,
        vec![event("ev0", 0, accept("g", &["shaA"], "song-new", &[]))],
    );
    write_plan(&l.corpus, b, &l.plan);
    write_empty_index(&l.index);
    let before = fs::read(&l.index).expect("index");
    let refusal = refuse(&l);
    assert!(matches!(
        refusal,
        ApplyRefusal::ExistingLabelReplacementNotAuthorized { .. }
    ));
    assert!(!l.output.exists(), "no output");
    assert!(!staging_path_of(&l.output).exists(), "no staging");
    assert!(!lock_path_of(&l.index).exists(), "no lock left");
    assert_eq!(
        fs::read(&l.index).expect("index"),
        before,
        "index identical"
    );
}

#[test]
fn f5_repeated_execution_is_byte_identical() {
    let make = |tag: &str| -> Layout {
        let l = layout(tag);
        let m = write_corpus(&l.corpus, AB);
        let b = batch_for(
            &m,
            "batch1",
            None,
            vec![event("ev0", 0, accept("g", &["shaA"], "song-000001", &[]))],
        );
        write_plan(&l.corpus, b, &l.plan);
        write_empty_index(&l.index);
        l
    };
    let l1 = make("f5x");
    let l2 = make("f5y");
    succeed(&l1);
    succeed(&l2);
    assert_eq!(
        walk_bytes(&l1.output),
        walk_bytes(&l2.output),
        "output trees must be byte-identical"
    );
    assert_eq!(
        fs::read(&l1.index).expect("i1"),
        fs::read(&l2.index).expect("i2"),
        "updated indexes must be byte-identical"
    );
}
