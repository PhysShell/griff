//! Slice-2 preregistered acceptance matrix — core cases (ADR-0033 Slice 2
//! contract §14): plan/integrity A1–A8, chain/index C1–C10, labels L1–L9.
//!
//! Every test is named after its §14 case id. These are the RED tests for the
//! Apply core: they exercise the accepted contract through the serialized
//! artifact boundary (plan file + corpus tree + index file → output tree).

mod common;

use common::{
    accept, batch_for, correct, event, layout, lock_path_of, merge, read_manifest, read_value,
    reject, split, staging_path_of, tamper_plan, write_corpus, write_empty_index, write_plan,
    Chunk, Layout,
};
use griff_song_curation::apply::{
    apply, ApplicationIndex, AppliedReceipt, ApplyPaths, ApplyRefusal, ApplyRun,
    APPLICATIONS_SCHEMA, CURATED_MANIFEST_RELPATH, REPORT_RELPATH,
};
use griff_song_curation::{decisions_digest, plan_digest, CurationError, DryRunPlan};
use serde_json::json;
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

fn read_index(l: &Layout) -> ApplicationIndex {
    serde_json::from_str(&fs::read_to_string(&l.index).expect("read index"))
        .expect("index parses strictly")
}

/// Refusal from steps 1–8 must leave nothing behind: no output, no staging,
/// no lockfile, index byte-identical.
fn assert_nothing_written(l: &Layout, index_before: &[u8]) {
    assert!(!l.output.exists(), "no output tree may exist");
    assert!(
        !staging_path_of(&l.output).exists(),
        "no staging dir may remain"
    );
    assert!(
        !lock_path_of(&l.index).exists(),
        "the lockfile must be released"
    );
    assert_eq!(
        fs::read(&l.index).expect("read index"),
        index_before,
        "the index must be byte-identical"
    );
}

fn slice1_errors(refusal: &ApplyRefusal) -> &Vec<CurationError> {
    match refusal {
        ApplyRefusal::PlanVerification { refusals } => refusals,
        other => panic!("expected PlanVerification, got {other:?}"),
    }
}

