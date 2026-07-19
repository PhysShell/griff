//! Native filesystem adapter for the curation store (step 3B2).
//!
//! The domain model in [`griff_core::curation_store`] owns the wire format and
//! its rules and performs **no I/O**. This module is the native backend: it
//! reads and writes those bytes on a real filesystem, durably and atomically,
//! so a decision file survives a crash between `write()` and the rename that
//! publishes it. All platform-specific behavior lives here — never in core.
//!
//! The write is the classic temp-then-rename dance: serialize, write a sibling
//! temp, flush and `fsync` it, then atomically rename it over the canonical
//! path, and finally `fsync` the parent directory where the platform supports
//! it. A failure at any step before the rename leaves the previous store
//! byte-for-byte intact and removes the temp; the temp never becomes the
//! canonical store by any path but a completed rename.
//!
//! Durability scope, stated precisely:
//! - the temp's *contents* are `fsync`ed before it is published;
//! - the replace itself is atomic on a filesystem that supports it;
//! - on **Unix** the parent directory entry is then `fsync`ed too, so the rename
//!   survives power loss;
//! - on **Windows** this `std`-only adapter does **not** `fsync` the directory
//!   handle (`std` exposes no way to), so the post-power-loss guarantee is not
//!   identical to Unix.
//!
//! A directory-`fsync` failure *after* a successful rename is a post-commit
//! durability warning ([`CurationFsError::PostCommitDurability`]), not a
//! rollback: the new bytes are already the store.
//!
//! Single-writer: one process owns a given store path at a time (the curation
//! cockpit flow is serial). The temp sibling has a fixed name for that reason.
//!
//! **Experimental, `#[doc(hidden)]`** via the crate root — not a stable API.

use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::{self, ErrorKind, Write};
use std::path::{Path, PathBuf};

use griff_core::curation_store::{
    decode_store, encode_store, CurationStoreV1, StoreDecodeError, StoreEncodeError,
};

