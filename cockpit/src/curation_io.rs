//! Curation-store persistence adapter for the cockpit (step 3C-A).
//!
//! The browser backend for [`griff_core::curation_store`]: it reads and writes
//! the store's **canonical bytes** — the same `encode_store`/`decode_store` the
//! CLI's native adapter uses (ADR-0027 §3: "no browser-specific wire schema") —
//! to a single OPFS file, `curation/store.json`. A native arm mirrors it over
//! `std::fs` for the desktop cockpit.
//!
//! Only the byte *transport* is per-target; the Missing-vs-malformed
//! classification and the format itself live in core. There is deliberately **no
//! trait abstracting this over the CLI's native adapter (3B2)**: the shared
//! contract is core's encode/decode, which both call directly.
//!
//! **Experimental, `#[doc(hidden)]`** via the crate — not a stable API.

use griff_core::curation_store::{decode_store, CurationStoreV1, StoreDecodeError};

#[cfg(not(target_arch = "wasm32"))]
use std::{fs, io::ErrorKind, path::Path};

/// The outcome of loading the store.
///
/// A *missing* file is a typed absence — the caller bootstraps a fresh empty
/// store — and is never conflated with a *malformed* file, which is a hard
/// [`CurationStoreError::Decode`].
#[derive(Debug)]
pub enum StoreOnDisk {
    /// No store file exists yet.
    Missing,
    /// A valid store was read and decoded.
    Loaded(CurationStoreV1),
}

/// A raw read of the store location, before decoding — the seam between the
/// per-target byte transport and the shared [`classify`] logic.
#[derive(Debug)]
pub enum StoreBytes {
    /// The store location does not exist.
    Missing,
    /// The store location holds these bytes (which may or may not decode).
    Present(Vec<u8>),
}

/// Everything that can go wrong loading the store.
#[derive(Debug, thiserror::Error)]
pub enum CurationStoreError {
    /// A storage operation failed.
    #[error("curation store I/O failed: {0}")]
    Io(String),
    /// Bytes are present but are not a valid store (malformed or rule-violating).
    #[error("curation store on disk is invalid: {0}")]
    Decode(#[from] StoreDecodeError),
}

/// Classifies a raw read: a missing location is a typed absence; present bytes
/// are decoded (a malformed store is a typed refusal, never an empty store).
///
/// # Errors
/// [`CurationStoreError::Decode`] when bytes are present but do not decode.
pub fn classify(read: StoreBytes) -> Result<StoreOnDisk, CurationStoreError> {
    match read {
        StoreBytes::Missing => Ok(StoreOnDisk::Missing),
        StoreBytes::Present(bytes) => Ok(StoreOnDisk::Loaded(decode_store(&bytes)?)),
    }
}

/// Loads the store from `path` on the native desktop cockpit.
///
/// # Errors
/// [`CurationStoreError::Decode`] if the file exists but is invalid;
/// [`CurationStoreError::Io`] for any read error other than "not found" (which
/// is [`StoreOnDisk::Missing`], not an error).
#[cfg(not(target_arch = "wasm32"))]
pub fn load_store(path: &Path) -> Result<StoreOnDisk, CurationStoreError> {
    match fs::read(path) {
        Ok(bytes) => classify(StoreBytes::Present(bytes)),
        // A missing file is a typed absence the caller resolves, not a failure.
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(StoreOnDisk::Missing),
        Err(e) => Err(CurationStoreError::Io(e.to_string())),
    }
}

/// Writes the store's canonical `bytes` to `path` on the native desktop cockpit.
///
/// # Errors
/// [`CurationStoreError::Io`] if the write fails.
#[cfg(not(target_arch = "wasm32"))]
pub fn write_store(path: &Path, bytes: &[u8]) -> Result<(), CurationStoreError> {
    fs::write(path, bytes).map_err(|e| CurationStoreError::Io(e.to_string()))
}

/// The browser byte transport: read and write `curation/store.json` in the
/// Origin Private File System. A missing directory or file reads as
/// [`StoreBytes::Missing`], so the caller can bootstrap a fresh store; the
/// Missing-vs-malformed split then happens in [`classify`].
#[cfg(target_arch = "wasm32")]
pub use wasm::{opfs_load_store, opfs_write_store};

#[cfg(target_arch = "wasm32")]
mod wasm {
    use wasm_bindgen::prelude::*;
    use wasm_bindgen_futures::JsFuture;

    use super::StoreBytes;

    /// The OPFS directory and file the store lives in — a sibling of the
    /// `corpus/` tree, one canonical file.
    const DIR: &str = "curation";
    const FILE: &str = "store.json";

    /// The OPFS `curation/` directory handle. `create` controls whether a
    /// missing directory is created (write) or reported as absent (read → `None`).
    async fn curation_dir(
        create: bool,
    ) -> Result<Option<web_sys::FileSystemDirectoryHandle>, JsValue> {
        let storage = web_sys::window()
            .ok_or_else(|| JsValue::from_str("no window"))?
            .navigator()
            .storage();
        let root = JsFuture::from(storage.get_directory())
            .await?
            .dyn_into::<web_sys::FileSystemDirectoryHandle>()?;
        let opts = web_sys::FileSystemGetDirectoryOptions::new();
        opts.set_create(create);
        match JsFuture::from(root.get_directory_handle_with_options(DIR, &opts)).await {
            Ok(handle) => Ok(Some(
                handle.dyn_into::<web_sys::FileSystemDirectoryHandle>()?,
            )),
            // Absent directory (no store recorded yet) → a typed absence, not an error.
            Err(_) if !create => Ok(None),
            Err(err) => Err(err),
        }
    }