const UNCURATED_AB: &[Chunk<'static>] = &[
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

// ── A. plan / integrity ────────────────────────────────────────────────────────

#[test]
fn a1_valid_serialized_plan_applies() {
    let l = layout("a1");
    let m = write_corpus(&l.corpus, UNCURATED_AB);
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

    // Output tree published with the labels applied.
    let out_a = read_value(&l.output.join("a1.chunk.json"));
    assert_eq!(out_a["source"]["song_id"], json!("song-000001"));
    let out_b = read_value(&l.output.join("b1.chunk.json"));
    assert_eq!(out_b["source"]["song_id"], json!("song-000002"));

    // Curated manifest and report live in the reserved area.
    assert!(l.output.join(CURATED_MANIFEST_RELPATH).exists());
    assert!(l.output.join(REPORT_RELPATH).exists());

    // The index gained exactly one record binding to the report.
    let index = read_index(&l);
    assert_eq!(index.schema, APPLICATIONS_SCHEMA);
    assert_eq!(index.applications.len(), 1);
    let record = &index.applications[0];
    assert_eq!(record.batch_id, "batch1");
    assert_eq!(record.report_digest, receipt.report.report_digest);
    assert_eq!(
        record.input_corpus_fingerprint,
        receipt.report.input_corpus_fingerprint
    );
    assert_eq!(
        record.output_corpus_fingerprint,
        receipt.report.output_corpus_fingerprint
    );
    assert!(
        !lock_path_of(&l.index).exists(),
        "the lock is released after success"
    );
}

#[test]
fn a2_plan_with_foreign_field_refuses() {
    let l = layout("a2");
    let m = write_corpus(&l.corpus, UNCURATED_AB);
    let b = batch_for(
        &m,
        "batch1",
        None,
        vec![event("ev0", 0, accept("g", &["shaA"], "song-1", &[]))],
    );
    write_plan(&l.corpus, b, &l.plan);
    write_empty_index(&l.index);
    let before = fs::read(&l.index).expect("index bytes");

    tamper_plan(&l.plan, |v| common::plant_rogue(v, &[]));

    let refusal = refuse(&l);
    assert!(
        matches!(refusal, ApplyRefusal::MalformedPlanArtifact { .. }),
        "got {refusal:?}"
    );
    assert_nothing_written(&l, &before);
}

#[test]
fn a3_corrupted_plan_digest_refuses() {
    let l = layout("a3");
    let m = write_corpus(&l.corpus, UNCURATED_AB);
    let b = batch_for(
        &m,
        "batch1",
        None,
        vec![event("ev0", 0, accept("g", &["shaA"], "song-1", &[]))],
    );
    write_plan(&l.corpus, b, &l.plan);
    write_empty_index(&l.index);
    let before = fs::read(&l.index).expect("index bytes");

    tamper_plan(&l.plan, |v| {
        v["plan_digest"] = json!("deadbeef");
    });

    let refusal = refuse(&l);
    assert!(slice1_errors(&refusal)
        .iter()
        .any(|e| matches!(e, CurationError::PlanDigestMismatch { .. })));
    assert_nothing_written(&l, &before);
}

#[test]
fn a4_corrupted_decisions_digest_refuses() {
    let l = layout("a4");
    let m = write_corpus(&l.corpus, UNCURATED_AB);
    let b = batch_for(
        &m,
        "batch1",
        None,
        vec![event("ev0", 0, accept("g", &["shaA"], "song-1", &[]))],
    );
    let mut plan = write_plan(&l.corpus, b, &l.plan);
    write_empty_index(&l.index);

    plan.decisions_digest = "deadbeef".to_owned();
    plan.plan_digest = plan_digest(&plan);
    fs::write(&l.plan, serde_json::to_string_pretty(&plan).expect("ser")).expect("write");

    let refusal = refuse(&l);
    assert!(slice1_errors(&refusal)
        .iter()
        .any(|e| matches!(e, CurationError::DecisionDigestMismatch { .. })));
}

#[test]
fn a5_corpus_drift_refuses() {
    let l = layout("a5");
    let m = write_corpus(&l.corpus, UNCURATED_AB);
    let b = batch_for(
        &m,
        "batch1",
        None,
        vec![event("ev0", 0, accept("g", &["shaA"], "song-1", &[]))],
    );
    write_plan(&l.corpus, b, &l.plan);
    write_empty_index(&l.index);
    let before = fs::read(&l.index).expect("index bytes");

    // The corpus is relabelled after planning: the fingerprint drifts.
    write_corpus(
        &l.corpus,
        &[
            Chunk {
                id: "a1",
                sha: Some("shaA"),
                song: Some("song-else"),
            },
            Chunk {
                id: "b1",
                sha: Some("shaB"),
                song: None,
            },
        ],
    );

    let refusal = refuse(&l);
    assert!(slice1_errors(&refusal)
        .iter()
        .any(|e| matches!(e, CurationError::PlanCorpusFingerprintMismatch { .. })));
    assert_nothing_written(&l, &before);
}

#[test]
fn a6_forged_projection_with_self_consistent_digests_refuses() {
    let l = layout("a6");
    let m = write_corpus(&l.corpus, UNCURATED_AB);
    let b = batch_for(
        &m,
        "batch1",
        None,
        vec![event("ev0", 0, accept("g", &["shaA"], "song-1", &[]))],
    );
    let mut plan = write_plan(&l.corpus, b, &l.plan);
    write_empty_index(&l.index);

    plan.assignments[0].song_id = "song-forged".to_owned();
    plan.plan_digest = plan_digest(&plan);
    fs::write(&l.plan, serde_json::to_string_pretty(&plan).expect("ser")).expect("write");

    let refusal = refuse(&l);
    assert!(slice1_errors(&refusal)
        .iter()
        .any(|e| matches!(e, CurationError::DecisionProjectionMismatch { .. })));
}

#[test]
fn a7_invalid_embedded_batch_short_circuits_before_digests() {
    let l = layout("a7");
    let m = write_corpus(&l.corpus, UNCURATED_AB);
    let b = batch_for(
        &m,
        "batch1",
        None,
        vec![event("ev0", 0, accept("g", &["shaA"], "song-1", &[]))],
    );
    write_plan(&l.corpus, b, &l.plan);
    write_empty_index(&l.index);

    tamper_plan(&l.plan, |v| {
        v["decision_batch"]["events"][0]["ordinal"] = json!(9);
        v["decisions_digest"] = json!("corrupt");
        v["plan_digest"] = json!("corrupt");
    });

    let refusal = refuse(&l);
    let errors = slice1_errors(&refusal);
    assert!(errors
        .iter()
        .any(|e| matches!(e, CurationError::InvalidDecisionBatchOrder { .. })));
    for forbidden in [
        errors
            .iter()
            .any(|e| matches!(e, CurationError::PlanDigestMismatch { .. })),
        errors
            .iter()
            .any(|e| matches!(e, CurationError::DecisionDigestMismatch { .. })),
        errors
            .iter()
            .any(|e| matches!(e, CurationError::DecisionProjectionMismatch { .. })),
    ] {
        assert!(
            !forbidden,
            "digest/projection work must not run: {errors:?}"
        );
    }
}

/// A8: apply-time corpus/batch faults surfacing unchanged through step 5.
#[test]
fn a8_reachable_slice1_refusals_surface_through_step5() {
    // (a) a corpus chunk with no sha256 → UnidentifiedSource.
    {
        let l = layout("a8a");
        let m = write_corpus(&l.corpus, UNCURATED_AB);
        let b = batch_for(
            &m,
            "batch1",
            None,
            vec![event("ev0", 0, accept("g", &["shaA"], "song-1", &[]))],
        );
        write_plan(&l.corpus, b, &l.plan);
        write_empty_index(&l.index);
        fs::remove_dir_all(&l.corpus).expect("clear corpus");
        write_corpus(
            &l.corpus,
            &[
                Chunk {
                    id: "a1",
                    sha: Some("shaA"),
                    song: None,
                },
                Chunk {
                    id: "z1",
                    sha: None,
                    song: None,
                },
            ],
        );
        let refusal = refuse(&l);
        assert!(slice1_errors(&refusal)
            .iter()
            .any(|e| matches!(e, CurationError::UnidentifiedSource { .. })));
        assert!(!l.output.exists());
    }
    // (b) conflicting existing labels on one source.
    {
        let l = layout("a8b");
        let m = write_corpus(&l.corpus, UNCURATED_AB);
        let b = batch_for(
            &m,
            "batch1",
            None,
            vec![event("ev0", 0, accept("g", &["shaA"], "song-1", &[]))],
        );
        write_plan(&l.corpus, b, &l.plan);
        write_empty_index(&l.index);
        fs::remove_dir_all(&l.corpus).expect("clear corpus");
        write_corpus(
            &l.corpus,
            &[
                Chunk {
                    id: "a1",
                    sha: Some("shaA"),
                    song: Some("song-1"),
                },
                Chunk {
                    id: "a2",
                    sha: Some("shaA"),
                    song: Some("song-2"),
                },
            ],
        );
        let refusal = refuse(&l);
        assert!(slice1_errors(&refusal)
            .iter()
            .any(|e| matches!(e, CurationError::ConflictingExistingSongIds { .. })));
    }
    // (c)–(e): tamper the embedded batch with recomputed digests, so the fault
    // is the only refusal source.
    let tampered = |tag: &str, mutate: fn(&mut DryRunPlan)| -> ApplyRefusal {
        let l = layout(tag);
        let m = write_corpus(&l.corpus, UNCURATED_AB);
        let b = batch_for(
            &m,
            "batch1",
            None,
            vec![event("ev0", 0, accept("g", &["shaA"], "song-1", &[]))],
        );
        write_plan(&l.corpus, b, &l.plan);
        write_empty_index(&l.index);
        let text = fs::read_to_string(&l.plan).expect("read plan");
        let mut plan: DryRunPlan = serde_json::from_str(&text).expect("parse plan");
        mutate(&mut plan);
        plan.decisions_digest = decisions_digest(&plan.decision_batch);
        plan.plan_digest = plan_digest(&plan);
        fs::write(&l.plan, serde_json::to_string_pretty(&plan).expect("ser")).expect("write");
        refuse(&l)
    };
    // (c) a decision naming an unknown source.
    let r = tampered("a8c", |plan| {
        plan.decision_batch
            .events
            .push(event("ev1", 1, accept("h", &["shaZ"], "song-2", &[])));
    });
    assert!(slice1_errors(&r)
        .iter()
        .any(|e| matches!(e, CurationError::UnknownDecisionSource { .. })));
    // (d) a split assigning one source two labels.
    let r = tampered("a8d", |plan| {
        plan.decision_batch.events.push(event(
            "ev1",
            1,
            split(
                "song-old",
                &[("song-1", &["shaB"]), ("song-2", &["shaB"])],
                &["song-old"],
            ),
        ));
    });
    assert!(slice1_errors(&r)
        .iter()
        .any(|e| matches!(e, CurationError::SourceAssignedToMultipleSongs { .. })));
    // (e) a duplicate event_id in the embedded batch (short-circuits).
    let r = tampered("a8e", |plan| {
        plan.decision_batch
            .events
            .push(event("ev0", 1, accept("h", &["shaB"], "song-2", &[])));
    });
    let errors = slice1_errors(&r);
    assert!(errors
        .iter()
        .any(|e| matches!(e, CurationError::DuplicateDecisionEventId { .. })));
    assert!(!errors
        .iter()
        .any(|e| matches!(e, CurationError::DecisionProjectionMismatch { .. })));
}

// ── C. chain / index ───────────────────────────────────────────────────────────

/// Apply one accept of `shaA` on a fresh A/B corpus; returns the receipt.
fn apply_initial(l: &Layout) -> AppliedReceipt {
    let m = write_corpus(&l.corpus, UNCURATED_AB);
    let b = batch_for(
        &m,
        "batch1",
        None,
        vec![event("ev0", 0, accept("g1", &["shaA"], "song-000001", &[]))],
    );
    write_plan(&l.corpus, b, &l.plan);
    write_empty_index(&l.index);
    succeed(l)
}

#[test]
fn c1_valid_initial_application() {
    let l = layout("c1");
    let receipt = apply_initial(&l);
    assert_eq!(receipt.report.batch_id, "batch1");
    assert_eq!(receipt.report.previous_application_report_digest, None);
    let index = read_index(&l);
    assert_eq!(index.applications.len(), 1);
}

#[test]
fn c2_valid_second_application_chained_to_first() {
    let l = layout("c2");
    let receipt1 = apply_initial(&l);

    // The second input is the first apply's REAL published output tree,
    // reserved area included.
    let corpus2 = l.output.clone();
    let m2 = read_manifest(&corpus2);
    let b2 = batch_for(
        &m2,
        "batch2",
        Some(&receipt1.report.report_digest),
        vec![event("ev0", 0, accept("g2", &["shaB"], "song-000002", &[]))],
    );
    let plan2 = l.td.path.join("plan2.json");
    write_plan(&corpus2, b2, &plan2);
    let output2 = l.td.path.join("out2");
    let run2 = apply(&ApplyPaths {
        plan: plan2,
        corpus: corpus2,
        index: l.index.clone(),
        output: output2.clone(),
    });
    let receipt2 = run2.primary.expect("second apply succeeds");

    let index = read_index(&l);
    assert_eq!(index.applications.len(), 2, "two ordered records");
    assert_eq!(index.applications[0].batch_id, "batch1");
    assert_eq!(index.applications[1].batch_id, "batch2");
    assert_eq!(
        receipt2.report.previous_application_report_digest,
        Some(receipt1.report.report_digest.clone())
    );

    // The second output's reserved area holds exactly the SECOND
    // application's artifacts — the first's are superseded, not raw-copied.
    let reserved = output2.join("song-curation");
    let mut entries: Vec<String> = fs::read_dir(&reserved)
        .expect("reserved area")
        .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
        .collect();
    entries.sort();
    assert_eq!(entries, vec!["apply-report.json", "manifest.json"]);
    let report2 = read_value(&output2.join(REPORT_RELPATH));
    assert_eq!(report2["batch_id"], json!("batch2"));
    assert_eq!(
        report2["report_digest"],
        json!(receipt2.report.report_digest)
    );
}

#[test]
fn c3_duplicate_batch_id_refuses_as_already_applied_not_chain() {
    let l = layout("c3");
    apply_initial(&l);

    // Re-apply the very same plan file to a fresh output path.
    let output2 = l.td.path.join("out2");
    let run2 = apply(&ApplyPaths {
        plan: l.plan.clone(),
        corpus: l.corpus.clone(),
        index: l.index.clone(),
        output: output2.clone(),
    });
    let refusal = run2.primary.expect_err("must refuse");
    assert!(
        matches!(&refusal, ApplyRefusal::DecisionBatchAlreadyApplied { batch_id } if batch_id == "batch1"),
        "already-applied must win over the chain refusal, got {refusal:?}"
    );
    assert!(!output2.exists());
}

#[test]
fn c4_wrong_previous_report_digest_refuses() {
    let l = layout("c4");
    apply_initial(&l);
    let corpus2 = l.output.clone();
    let m2 = read_manifest(&corpus2);
    let b2 = batch_for(
        &m2,
        "batch2",
        Some("deadbeef-wrong"),
        vec![event("ev0", 0, accept("g2", &["shaB"], "song-000002", &[]))],
    );
    let plan2 = l.td.path.join("plan2.json");
    write_plan(&corpus2, b2, &plan2);
    let run2 = apply(&ApplyPaths {
        plan: plan2,
        corpus: corpus2,
        index: l.index.clone(),
        output: l.td.path.join("out2"),
    });
    let refusal = run2.primary.expect_err("must refuse");
    match &refusal {
        ApplyRefusal::ApplicationChainMismatch { relation, .. } => {
            assert!(
                relation.contains("previous_application_report_digest"),
                "relation (1) must be named, got {relation}"
            );
        }
        other => panic!("expected ApplicationChainMismatch, got {other:?}"),
    }
}

#[test]
fn c5_wrong_chained_corpus_fingerprint_refuses() {
    let l = layout("c5");
    let receipt1 = apply_initial(&l);

    // A second batch planned against the ORIGINAL corpus (stale head).
    let m = read_manifest(&l.corpus);
    let b2 = batch_for(
        &m,
        "batch2",
        Some(&receipt1.report.report_digest),
        vec![event("ev0", 0, accept("g2", &["shaB"], "song-000002", &[]))],
    );
    let plan2 = l.td.path.join("plan2.json");
    write_plan(&l.corpus, b2, &plan2);
    let run2 = apply(&ApplyPaths {
        plan: plan2,
        corpus: l.corpus.clone(),
        index: l.index.clone(),
        output: l.td.path.join("out2"),
    });
    let refusal = run2.primary.expect_err("must refuse");
    assert!(
        matches!(refusal, ApplyRefusal::ApplicationChainMismatch { .. }),
        "got {refusal:?}"
    );
}

#[test]
fn c6_fingerprint_neutral_batch_applies_then_refuses_by_id() {
    let l = layout("c6");
    let m = write_corpus(&l.corpus, UNCURATED_AB);
    let b = batch_for(
        &m,
        "batch1",
        None,
        vec![event("ev0", 0, reject("g1", &["shaA"]))],
    );
    write_plan(&l.corpus, b, &l.plan);
    write_empty_index(&l.index);

    let receipt = succeed(&l);
    assert_eq!(
        receipt.report.input_corpus_fingerprint, receipt.report.output_corpus_fingerprint,
        "reject-only batch is fingerprint-neutral"
    );
    assert_eq!(receipt.report.assignments_applied, 0);
    assert_eq!(receipt.report.assignments_unchanged, 0);
    assert_eq!(receipt.report.sources_reviewed_unassigned, 1);
    assert_eq!(receipt.report.sources_untouched, 1);
    assert_eq!(read_index(&l).applications.len(), 1);

    // The corpus shows no side effect, but the index still proves application.
    let run2 = apply(&ApplyPaths {
        plan: l.plan.clone(),
        corpus: l.corpus.clone(),
        index: l.index.clone(),
        output: l.td.path.join("out2"),
    });
    let refusal = run2.primary.expect_err("must refuse");
    assert!(matches!(
        refusal,
        ApplyRefusal::DecisionBatchAlreadyApplied { .. }
    ));
}

#[test]
fn c7_missing_index_and_foreign_field_refuse() {
    // (i) missing index file: refused at step 1, before the lock.
    let l = layout("c7a");
    let m = write_corpus(&l.corpus, UNCURATED_AB);
    let b = batch_for(
        &m,
        "batch1",
        None,
        vec![event("ev0", 0, accept("g", &["shaA"], "song-1", &[]))],
    );
    write_plan(&l.corpus, b, &l.plan);
    // no index written
    let refusal = refuse(&l);
    assert!(
        matches!(refusal, ApplyRefusal::MalformedApplicationIndex { .. }),
        "got {refusal:?}"
    );
    assert!(
        !lock_path_of(&l.index).exists(),
        "no lockfile may be created for a missing index"
    );
    assert!(!l.output.exists());

    // (ii) foreign field in the index document (step 2).
    let l = layout("c7b");
    let m = write_corpus(&l.corpus, UNCURATED_AB);
    let b = batch_for(
        &m,
        "batch1",
        None,
        vec![event("ev0", 0, accept("g", &["shaA"], "song-1", &[]))],
    );
    write_plan(&l.corpus, b, &l.plan);
    fs::write(
        &l.index,
        "{\"schema\":\"song-curation.applications.v1\",\"applications\":[],\"rogue\":1}",
    )
    .expect("write index");
    let refusal = refuse(&l);
    assert!(matches!(
        refusal,
        ApplyRefusal::MalformedApplicationIndex { .. }
    ));
    assert!(!lock_path_of(&l.index).exists(), "lock released");
}

#[test]
fn c8_duplicate_internal_batch_id_refuses() {
    let l = layout("c8");
    let m = write_corpus(&l.corpus, UNCURATED_AB);
    let b = batch_for(
        &m,
        "batch1",
        None,
        vec![event("ev0", 0, accept("g", &["shaA"], "song-1", &[]))],
    );
    write_plan(&l.corpus, b, &l.plan);
    fs::write(
        &l.index,
        r#"{"schema":"song-curation.applications.v1","applications":[
            {"batch_id":"dup","report_digest":"r1","input_corpus_fingerprint":"f1","output_corpus_fingerprint":"f2"},
            {"batch_id":"dup","report_digest":"r2","input_corpus_fingerprint":"f2","output_corpus_fingerprint":"f3"}]}"#,
    )
    .expect("write index");
    let refusal = refuse(&l);
    assert!(
        matches!(&refusal, ApplyRefusal::DuplicateAppliedBatchId { batch_id } if batch_id == "dup"),
        "got {refusal:?}"
    );
}

