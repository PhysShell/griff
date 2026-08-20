//! Slice-2 preregistered acceptance matrix — filesystem / transactional
//! adversarial cases (ADR-0033 Slice 2 contract §14): F1, F2, F4, F6–F13,
//! C11–C16, K10, K11, R4.
//!
//! Fault points come from the deterministic `apply::fault` harness; the
//! concurrency cases run the second applier inline inside a hook on the same
//! thread, so nothing depends on scheduler timing.

mod common;

use common::{
    accept, batch_for, event, layout, lock_path_of, read_value, staging_path_of, temp_path_of,
    write_corpus, write_empty_index, write_plan, Chunk, Layout, EMPTY_INDEX,
};
use griff_song_curation::apply::{apply, fault, ApplyPaths, ApplyRefusal, ApplyRun, LOCK_MARKER};
use std::cell::RefCell;
use std::fs;
use std::io;
use std::path::Path;
use std::rc::Rc;

fn run(l: &Layout) -> ApplyRun {
    run_paths(&l.plan, &l.corpus, &l.index, &l.output)
}

fn run_paths(plan: &Path, corpus: &Path, index: &Path, output: &Path) -> ApplyRun {
    apply(&ApplyPaths {
        plan: plan.to_path_buf(),
        corpus: corpus.to_path_buf(),
        index: index.to_path_buf(),
        output: output.to_path_buf(),
    })
}

fn refuse(l: &Layout) -> ApplyRefusal {
    run(l).primary.expect_err("expected a refusal")
}

/// Standard valid setup: A/B corpus, one accept of shaA, empty index.
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

fn io_fail() -> io::Result<()> {
    Err(io::Error::other("injected fault"))
}

// ── F. filesystem preflight and protocol ───────────────────────────────────────

#[test]
fn f1_output_inside_input_refuses_including_symlink_aliases() {
    // (a) output textually inside the corpus root.
    let l = layout("f1a");
    valid_setup(&l);
    let output = l.corpus.join("out");
    let result = run_paths(&l.plan, &l.corpus, &l.index, &output);
    let refusal = result.primary.expect_err("must refuse");
    assert!(
        matches!(refusal, ApplyRefusal::OutputWouldModifyInput { .. }),
        "got {refusal:?}"
    );
    assert!(!output.exists());

    // (b) the same containment reached only through a symlink alias of the
    // corpus root.
    let l = layout("f1b");
    valid_setup(&l);
    let alias = l.td.path.join("corpus-alias");
    std::os::unix::fs::symlink(&l.corpus, &alias).expect("symlink");
    let output = l.corpus.join("out2");
    let result = run_paths(&l.plan, &alias, &l.index, &output);
    let refusal = result.primary.expect_err("must refuse");
    assert!(
        matches!(refusal, ApplyRefusal::OutputWouldModifyInput { .. }),
        "canonicalization must see through the alias, got {refusal:?}"
    );
}

#[test]
fn f2_pre_existing_output_or_staging_refuses() {
    // (a) pre-existing (non-empty) output.
    let l = layout("f2a");
    valid_setup(&l);
    fs::create_dir_all(l.output.join("keep")).expect("mk output");
    fs::write(l.output.join("keep/x"), "x").expect("occupant");
    let refusal = refuse(&l);
    assert!(
        matches!(refusal, ApplyRefusal::OutputAlreadyExists { .. }),
        "got {refusal:?}"
    );
    assert!(l.output.join("keep/x").exists(), "occupant untouched");

    // (b) pre-existing staging directory.
    let l = layout("f2b");
    valid_setup(&l);
    fs::create_dir_all(staging_path_of(&l.output)).expect("mk staging");
    let refusal = refuse(&l);
    match &refusal {
        ApplyRefusal::OutputAlreadyExists { path } => {
            assert!(path.contains(".apply-staging"), "carries the staging path");
        }
        other => panic!("expected OutputAlreadyExists, got {other:?}"),
    }
}