    /// Reads `curation/store.json`. An absent directory or file is
    /// [`StoreBytes::Missing`]; present bytes are returned verbatim for
    /// [`super::classify`] to decode.
    // OPFS handles aren't `Send`, but wasm is single-threaded and the future is
    // driven by `spawn_local`, which never requires `Send`.
    #[allow(clippy::future_not_send)]
    pub async fn opfs_load_store() -> Result<StoreBytes, JsValue> {
        let Some(dir) = curation_dir(false).await? else {
            return Ok(StoreBytes::Missing);
        };
        // `create` defaults to false: a missing file rejects, which we read as absence.
        let opts = web_sys::FileSystemGetFileOptions::new();
        let handle = match JsFuture::from(dir.get_file_handle_with_options(FILE, &opts)).await {
            Ok(handle) => handle.dyn_into::<web_sys::FileSystemFileHandle>()?,
            Err(_) => return Ok(StoreBytes::Missing),
        };
        // A `File` is a `Blob`; `Blob::text()` yields its contents as a string.
        let blob = JsFuture::from(handle.get_file())
            .await?
            .dyn_into::<web_sys::Blob>()?;
        let text = JsFuture::from(blob.text())
            .await?
            .as_string()
            .unwrap_or_default();
        Ok(StoreBytes::Present(text.into_bytes()))
    }

    /// Writes `contents` to `curation/store.json`, creating the directory and
    /// file as needed. Returns once the writable stream is closed.
    #[allow(clippy::future_not_send)]
    pub async fn opfs_write_store(contents: String) -> Result<(), JsValue> {
        let dir = curation_dir(true)
            .await?
            .ok_or_else(|| JsValue::from_str("no curation directory"))?;
        let opts = web_sys::FileSystemGetFileOptions::new();
        opts.set_create(true);
        let file = JsFuture::from(dir.get_file_handle_with_options(FILE, &opts))
            .await?
            .dyn_into::<web_sys::FileSystemFileHandle>()?;
        let writable = JsFuture::from(file.create_writable())
            .await?
            .dyn_into::<web_sys::FileSystemWritableFileStream>()?;
        JsFuture::from(writable.write_with_str(&contents)?).await?;
        JsFuture::from(writable.close()).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::{classify, StoreBytes, StoreOnDisk};
    use griff_core::corpus::{ChunkId, ReviewerDecision};
    use griff_core::curation_store::{
        encode_store, ChunkFingerprint, CorpusFingerprint, CurationContext, CurationEvent,
        CurationEventId, CurationStoreV1, ReviewerId, StoreDecodeError, CURATION_STORE_VERSION,
    };

    fn sample() -> CurationStoreV1 {
        CurationStoreV1 {
            version: CURATION_STORE_VERSION,
            corpus_fingerprint: CorpusFingerprint("corpus".to_owned()),
            events: vec![CurationEvent {
                event_id: CurationEventId("e1".to_owned()),
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
            }],
        }
    }

    #[test]
    fn a_missing_location_is_a_typed_absence() {
        assert!(matches!(
            classify(StoreBytes::Missing),
            Ok(StoreOnDisk::Missing)
        ));
    }

    #[test]
    fn present_valid_bytes_decode_to_the_same_store() {
        let store = sample();
        let bytes = encode_store(&store).expect("encodes");
        match classify(StoreBytes::Present(bytes)).expect("classifies") {
            StoreOnDisk::Loaded(loaded) => assert_eq!(loaded, store),
            StoreOnDisk::Missing => panic!("present bytes must not read as missing"),
        }
    }

    #[test]
    fn present_malformed_bytes_are_a_typed_refusal_not_absence() {
        let err = classify(StoreBytes::Present(b"{ not a store".to_vec())).expect_err("must fail");
        assert!(matches!(
            err,
            super::CurationStoreError::Decode(StoreDecodeError::MalformedStore(_))
        ));
    }

    #[test]
    fn canonical_bytes_survive_a_decode_encode_round_trip() {
        let store = sample();
        let bytes = encode_store(&store).expect("encodes");
        match classify(StoreBytes::Present(bytes.clone())).expect("classifies") {
            StoreOnDisk::Loaded(loaded) => {
                assert_eq!(encode_store(&loaded).expect("re-encodes"), bytes);
            }
            StoreOnDisk::Missing => panic!("present bytes must not read as missing"),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn native_write_then_load_round_trips_and_absence_is_missing() {
        use super::{load_store, write_store};
        use std::{env, fs, process};

        let dir = env::temp_dir().join(format!("griff_curio_{}", process::id()));
        fs::remove_dir_all(&dir).ok();
        fs::create_dir_all(&dir).expect("mkdir");
        let path = dir.join("store.json");

        // Absent file → Missing.
        assert!(matches!(
            load_store(&path).expect("load absent"),
            StoreOnDisk::Missing
        ));

        let store = sample();
        let bytes = encode_store(&store).expect("encodes");
        write_store(&path, &bytes).expect("write");
        assert_eq!(
            fs::read(&path).expect("reread"),
            bytes,
            "bytes land verbatim"
        );
        match load_store(&path).expect("load present") {
            StoreOnDisk::Loaded(loaded) => assert_eq!(loaded, store),
            StoreOnDisk::Missing => panic!("written store must load"),
        }

        fs::remove_dir_all(&dir).ok();
    }
}