#[test]
fn c9_index_with_broken_internal_chain_refuses() {
    let l = layout("c9");
    let m = write_corpus(&l.corpus, UNCURATED_AB);
    let b = batch_for(
        &m,
        "batch1",
        None,
        vec![event("ev0", 0, accept("g", &["shaA"], "song-1", &[]))],
    );
    write_plan(&l.corpus, b, &l.plan);
    fs::write(
        &l.index,
        r#"{"schema":"song-curation.applications.v1","applications":[
            {"batch_id":"b1","report_digest":"r1","input_corpus_fingerprint":"f1","output_corpus_fingerprint":"f2"},
            {"batch_id":"b2","report_digest":"r2","input_corpus_fingerprint":"f9","output_corpus_fingerprint":"f3"}]}"#,
    )
    .expect("write index");
    let refusal = refuse(&l);
    match &refusal {
        ApplyRefusal::ApplicationIndexChainInvalid { position, .. } => {
            assert_eq!(*position, 1);
        }
        other => panic!("expected ApplicationIndexChainInvalid, got {other:?}"),
    }
}

#[test]
fn c10_initial_batch_on_independent_copy_with_own_index_applies() {
    let l = layout("c10");
    apply_initial(&l);

    // A byte-identical copy of the corpus with its OWN fresh empty index is
    // an independent lineage: the same plan applies there too.
    let corpus2 = l.td.path.join("corpus2");
    fs::create_dir_all(&corpus2).expect("mkdir");
    for entry in fs::read_dir(&l.corpus).expect("read corpus") {
        let p = entry.expect("entry").path();
        fs::copy(&p, corpus2.join(p.file_name().expect("name"))).expect("copy");
    }
    let index2 = l.td.path.join("index2.json");
    write_empty_index(&index2);
    let run2 = apply(&ApplyPaths {
        plan: l.plan.clone(),
        corpus: corpus2,
        index: index2,
        output: l.td.path.join("out2"),
    });
    assert!(run2.primary.is_ok(), "independent lineage must apply");
}