/// Everything that can go wrong reading or writing a store on disk.
#[derive(Debug, thiserror::Error)]
pub enum CurationFsError {
    /// A filesystem operation failed.
    #[error("curation store I/O failed: {0}")]
    Io(#[from] io::Error),
    /// The bytes on disk are not a valid store (malformed or rule-violating).
    #[error("curation store on disk is invalid: {0}")]
    Decode(#[from] StoreDecodeError),
    /// The store to write could not be encoded (it violates a domain rule).
    #[error("curation store cannot be encoded: {0}")]
    Encode(#[from] StoreEncodeError),
    /// The new bytes are already published at the canonical path, but the
    /// directory entry's durability could not be confirmed (the post-rename
    /// parent-directory `fsync` failed). Distinct from a pre-commit
    /// [`CurationFsError::Io`]: there is no old store to fall back to, and no
    /// rollback — the write happened.
    #[error("curation store published but directory durability is unconfirmed: {source}")]
    PostCommitDurability {
        #[source]
        source: io::Error,
    },
}

/// The outcome of loading.
///
/// A *missing* file is an explicit, typed absence — the caller decides whether
/// that means "start a fresh empty store" — and is never conflated with a
/// *malformed* file, which is a hard [`CurationFsError::Decode`].
#[derive(Debug)]
pub enum StoreOnDisk {
    /// No store file exists at the path yet.
    Missing,
    /// A valid store was read and decoded.
    Loaded(CurationStoreV1),
}

/// Where a test asks the atomic write to fail. `None` in all production paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FailPoint {
    None,
    BeforeReplace,
    DuringReplace,
    /// The parent-directory sync *after* a successful rename — the new bytes are
    /// already published; only their directory-entry durability is unconfirmed.
    AfterReplace,
}

/// Loads the store at `path`.
///
/// # Errors
/// [`CurationFsError::Decode`] if the file exists but is not a valid store;
/// [`CurationFsError::Io`] for any read error other than "not found" (which is
/// reported as [`StoreOnDisk::Missing`], not an error).
pub fn load_store(path: &Path) -> Result<StoreOnDisk, CurationFsError> {
    match fs::read(path) {
        Ok(bytes) => Ok(StoreOnDisk::Loaded(decode_store(&bytes)?)),
        // A missing file is a typed absence the caller resolves, not a failure.
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(StoreOnDisk::Missing),
        Err(e) => Err(e.into()),
    }
}

/// Atomically and durably writes `store` to `path`, replacing any existing one.
///
/// # Errors
/// - [`CurationFsError::Encode`] if the store violates a domain rule — nothing
///   is written.
/// - [`CurationFsError::Io`] for a *pre-commit* failure (staging or the replace
///   itself) — the previous store is left byte-for-byte intact and the temp is
///   removed.
/// - [`CurationFsError::PostCommitDurability`] if the replace succeeded but the
///   parent-directory `fsync` did not — the new bytes are already the store;
///   only their directory-entry durability is unconfirmed.
pub fn write_store(path: &Path, store: &CurationStoreV1) -> Result<(), CurationFsError> {
    // Encoding validates the store; an invalid one is refused before any bytes
    // are produced, so a write never begins with garbage.
    let bytes = encode_store(store)?;
    atomic_write(path, &bytes, FailPoint::None)
}

/// The atomic-write core, with a test-only failure injection point.
fn atomic_write(path: &Path, bytes: &[u8], fail: FailPoint) -> Result<(), CurationFsError> {
    let tmp = temp_sibling(path);
    // Stage the full contents and flush them to the device before the temp is
    // eligible to become the store; on any failure, take the temp with us. A
    // leftover temp is harmless (it is never the canonical store), so a failed
    // cleanup is best-effort and does not mask the original error.
    if let Err(e) = write_temp(&tmp, bytes) {
        fs::remove_file(&tmp).ok();
        return Err(e);
    }
    if fail == FailPoint::BeforeReplace {
        fs::remove_file(&tmp).ok();
        return Err(CurationFsError::Io(io::Error::other(
            "injected curation-store failure before replace",
        )));
    }
    // Pre-commit: the atomic publish. A replace failure — injected or a real OS
    // error — leaves the previous store intact, so we drop the temp and report a
    // plain Io.
    if let Err(e) = replace_file(&tmp, path, fail) {
        fs::remove_file(&tmp).ok();
        return Err(e.into());
    }
    // Post-commit: the new bytes ARE the store now. A directory-sync failure is a
    // durability warning, not a rollback — a second file transaction cannot undo
    // the first — so it is reported as its own typed error, never a pre-commit Io.
    let durability = if fail == FailPoint::AfterReplace {
        Err(io::Error::other(
            "injected curation-store durability failure after replace",
        ))
    } else {
        sync_parent_dir(path)
    };
    durability.map_err(|source| CurationFsError::PostCommitDurability { source })
}

/// The atomic publish step, isolated so a test can drive its error branch the
/// same way a real OS replace failure would. Both POSIX and Windows
/// (`MoveFileExW`) replace an existing destination atomically.
fn replace_file(temp: &Path, canonical: &Path, fail: FailPoint) -> io::Result<()> {
    if fail == FailPoint::DuringReplace {
        return Err(io::Error::other("injected curation-store replace failure"));
    }
    fs::rename(temp, canonical)
}

/// The fixed sibling temp path a write stages into before the rename.
fn temp_sibling(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map(OsStr::to_os_string)
        .unwrap_or_default();
    name.push(".tmp");
    path.with_file_name(name)
}

/// The directory to fsync for `path`, treating an empty parent — a bare relative
/// filename like `curation.json`, whose `Path::parent` is `""` — as the current
/// directory `.`, so a relative store path still fsyncs a real directory.
#[cfg_attr(not(unix), allow(dead_code))] // only the unix sync path calls this
fn parent_for_sync(path: &Path) -> &Path {
    path.parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

/// Writes `bytes` to `tmp` and flushes them all the way to the device.
fn write_temp(tmp: &Path, bytes: &[u8]) -> Result<(), CurationFsError> {
    let mut file = File::create(tmp)?;
    file.write_all(bytes)?;
    file.flush()?;
    file.sync_all()?;
    Ok(())
}

/// `fsync`s the parent directory so the rename itself is durable — on platforms
/// that let a directory be opened as a file (POSIX).
#[cfg(unix)]
fn sync_parent_dir(path: &Path) -> io::Result<()> {
    File::open(parent_for_sync(path))?.sync_all()
}

/// Windows/other: `std` cannot `fsync` a directory handle, and the rename is the
/// durability barrier the platform itself provides. Intentionally a no-op.
#[cfg(not(unix))]
#[allow(clippy::unnecessary_wraps)] // signature parity with the unix variant
const fn sync_parent_dir(_path: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::unwrap_used,
        clippy::indexing_slicing,
        clippy::panic
    )]

    use super::{
        atomic_write, load_store, parent_for_sync, temp_sibling, write_store, CurationFsError,
        FailPoint, StoreOnDisk,
    };
    use griff_core::corpus::{ChunkId, ReviewerDecision};
    use griff_core::curation_store::{
        encode_store, ChunkFingerprint, CorpusFingerprint, CurationContext, CurationEvent,
        CurationEventId, CurationStoreV1, ReviewerId, StoreDecodeError, CURATION_STORE_VERSION,
    };
    use std::path::{Path, PathBuf};
    use std::{env, fs, process};

    /// A fresh, empty scratch directory unique to this test (and process).
    fn scratch(tag: &str) -> PathBuf {
        let dir = env::temp_dir().join(format!("griff_curfs_{}_{tag}", process::id()));
        fs::remove_dir_all(&dir).ok();
        fs::create_dir_all(&dir).expect("create scratch dir");
        dir
    }

    fn event(id: &str) -> CurationEvent {
        CurationEvent {
            event_id: CurationEventId(id.to_owned()),
            chunk_id: ChunkId("c1".to_owned()),
            chunk_fingerprint: ChunkFingerprint("fp1".to_owned()),
            decision: ReviewerDecision::Accepted,
            reviewer: ReviewerId("alice".to_owned()),
            occurred_at: "2026-07-18T10:00:00Z".to_owned(),
            corpus_fingerprint: CorpusFingerprint("corpus".to_owned()),
            context: CurationContext {
                cluster_representative: ChunkId("c1".to_owned()),
                cluster_members: vec![ChunkId("c1".to_owned())],
            },
            tags: Vec::new(),
            note: None,
        }
    }

    fn sample(id: &str) -> CurationStoreV1 {
        CurationStoreV1 {
            version: CURATION_STORE_VERSION,
            corpus_fingerprint: CorpusFingerprint("corpus".to_owned()),
            events: vec![event(id)],
        }
    }

    fn loaded(path: &Path) -> CurationStoreV1 {
        match load_store(path).expect("load succeeds") {
            StoreOnDisk::Loaded(store) => store,
            StoreOnDisk::Missing => panic!("expected a loaded store, found none"),
        }
    }

    #[test]
    fn a_missing_store_is_a_typed_absence_not_malformed() {
        let dir = scratch("missing");
        let outcome = load_store(&dir.join("none.json")).expect("missing is not an error");
        assert!(matches!(outcome, StoreOnDisk::Missing));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_malformed_store_is_a_typed_refusal_not_empty() {
        let dir = scratch("malformed");
        let path = dir.join("curation.json");
        fs::write(&path, b"{ this is not json").expect("write garbage");
        assert!(
            matches!(
                load_store(&path),
                Err(CurationFsError::Decode(StoreDecodeError::MalformedStore(_)))
            ),
            "garbage on disk is a typed decode refusal, never an empty store"
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_written_store_survives_a_reload() {
        let dir = scratch("reload");
        let path = dir.join("curation.json");
        let store = sample("e1");
        write_store(&path, &store).expect("write");
        // A fresh load (a stand-in for a process restart) sees the same store.
        assert_eq!(loaded(&path), store);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_failure_before_replace_keeps_the_old_store_byte_identical() {
        let dir = scratch("before");
        let path = dir.join("curation.json");
        write_store(&path, &sample("old")).expect("seed old store");
        let old_bytes = fs::read(&path).expect("read old");

        let new_bytes = encode_store(&sample("new")).expect("encode new");
        let result = atomic_write(&path, &new_bytes, FailPoint::BeforeReplace);

        assert!(result.is_err(), "an injected failure is a typed error");
        assert_eq!(
            fs::read(&path).expect("reread"),
            old_bytes,
            "old store intact"
        );
        assert!(!temp_sibling(&path).exists(), "temp cleaned up on failure");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_failure_during_replace_is_a_typed_error_and_keeps_the_old_store() {
        let dir = scratch("during");
        let path = dir.join("curation.json");
        write_store(&path, &sample("old")).expect("seed old store");
        let old_bytes = fs::read(&path).expect("read old");

        let new_bytes = encode_store(&sample("new")).expect("encode new");
        let result = atomic_write(&path, &new_bytes, FailPoint::DuringReplace);

        assert!(matches!(result, Err(CurationFsError::Io(_))), "typed error");
        assert_eq!(
            fs::read(&path).expect("reread"),
            old_bytes,
            "old store intact"
        );
        assert!(!temp_sibling(&path).exists(), "temp cleaned up on failure");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_temp_file_never_becomes_the_canonical_store() {
        let dir = scratch("temp");
        let path = dir.join("curation.json");
        write_store(&path, &sample("e1")).expect("write");
        assert!(
            !temp_sibling(&path).exists(),
            "the temp is renamed away, never left as the store"
        );
        assert!(matches!(load_store(&path), Ok(StoreOnDisk::Loaded(_))));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn re_encoding_a_canonical_store_is_byte_stable() {
        let dir = scratch("stable");
        let path = dir.join("curation.json");
        let store = sample("e1");
        write_store(&path, &store).expect("write once");
        let first = fs::read(&path).expect("read first");
        write_store(&path, &store).expect("write twice");
        let second = fs::read(&path).expect("read second");
        assert_eq!(first, second, "the same store writes the same bytes");
        assert_eq!(
            encode_store(&loaded(&path)).expect("re-encode"),
            first,
            "decode->encode reproduces the on-disk bytes"
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_failure_after_replace_is_a_post_commit_durability_error() {
        // The rename already succeeded — the NEW bytes ARE the store — and only
        // the directory fsync failed. That must be a distinct post-commit error,
        // never a pre-commit Io that would falsely promise the old store survived.
        let dir = scratch("after");
        let path = dir.join("curation.json");
        write_store(&path, &sample("old")).expect("seed old store");
        let new_bytes = encode_store(&sample("new")).expect("encode new");

        let result = atomic_write(&path, &new_bytes, FailPoint::AfterReplace);

        assert!(
            matches!(result, Err(CurationFsError::PostCommitDurability { .. })),
            "a post-rename durability failure is its own typed error"
        );
        assert!(
            !matches!(result, Err(CurationFsError::Io(_))),
            "it is distinguishable from a pre-commit Io"
        );
        assert_eq!(
            fs::read(&path).expect("reread"),
            new_bytes,
            "the new bytes are already published"
        );
        assert!(
            !temp_sibling(&path).exists(),
            "temp is gone (it was renamed)"
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_parent_to_sync_treats_a_bare_filename_as_the_current_dir() {
        // A relative store path must still fsync a real directory on Unix.
        assert_eq!(parent_for_sync(Path::new("curation.json")), Path::new("."));
        assert_eq!(
            parent_for_sync(Path::new("dir/curation.json")),
            Path::new("dir")
        );
        assert_eq!(
            parent_for_sync(Path::new("/dir/curation.json")),
            Path::new("/dir")
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_write_syncs_the_parent_directory_on_unix() {
        // On POSIX the write path opens and fsyncs the parent dir; exercising a
        // real write into a nested existing directory covers that step.
        let dir = scratch("dirsync").join("nested");
        fs::create_dir_all(&dir).expect("nested dir");
        let path = dir.join("curation.json");
        write_store(&path, &sample("e1")).expect("write succeeds with dir sync");
        assert!(matches!(load_store(&path), Ok(StoreOnDisk::Loaded(_))));
        fs::remove_dir_all(&dir).ok();
    }
}
