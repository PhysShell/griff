//! Adversarial self-review witnesses (implementation Phase 8).
//!
//! Each test kills one specific mutation that the preregistered §14 matrix
//! alone would let survive. They characterize behaviour the implementation
//! already provides (no new public API; green before commit), pinning it
//! against exactly the mutations named in their doc comments.

mod common;

use common::{
    accept, batch_for, event, layout, read_manifest, write_corpus, write_empty_index, write_plan,
    Chunk, Layout,
};
use griff_song_curation::apply::{
    apply, ApplicationReport, ApplyPaths, ApplyRefusal, ApplyRun, REPORT_RELPATH,
};
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

const AB: [Chunk<'static>; 2] = [
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

fn valid_setup(l: &Layout) {
    let m = write_corpus(&l.corpus, &AB);
    let b = batch_for(
        &m,
        "batch1",
        None,
        vec![event("ev0", 0, accept("g", &["shaA"], "song-000001", &[]))],
    );
    write_plan(&l.corpus, b, &l.plan);
    write_empty_index(&l.index);
}

/// Kills: removing the §5.1 schema-identity check from index validation.
#[test]
fn witness_unsupported_index_schema_refuses() {
    let l = layout("w-schema");
    valid_setup(&l);
    fs::write(
        &l.index,
        "{\"schema\":\"song-curation.applications.v99\",\"applications\":[]}",
    )
    .expect("write index");
    let refusal = run(&l).primary.expect_err("must refuse");
    assert!(
        matches!(&refusal, ApplyRefusal::UnsupportedApplicationIndexSchema { schema }
            if schema == "song-curation.applications.v99"),
        "got {refusal:?}"
    );
}

/// Kills: letting a `null` previous_application_report_digest pass §7.2
/// relation (1) against a non-empty index head.
#[test]
fn witness_null_prev_digest_against_nonempty_head_refuses() {
    let l = layout("w-nullprev");
    valid_setup(&l);
    let receipt = run(&l).primary.expect("initial apply");
    let corpus2 = l.output.clone();
    let m2 = read_manifest(&corpus2);
    // batch2 correctly fingerprinted against the head output, but prev=None.
    let b2 = batch_for(
        &m2,
        "batch2",
        None,
        vec![event("ev0", 0, accept("g2", &["shaB"], "song-000002", &[]))],
    );
    let plan2 = l.td.path.join("plan2.json");
    write_plan(&corpus2, b2, &plan2);
    let result = apply(&ApplyPaths {
        plan: plan2,
        corpus: corpus2,
        index: l.index.clone(),
        output: l.td.path.join("out2"),
    });
    let refusal = result.primary.expect_err("must refuse");
    match &refusal {
        ApplyRefusal::ApplicationChainMismatch {
            relation,
            expected,
            actual,
        } => {
            assert!(relation.contains("previous_application_report_digest"));
            assert_eq!(expected, &receipt.report.report_digest);
            assert_eq!(actual, "null");
        }
        other => panic!("expected ApplicationChainMismatch, got {other:?}"),
    }
}

/// Kills: dropping `deny_unknown_fields` from the index RECORD type — a
/// foreign field nested inside a record must refuse, not launder.
#[test]
fn witness_index_record_foreign_field_refuses() {
    let l = layout("w-recfield");
    valid_setup(&l);
    fs::write(
        &l.index,
        r#"{"schema":"song-curation.applications.v1","applications":[
            {"batch_id":"b1","report_digest":"r","input_corpus_fingerprint":"f1",
             "output_corpus_fingerprint":"f2","rogue":true}]}"#,
    )
    .expect("write index");
    let refusal = run(&l).primary.expect_err("must refuse");
    assert!(
        matches!(refusal, ApplyRefusal::MalformedApplicationIndex { .. }),
        "got {refusal:?}"
    );
}

/// Kills: dropping `deny_unknown_fields` from the report type — the
/// published artifact must stay strict for every future reader.
#[test]
fn witness_published_report_is_strict_on_the_wire() {
    let l = layout("w-report");
    valid_setup(&l);
    run(&l).primary.expect("apply");
    let text = fs::read_to_string(l.output.join(REPORT_RELPATH)).expect("read report");
    let mut value: serde_json::Value = serde_json::from_str(&text).expect("parse");
    value
        .as_object_mut()
        .expect("object")
        .insert("rogue".to_owned(), json!(1));
    let laundered = serde_json::from_value::<ApplicationReport>(value);
    assert!(
        laundered.is_err(),
        "a foreign field in the report must be rejected"
    );
}