// ── L. labels ─────────────────────────────────────────────────────────────────

#[test]
fn l1_assign_unlabelled_source_updates_every_chunk() {
    let l = layout("l1");
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
        vec![event("ev0", 0, accept("g", &["shaA"], "song-000001", &[]))],
    );
    write_plan(&l.corpus, b, &l.plan);
    write_empty_index(&l.index);
    let receipt = succeed(&l);
    assert_eq!(receipt.report.assignments_applied, 1);
    assert_eq!(receipt.report.assignments_unchanged, 0);
    for id in ["a1", "a2"] {
        let v = read_value(&l.output.join(format!("{id}.chunk.json")));
        assert_eq!(v["source"]["song_id"], json!("song-000001"), "{id}");
    }
    let v = read_value(&l.output.join("b1.chunk.json"));
    assert_eq!(v["source"].get("song_id"), None, "untouched stays None");
}

#[test]
fn l2_already_correct_label_counts_unchanged() {
    let l = layout("l2");
    let m = write_corpus(
        &l.corpus,
        &[Chunk {
            id: "a1",
            sha: Some("shaA"),
            song: Some("song-1"),
        }],
    );
    let b = batch_for(
        &m,
        "batch1",
        None,
        vec![event("ev0", 0, accept("g", &["shaA"], "song-1", &[]))],
    );
    write_plan(&l.corpus, b, &l.plan);
    write_empty_index(&l.index);
    let receipt = succeed(&l);
    assert_eq!(receipt.report.assignments_applied, 0);
    assert_eq!(receipt.report.assignments_unchanged, 1);
    let v = read_value(&l.output.join("a1.chunk.json"));
    assert_eq!(v["source"]["song_id"], json!("song-1"));
}