#[test]
fn f4_injected_failures_land_in_enumerated_states() {
    // (a) staging write failure: nothing published, staging cleaned.
    let l = layout("f4a");
    valid_setup(&l);
    let before = fs::read(&l.index).expect("index");
    fault::set("stage:write", io_fail);
    let refusal = refuse(&l);
    fault::clear();
    assert!(matches!(refusal, ApplyRefusal::ApplyIoError { .. }));
    assert!(!l.output.exists());
    assert!(!staging_path_of(&l.output).exists(), "cleanup ran");
    assert_eq!(fs::read(&l.index).expect("index"), before);

    // (b) failure of the step-11 publication rename itself: output absent,
    // index unchanged; staging may remain only if cleanup also failed.
    let l = layout("f4b");
    valid_setup(&l);
    let before = fs::read(&l.index).expect("index");
    fault::set("publish:rename", io_fail);
    let refusal = refuse(&l);
    fault::clear();
    assert!(matches!(refusal, ApplyRefusal::ApplyIoError { .. }));
    assert!(!l.output.exists());
    assert_eq!(fs::read(&l.index).expect("index"), before);

    // (c) temp-write failure inside step 12: the output is already
    // published, the old index is unchanged — provably not applied.
    let l = layout("f4c");
    valid_setup(&l);
    let before = fs::read(&l.index).expect("index");
    fault::set("commit:temp_write", io_fail);
    let refusal = refuse(&l);
    fault::clear();
    assert!(matches!(refusal, ApplyRefusal::ApplyIoError { .. }));
    assert!(l.output.exists(), "published orphan (§8.2)");
    assert_eq!(fs::read(&l.index).expect("index"), before, "not applied");

    // (d) commit-rename failure: same state; the run's own temp is removed.
    let l = layout("f4d");
    valid_setup(&l);
    let before = fs::read(&l.index).expect("index");
    fault::set("commit:rename", io_fail);
    let refusal = refuse(&l);
    fault::clear();
    assert!(matches!(refusal, ApplyRefusal::ApplyIoError { .. }));
    assert!(l.output.exists(), "published orphan (§8.2)");
    assert_eq!(fs::read(&l.index).expect("index"), before, "not applied");
    assert!(!temp_path_of(&l.index).exists(), "own temp removed");
}

#[test]
fn f6_index_inside_corpus_tree_refuses() {
    let l = layout("f6");
    let m = write_corpus(&l.corpus, &AB);
    let b = batch_for(
        &m,
        "batch1",
        None,
        vec![event("ev0", 0, accept("g", &["shaA"], "song-000001", &[]))],
    );
    write_plan(&l.corpus, b, &l.plan);
    let index = l.corpus.join("index.json");
    write_empty_index(&index);
    let result = run_paths(&l.plan, &l.corpus, &index, &l.output);
    let refusal = result.primary.expect_err("must refuse");
    assert!(
        matches!(refusal, ApplyRefusal::ApplicationIndexInsideTree { .. }),
        "got {refusal:?}"
    );
    assert!(!l.output.exists());
}

#[test]
fn f7_path_like_batch_id_influences_no_filesystem_path() {
    let l = layout("f7");
    let m = write_corpus(&l.corpus, &AB);
    let b = batch_for(
        &m,
        "../griff-f7-escape",
        None,
        vec![event("ev0", 0, accept("g", &["shaA"], "song-000001", &[]))],
    );
    write_plan(&l.corpus, b, &l.plan);
    write_empty_index(&l.index);
    let result = run(&l);
    assert!(
        result.primary.is_ok(),
        "the apply proceeds purely by contract law"
    );
    for escape in [
        l.td.path.join("griff-f7-escape"),
        l.td.path.parent().expect("parent").join("griff-f7-escape"),
    ] {
        assert!(
            !escape.exists(),
            "no path derived from batch_id: {escape:?}"
        );
    }
}

#[test]
fn f8_index_symlink_aliases_converge_on_the_canonical_file() {
    let l = layout("f8");
    valid_setup(&l);
    let alias = l.td.path.join("alias.json");
    std::os::unix::fs::symlink(&l.index, &alias).expect("symlink");

    // Contention through another alias contends on the canonical lock.
    fs::write(lock_path_of(&l.index), LOCK_MARKER).expect("hold canonical lock");
    let held = run_paths(&l.plan, &l.corpus, &alias, &l.output);
    let refusal = held.primary.expect_err("must contend");
    assert!(
        matches!(refusal, ApplyRefusal::ApplicationIndexLocked { .. }),
        "alias must derive the canonical lock, got {refusal:?}"
    );
    fs::remove_file(lock_path_of(&l.index)).expect("release");

    // Applying through the alias commits over the REAL file; the alias still
    // resolves to the updated index.
    let result = run_paths(&l.plan, &l.corpus, &alias, &l.output);
    assert!(result.primary.is_ok(), "{:?}", result.primary);
    let real = read_value(&l.index);
    assert_eq!(real["applications"].as_array().expect("apps").len(), 1);
    assert!(
        !alias.symlink_metadata().expect("alias meta").is_file(),
        "the alias entry remains a symlink — the rename replaced the real file"
    );
    let via_alias = read_value(&alias);
    assert_eq!(via_alias, real);
}

