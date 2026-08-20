//! Implementation-review repair witnesses (PR #191 hostile review round 1).
//!
//! Each test preregisters one defect the review found in the implementation
//! relative to the accepted contract `47e734c`. Committed RED, tests-only;
//! the GREEN repair follows in a separate commit. No contract law changes:
//! every expected refusal is the one the accepted taxonomy already assigns.

mod common;

use common::{
    accept, batch_for, event, layout, lock_path_of, write_corpus, write_empty_index, write_plan,
    Chunk, Layout,
};
use griff_song_curation::apply::{apply, fault, ApplyPaths, ApplyRefusal, ApplyRun};
use std::fs;
use std::io;

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

/// Review blocker 1a: an EMPTY foreign subdirectory inside the reserved area
/// is invisible to a file-only walk, yet the accepted §4.2 shape law admits
/// only the two tool-owned proof artifacts as regular files at the reserved
/// root — a directory entry is foreign even when it contains nothing.
#[test]
fn review1_empty_foreign_reserved_subdir_refuses() {
    let l = layout("rev1-emptydir");
    valid_setup(&l);
    fs::create_dir_all(l.corpus.join("song-curation/extra")).expect("mk empty foreign dir");
    let refusal = refuse(&l);
    match &refusal {
        ApplyRefusal::CorpusTreeDisagreement { detail } => {
            assert!(detail.contains("extra"), "must name the entry: {detail}");
        }
        other => panic!("expected CorpusTreeDisagreement, got {other:?}"),
    }
    assert!(!l.output.exists(), "nothing written");
}

/// Review blocker 1b: an allowed NAME does not satisfy the shape law unless
/// it is a regular file — a symlink at `song-curation/manifest.json` must
/// refuse, not pass a name-only test.
#[test]
fn review1_reserved_allowed_name_must_be_a_regular_file() {
    let l = layout("rev1-symlink");
    valid_setup(&l);
    fs::create_dir_all(l.corpus.join("song-curation")).expect("mk reserved");
    // Symlink to a real file elsewhere, so a follow-based test would even
    // find readable JSON behind it.
    std::os::unix::fs::symlink(
        l.corpus.join("manifest.json"),
        l.corpus.join("song-curation/manifest.json"),
    )
    .expect("symlink");
    let refusal = refuse(&l);
    match &refusal {
        ApplyRefusal::CorpusTreeDisagreement { detail } => {
            assert!(
                detail.contains("song-curation/manifest.json"),
                "must name the entry: {detail}"
            );
        }
        other => panic!("expected CorpusTreeDisagreement, got {other:?}"),
    }
    assert!(!l.output.exists(), "nothing written");
}

/// Review blocker 2: a pre-existing NON-REGULAR occupant of the lock path
/// (here: a directory) must be classified at the lock boundary as
/// `ApplicationIndexLockPathOccupied` — §12 reserves `ApplyIoError` for lock
/// -acquisition causes other than pre-existence, and reading an arbitrary
/// occupant (a FIFO could block) contradicts the no-wait protocol. The
/// occupant stays untouched.
#[test]
fn review2_directory_lock_occupant_classifies_occupied() {
    let l = layout("rev2-dirlock");
    valid_setup(&l);
    let lock = lock_path_of(&l.index);
    fs::create_dir(&lock).expect("directory occupant");
    let refusal = refuse(&l);
    assert!(
        matches!(
            refusal,
            ApplyRefusal::ApplicationIndexLockPathOccupied { .. }
        ),
        "pre-existence is classified, never ApplyIoError: got {refusal:?}"
    );
    assert!(lock.is_dir(), "the occupant is left untouched");
    assert!(!l.output.exists(), "nothing written");
}

// Keep the fault import referenced so the harness stays linked for the
// blocker-3 witness added in the next tests-only commit.
#[allow(dead_code)]
fn _fault_link() {
    fault::clear();
    let _ = io::Error::other("unused");
}