#[test]
fn l3_authorized_correct_replaces_label() {
    let l = layout("l3");
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
        vec![event(
            "ev0",
            0,
            correct(&["shaA"], "song-new", &["song-old"]),
        )],
    );
    write_plan(&l.corpus, b, &l.plan);
    write_empty_index(&l.index);
    let receipt = succeed(&l);
    assert_eq!(receipt.report.assignments_applied, 1);
    let v = read_value(&l.output.join("a1.chunk.json"));
    assert_eq!(v["source"]["song_id"], json!("song-new"));
}

#[test]
fn l4_authorized_merge_replaces_labels() {
    let l = layout("l4");
    let m = write_corpus(
        &l.corpus,
        &[
            Chunk {
                id: "a1",
                sha: Some("shaA"),
                song: Some("song-1"),
            },
            Chunk {
                id: "b1",
                sha: Some("shaB"),
                song: Some("song-2"),
            },
        ],
    );
    let b = batch_for(
        &m,
        "batch1",
        None,
        vec![event(
            "ev0",
            0,
            merge(
                &["song-1", "song-2"],
                "song-1",
                &["shaA", "shaB"],
                &["song-1", "song-2"],
            ),
        )],
    );
    write_plan(&l.corpus, b, &l.plan);
    write_empty_index(&l.index);
    let receipt = succeed(&l);
    assert_eq!(receipt.report.assignments_applied, 1, "shaB changed");
    assert_eq!(
        receipt.report.assignments_unchanged, 1,
        "shaA already song-1"
    );
    let v = read_value(&l.output.join("b1.chunk.json"));
    assert_eq!(v["source"]["song_id"], json!("song-1"));
}