#[test]
fn f9_lock_release_failure_is_a_warning_never_the_primary_outcome() {
    // (a) after a committed apply: success stays success.
    let l = layout("f9a");
    valid_setup(&l);
    fault::set("lock:release", io_fail);
    let result = run(&l);
    fault::clear();
    assert!(result.primary.is_ok(), "committed apply stays applied");
    let warning = result.lock_release_warning.expect("warning attached");
    assert!(warning.lockfile.contains(".lock"));
    assert!(
        lock_path_of(&l.index).exists(),
        "the stale lock remains for §8.2 recovery"
    );
    fs::remove_file(lock_path_of(&l.index)).expect("operator recovery");

    // (b) after a refusal: the original refusal stays primary.
    let l = layout("f9b");
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
    fault::set("lock:release", io_fail);
    let result = run(&l);
    fault::clear();
    let refusal = result.primary.expect_err("refusal preserved");
    assert!(
        matches!(
            refusal,
            ApplyRefusal::ExistingLabelReplacementNotAuthorized { .. }
        ),
        "never masked by the release failure, got {refusal:?}"
    );
    assert!(result.lock_release_warning.is_some());
    let _ = fs::remove_file(lock_path_of(&l.index));
}

#[test]
fn f10_output_colliding_with_index_artifacts_refuses_before_the_lock() {
    // (a) output equal to the lock coordination path.
    let l = layout("f10a");
    valid_setup(&l);
    let output = lock_path_of(&l.index);
    let result = run_paths(&l.plan, &l.corpus, &l.index, &output);
    let refusal = result.primary.expect_err("must refuse");
    assert!(
        matches!(
            refusal,
            ApplyRefusal::OutputCollidesWithIndexArtifacts { .. }
        ),
        "got {refusal:?}"
    );
    assert!(
        !output.exists(),
        "the lock must never be created at a declared output path"
    );

    // (b) output equal to the temp coordination path.
    let l = layout("f10b");
    valid_setup(&l);
    let output = temp_path_of(&l.index);
    let result = run_paths(&l.plan, &l.corpus, &l.index, &output);
    let refusal = result.primary.expect_err("must refuse");
    assert!(matches!(
        refusal,
        ApplyRefusal::OutputCollidesWithIndexArtifacts { .. }
    ));

    // (c) staging equal to the canonical index file: an index deliberately
    // named like a staging sibling.
    let l = layout("f10c");
    let m = write_corpus(&l.corpus, &AB);
    let b = batch_for(
        &m,
        "batch1",
        None,
        vec![event("ev0", 0, accept("g", &["shaA"], "song-000001", &[]))],
    );
    write_plan(&l.corpus, b, &l.plan);
    let index = l.td.path.join(".out.apply-staging");
    write_empty_index(&index);
    let before = fs::read(&index).expect("index");
    let result = run_paths(&l.plan, &l.corpus, &index, &l.output);
    let refusal = result.primary.expect_err("must refuse");
    assert!(matches!(
        refusal,
        ApplyRefusal::OutputCollidesWithIndexArtifacts { .. }
    ));
    assert_eq!(fs::read(&index).expect("index"), before, "index untouched");
}

#[test]
fn f11_hardlinked_index_refuses_through_either_entry() {
    let l = layout("f11");
    valid_setup(&l);
    let link = l.td.path.join("link2.json");
    fs::hard_link(&l.index, &link).expect("hard link");
    for entry in [&l.index, &link] {
        let result = run_paths(&l.plan, &l.corpus, entry, &l.output);
        let refusal = result.primary.expect_err("must refuse");
        match &refusal {
            ApplyRefusal::ApplicationIndexHardLinked { nlink, .. } => {
                assert_eq!(*nlink, 2);
            }
            other => panic!("expected ApplicationIndexHardLinked, got {other:?}"),
        }
        assert!(
            !lock_path_of(entry).exists() && !lock_path_of(&l.index).exists(),
            "no lock may be created"
        );
    }
    assert!(!l.output.exists());
}

#[test]
fn f12_late_temp_occupant_makes_commit_refuse_and_stays_untouched() {
    let l = layout("f12");
    valid_setup(&l);
    let temp = temp_path_of(&l.index);
    let before = fs::read(&l.index).expect("index");
    {
        let temp = temp.clone();
        fault::set("commit:before_temp", move || {
            fs::write(&temp, b"foreign occupant").expect("plant occupant");
            Ok(())
        });
    }
    let refusal = refuse(&l);
    fault::clear();
    assert!(
        matches!(refusal, ApplyRefusal::ApplyIoError { .. }),
        "pre-commit ApplyIoError, got {refusal:?}"
    );
    assert_eq!(
        fs::read(&temp).expect("occupant"),
        b"foreign occupant",
        "the late occupant is left untouched"
    );
    assert_eq!(fs::read(&l.index).expect("index"), before, "not applied");
}

#[test]
fn f13_output_in_reserved_staging_namespace_refuses() {
    let l = layout("f13");
    valid_setup(&l);
    let output = l.td.path.join(".x.apply-staging");
    let result = run_paths(&l.plan, &l.corpus, &l.index, &output);
    let refusal = result.primary.expect_err("must refuse");
    assert!(
        matches!(refusal, ApplyRefusal::OutputNameReserved { .. }),
        "got {refusal:?}"
    );
    assert!(!output.exists(), "nothing created");
}

// ── C. lock / temp coordination ────────────────────────────────────────────────

#[test]
fn c11_existing_lockfile_refuses_locked() {
    let l = layout("c11");
    valid_setup(&l);
    fs::write(lock_path_of(&l.index), LOCK_MARKER).expect("hold lock");
    let before = fs::read(&l.index).expect("index");
    let refusal = refuse(&l);
    assert!(
        matches!(refusal, ApplyRefusal::ApplicationIndexLocked { .. }),
        "got {refusal:?}"
    );
    assert!(!l.output.exists());
    assert_eq!(fs::read(&l.index).expect("index"), before);
    assert_eq!(
        fs::read(lock_path_of(&l.index)).expect("lock"),
        LOCK_MARKER.as_bytes(),
        "the held lock is untouched"
    );
}

#[test]
fn c12_stale_lock_blocks_until_validated_recovery() {
    let l = layout("c12");
    valid_setup(&l);
    fs::write(lock_path_of(&l.index), LOCK_MARKER).expect("stale lock");
    let refusal = refuse(&l);
    assert!(matches!(
        refusal,
        ApplyRefusal::ApplicationIndexLocked { .. }
    ));

    // §8.2 recovery: only an exact complete marker may be auto-deleted.
    let content = fs::read(lock_path_of(&l.index)).expect("read lock");
    assert_eq!(content, LOCK_MARKER.as_bytes(), "validated before deletion");
    fs::remove_file(lock_path_of(&l.index)).expect("recovery");
    let result = run(&l);
    assert!(result.primary.is_ok(), "apply succeeds after recovery");
}

#[test]
fn c13_real_second_index_at_temp_name_is_never_unlinked() {
    let l = layout("c13");
    valid_setup(&l);
    // A REAL second index whose filename equals this index's temp name.
    let second = temp_path_of(&l.index);
    fs::write(&second, EMPTY_INDEX).expect("second index");
    let refusal = refuse(&l);
    assert!(
        matches!(refusal, ApplyRefusal::ApplicationIndexTempExists { .. }),
        "got {refusal:?}"
    );
    assert_eq!(
        fs::read(&second).expect("second"),
        EMPTY_INDEX.as_bytes(),
        "the second index is never unlinked or overwritten"
    );
    assert!(!lock_path_of(&l.index).exists(), "lock released");
    assert!(!l.output.exists());
}

#[test]
fn c14_second_applier_during_live_step12_temp_loses_at_the_lock() {
    let l = layout("c14");
    valid_setup(&l);
    let second_result: Rc<RefCell<Option<ApplyRefusal>>> = Rc::new(RefCell::new(None));
    {
        let second_result = Rc::clone(&second_result);
        let plan = l.plan.clone();
        let corpus = l.corpus.clone();
        let index = l.index.clone();
        let output2 = l.td.path.join("out2");
        let temp = temp_path_of(&l.index);
        fault::set("commit:rename", move || {
            // Inside the first applier's step 12: temp exists, lock held.
            assert!(temp.exists(), "the live temp exists in this window");
            let second = run_paths(&plan, &corpus, &index, &output2);
            *second_result.borrow_mut() =
                Some(second.primary.expect_err("second applier must refuse"));
            Ok(())
        });
    }
    let first = run(&l);
    fault::clear();
    assert!(first.primary.is_ok(), "the live commit completes");
    let second = second_result.borrow_mut().take().expect("second ran");
    assert!(
        matches!(second, ApplyRefusal::ApplicationIndexLocked { .. }),
        "a non-holder always loses at the lock boundary and can never \
         observe the live temp as ApplicationIndexTempExists, got {second:?}"
    );
}