#[test]
fn l5_authorized_split_replaces_labels() {
    let l = layout("l5");
    let m = write_corpus(
        &l.corpus,
        &[
            Chunk {
                id: "a1",
                sha: Some("shaA"),
                song: Some("song-1"),
            },
            Chunk {
                id: "b1",
                sha: Some("shaB"),
                song: Some("song-1"),
            },
        ],
    );
    let b = batch_for(
        &m,
        "batch1",
        None,
        vec![event(
            "ev0",
            0,
            split(
                "song-1",
                &[("song-2", &["shaA"]), ("song-3", &["shaB"])],
                &["song-1"],
            ),
        )],
    );
    write_plan(&l.corpus, b, &l.plan);
    write_empty_index(&l.index);
    let receipt = succeed(&l);
    assert_eq!(receipt.report.assignments_applied, 2);
    let a = read_value(&l.output.join("a1.chunk.json"));
    assert_eq!(a["source"]["song_id"], json!("song-2"));
    let b1 = read_value(&l.output.join("b1.chunk.json"));
    assert_eq!(b1["source"]["song_id"], json!("song-3"));
}

#[test]
fn l6_unauthorized_replacement_refuses() {
    // (i) accept over an existing label: empty supersession set.
    let l = layout("l6a");
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
    let before = fs::read(&l.index).expect("index bytes");
    let refusal = refuse(&l);
    match &refusal {
        ApplyRefusal::ExistingLabelReplacementNotAuthorized {
            source_sha256,
            on_disk_song_id,
            new_song_id,
            event_id,
        } => {
            assert_eq!(source_sha256, "shaA");
            assert_eq!(on_disk_song_id, "song-old");
            assert_eq!(new_song_id, "song-new");
            assert_eq!(event_id, "ev0");
        }
        other => panic!("expected ExistingLabelReplacementNotAuthorized, got {other:?}"),
    }
    assert_nothing_written(&l, &before);

    // (ii) correct whose supersession set does not cover the on-disk label.
    let l = layout("l6b");
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
        vec![event(
            "ev0",
            0,
            correct(&["shaA"], "song-new", &["song-other"]),
        )],
    );
    write_plan(&l.corpus, b, &l.plan);
    write_empty_index(&l.index);
    let refusal = refuse(&l);
    assert!(matches!(
        refusal,
        ApplyRefusal::ExistingLabelReplacementNotAuthorized { .. }
    ));
}