#[test]
fn c15_real_second_index_at_lock_name_is_occupied_not_locked() {
    let l = layout("c15");
    valid_setup(&l);
    // A REAL second index whose filename equals this index's lock name.
    let second = lock_path_of(&l.index);
    fs::write(&second, EMPTY_INDEX).expect("second index");
    let refusal = refuse(&l);
    assert!(
        matches!(
            refusal,
            ApplyRefusal::ApplicationIndexLockPathOccupied { .. }
        ),
        "an index document is not a byte-prefix of the marker, got {refusal:?}"
    );
    assert_eq!(
        fs::read(&second).expect("second"),
        EMPTY_INDEX.as_bytes(),
        "the second index is never deleted"
    );
}

#[test]
fn c16_partial_lock_marker_classifies_locked_and_recovery_is_gated() {
    // (a) live window, the exact preregistered case: a concurrent applier
    // observes a NON-EMPTY strict prefix of the marker while the first
    // writer's marker publication is still incomplete. The hook fires
    // between create_new and the marker write; it materializes the partial
    // state at the held lock path, proves it is really non-empty and
    // partial, and only then runs the second applier inline.
    let l = layout("c16a");
    valid_setup(&l);
    let second_result: Rc<RefCell<Option<ApplyRefusal>>> = Rc::new(RefCell::new(None));
    {
        let second_result = Rc::clone(&second_result);
        let plan = l.plan.clone();
        let corpus = l.corpus.clone();
        let index = l.index.clone();
        let output2 = l.td.path.join("out2");
        let lock = lock_path_of(&l.index);
        fault::set("lock:after_create", move || {
            let partial = &LOCK_MARKER.as_bytes()[..10];
            fs::write(&lock, partial).expect("materialize live partial marker");
            let observed = fs::read(&lock).expect("read live lock");
            assert_eq!(
                observed, partial,
                "the live state is a non-empty strict prefix"
            );
            let second = run_paths(&plan, &corpus, &index, &output2);
            *second_result.borrow_mut() =
                Some(second.primary.expect_err("second applier must refuse"));
            Ok(())
        });
    }
    let first = run(&l);
    fault::clear();
    assert!(
        first.primary.is_ok(),
        "the first writer completes: {:?}",
        first.primary
    );
    let second = second_result.borrow_mut().take().expect("second ran");
    assert!(
        matches!(second, ApplyRefusal::ApplicationIndexLocked { .. }),
        "a live NON-EMPTY partial marker is a Griff lock, never foreign \
         occupancy, got {second:?}"
    );

    // (b) crashed non-empty partial marker: classified Locked; never
    // auto-deleted; the operator-proven relocation unblocks.
    let l = layout("c16b");
    valid_setup(&l);
    let partial = &LOCK_MARKER.as_bytes()[..10];
    fs::write(lock_path_of(&l.index), partial).expect("crash debris");
    let refusal = refuse(&l);
    assert!(
        matches!(refusal, ApplyRefusal::ApplicationIndexLocked { .. }),
        "a non-empty partial prefix is never LockPathOccupied, got {refusal:?}"
    );
    assert_eq!(
        fs::read(lock_path_of(&l.index)).expect("lock"),
        partial,
        "the partial marker is not auto-deleted by any apply"
    );
    // Operator-proven recovery: non-destructive relocation out of the
    // coordination namespace (§8.2), then the apply succeeds.
    fs::rename(
        lock_path_of(&l.index),
        l.td.path.join("quarantined-lock-debris"),
    )
    .expect("relocate");
    let result = run(&l);
    assert!(result.primary.is_ok(), "unblocked after relocation");
    assert!(
        l.td.path.join("quarantined-lock-debris").exists(),
        "relocation preserved the evidence"
    );
}

// ── K. preservation under adversarial bytes ────────────────────────────────────