#[test]
fn l7_on_disk_label_drift_refuses_as_fingerprint_mismatch() {
    let l = layout("l7");
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
        vec![event(
            "ev0",
            0,
            correct(&["shaA"], "song-new", &["song-old"]),
        )],
    );
    write_plan(&l.corpus, b, &l.plan);
    write_empty_index(&l.index);

    // The corpus is relabelled between planning and apply.
    write_corpus(
        &l.corpus,
        &[Chunk {
            id: "a1",
            sha: Some("shaA"),
            song: Some("song-else"),
        }],
    );
    let refusal = refuse(&l);
    assert!(
        slice1_errors(&refusal)
            .iter()
            .any(|e| matches!(e, CurationError::PlanCorpusFingerprintMismatch { .. })),
        "drift is excluded by the fingerprint (§7.4 note)"
    );
}

#[test]
fn l8_merge_and_split_supersession_mismatch_refuse() {
    // merge whose supersedes ≠ from_song_ids.
    let l = layout("l8a");
    let m = write_corpus(
        &l.corpus,
        &[
            Chunk {
                id: "a1",
                sha: Some("shaA"),
                song: Some("song-1"),
            },
            Chunk {
                id: "b1",
                sha: Some("shaB"),
                song: Some("song-2"),
            },
        ],
    );
    let b = batch_for(
        &m,
        "batch1",
        None,
        vec![event(
            "ev0",
            0,
            merge(
                &["song-1", "song-2"],
                "song-1",
                &["shaA", "shaB"],
                &["song-1"],
            ),
        )],
    );
    write_plan(&l.corpus, b, &l.plan);
    write_empty_index(&l.index);
    let refusal = refuse(&l);
    assert!(
        matches!(&refusal, ApplyRefusal::SupersessionEvidenceContradiction { event_id, .. } if event_id == "ev0"),
        "got {refusal:?}"
    );

    // split whose supersedes ≠ [from_song_id].
    let l = layout("l8b");
    let m = write_corpus(
        &l.corpus,
        &[Chunk {
            id: "a1",
            sha: Some("shaA"),
            song: Some("song-1"),
        }],
    );
    let b = batch_for(
        &m,
        "batch1",
        None,
        vec![event(
            "ev0",
            0,
            split("song-1", &[("song-2", &["shaA"])], &[]),
        )],
    );
    write_plan(&l.corpus, b, &l.plan);
    write_empty_index(&l.index);
    let refusal = refuse(&l);
    assert!(matches!(
        refusal,
        ApplyRefusal::SupersessionEvidenceContradiction { .. }
    ));
}

#[test]
fn l9_accept_with_nonempty_supersedes_refuses() {
    let l = layout("l9");
    let m = write_corpus(&l.corpus, UNCURATED_AB);
    let b = batch_for(
        &m,
        "batch1",
        None,
        vec![event(
            "ev0",
            0,
            accept("g", &["shaA"], "song-1", &["song-x"]),
        )],
    );
    write_plan(&l.corpus, b, &l.plan);
    write_empty_index(&l.index);
    let refusal = refuse(&l);
    assert!(
        matches!(
            refusal,
            ApplyRefusal::SupersessionEvidenceContradiction { .. }
        ),
        "got {refusal:?}"
    );
}