#[test]
fn k10_duplicate_json_keys_in_touched_file_refuse() {
    // (i) A duplicate key the tolerant derive parse cannot see: inside an
    // unknown member, which serde's derived deserializer skips wholesale.
    // Value comparison cannot prove duplicates absent either (last wins), so
    // only the distinct §10.3 duplicate-rejecting pass can catch it — and it
    // must run before the round-trip guard, so the refusal names the
    // duplicate, not the unknown member.
    let l = layout("k10a");
    valid_setup(&l);
    let path = l.corpus.join("a1.chunk.json");
    let text = fs::read_to_string(&path).expect("read");
    let with_rogue = text.replacen("{\n", "{\n  \"rogue\": {\"k\": 1, \"k\": 1},\n", 1);
    assert_ne!(with_rogue, text, "fixture shape changed");
    fs::write(&path, with_rogue).expect("write");
    let refusal = refuse(&l);
    match &refusal {
        ApplyRefusal::NonCanonicalCorpusFile { path, detail } => {
            assert!(path.contains("a1.chunk.json"));
            assert!(
                detail.contains("duplicate"),
                "the duplicate-rejecting pass must fire first: {detail}"
            );
        }
        other => panic!("expected NonCanonicalCorpusFile, got {other:?}"),
    }
    assert!(!l.output.exists(), "nothing published");

    // (ii) A duplicated KNOWN field is refused even earlier: the derived
    // parse itself rejects it during step-3 tree agreement, so the file
    // never reaches the rewrite path at all. Fail-closed both ways — no
    // duplicate is ever silently laundered.
    let l = layout("k10b");
    valid_setup(&l);
    let path = l.corpus.join("a1.chunk.json");
    let text = fs::read_to_string(&path).expect("read");
    let needle = "\"title\": \"Title a1\",";
    assert!(text.contains(needle), "fixture shape changed");
    let dup = format!("\"title\": \"Title a1\",\n  {needle}");
    fs::write(&path, text.replacen(needle, &dup, 1)).expect("write");
    let refusal = refuse(&l);
    assert!(
        matches!(&refusal, ApplyRefusal::CorpusTreeDisagreement { detail }
            if detail.contains("duplicate field")),
        "got {refusal:?}"
    );
    assert!(!l.output.exists(), "nothing published");
}

#[test]
fn k11_staged_tree_corruption_aborts_before_publication() {
    let l = layout("k11");
    valid_setup(&l);
    let original = fs::read(l.corpus.join("a1.chunk.json")).expect("original");
    {
        let staged_chunk = staging_path_of(&l.output).join("a1.chunk.json");
        fault::set("stage:before_selfcheck", move || {
            // A buggy write left one affected chunk file stale.
            fs::write(&staged_chunk, &original).expect("corrupt staged chunk");
            Ok(())
        });
    }
    let refusal = refuse(&l);
    fault::clear();
    assert!(
        matches!(refusal, ApplyRefusal::OutputPreflightInconsistent { .. }),
        "the staged tree-agreement re-run must abort, got {refusal:?}"
    );
    assert!(!l.output.exists(), "nothing published");
    assert!(!staging_path_of(&l.output).exists(), "staging cleaned");
}

// ── R. commit failure can never surface as success ─────────────────────────────

#[test]
fn r4_commit_failure_is_never_success_and_orphan_blocks_retry() {
    let l = layout("r4");
    valid_setup(&l);
    let before = fs::read(&l.index).expect("index");
    fault::set("commit:rename", io_fail);
    let result = run(&l);
    fault::clear();
    let refusal = result.primary.expect_err("commit failure is a refusal");
    assert!(matches!(refusal, ApplyRefusal::ApplyIoError { .. }));
    assert_eq!(
        fs::read(&l.index).expect("index"),
        before,
        "no record ⇒ provably not applied"
    );
    assert!(l.output.exists(), "published orphan per §8.2");

    // A retry must refuse OutputAlreadyExists until the operator removes the
    // orphan — never stack a second tree, never claim success.
    let retry = run(&l);
    let refusal = retry.primary.expect_err("retry must refuse");
    assert!(
        matches!(refusal, ApplyRefusal::OutputAlreadyExists { .. }),
        "got {refusal:?}"
    );
    // Mandated recovery: delete the orphan, then the apply succeeds.
    fs::remove_dir_all(&l.output).expect("operator removes orphan");
    let after_recovery = run(&l);
    assert!(
        after_recovery.primary.is_ok(),
        "{:?}",
        after_recovery.primary
    );
}
