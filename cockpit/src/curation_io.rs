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

use griff_core::curation_store::{
    decode_store, encode_store, CurationStoreV1, StoreDecodeError, StoreEncodeError,
};

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

/// Everything that can go wrong loading or saving the store.
#[derive(Debug, thiserror::Error)]
pub enum CurationStoreError {
    /// A storage/transport operation failed (and is *not* a benign absence).
    #[error("curation store I/O failed: {0}")]
    Io(String),
    /// Bytes are present but are not a valid store (malformed or rule-violating).
    #[error("curation store on disk is invalid: {0}")]
    Decode(#[from] StoreDecodeError),
    /// The store to save is itself invalid — refused before any bytes are
    /// written, so a transport never carries non-canonical or invalid data.
    #[error("curation store cannot be encoded: {0}")]
    Encode(#[from] StoreEncodeError),
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

/// Encodes a store to the canonical bytes a transport writes.
///
/// This is the fallible, *validated* step every write shares: an invalid store
/// is refused here — a typed [`CurationStoreError::Encode`] — before any I/O, so
/// a transport can never persist non-canonical or rule-violating bytes. Core's
/// [`encode_store`] is the only encoder; there is no cockpit-side serializer.
///
/// # Errors
/// [`CurationStoreError::Encode`] if the store violates a domain rule.
pub fn encode(store: &CurationStoreV1) -> Result<Vec<u8>, CurationStoreError> {
    Ok(encode_store(store)?)
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

    use griff_core::curation_store::CurationStoreV1;

    use super::{encode, CurationStoreError, StoreBytes};

    /// The OPFS directory and file the store lives in — a sibling of the
    /// `corpus/` tree, one canonical file.
    const DIR: &str = "curation";
    const FILE: &str = "store.json";

    /// Whether `err` is a genuine "does not exist" — the *only* reject that means
    /// absence. A `SecurityError`, `InvalidStateError`, or anything else is a real
    /// failure and must not masquerade as "no store yet".
    fn is_not_found(err: &JsValue) -> bool {
        err.dyn_ref::<web_sys::DomException>()
            .is_some_and(|e| e.name() == "NotFoundError")
    }

    /// Maps a transport reject to a typed [`CurationStoreError::Io`], preferring
    /// the `DOMException` message.
    fn transport(err: &JsValue) -> CurationStoreError {
        let message = err
            .dyn_ref::<web_sys::DomException>()
            .map_or_else(|| format!("{err:?}"), web_sys::DomException::message);
        CurationStoreError::Io(message)
    }

    /// The OPFS `curation/` directory handle. `create` controls whether a missing
    /// directory is created (write) or reported as absent (read → `None`). Only a
    /// genuine `NotFoundError` is absence; every other reject is an error.
    async fn curation_dir(
        create: bool,
    ) -> Result<Option<web_sys::FileSystemDirectoryHandle>, CurationStoreError> {
        let storage = web_sys::window()
            .ok_or_else(|| CurationStoreError::Io("no window".to_owned()))?
            .navigator()
            .storage();
        let root = JsFuture::from(storage.get_directory())
            .await
            .map_err(|e| transport(&e))?
            .dyn_into::<web_sys::FileSystemDirectoryHandle>()
            .map_err(|e| transport(&e))?;
        let opts = web_sys::FileSystemGetDirectoryOptions::new();
        opts.set_create(create);
        match JsFuture::from(root.get_directory_handle_with_options(DIR, &opts)).await {
            Ok(handle) => Ok(Some(
                handle
                    .dyn_into::<web_sys::FileSystemDirectoryHandle>()
                    .map_err(|e| transport(&e))?,
            )),
            Err(err) if !create && is_not_found(&err) => Ok(None),
            Err(err) => Err(transport(&err)),
        }
    }

    /// Reads `curation/store.json`. An absent directory or file is
    /// [`StoreBytes::Missing`]; a non-`NotFound` reject is a typed error. Present
    /// bytes are read losslessly (`ArrayBuffer`, not a UTF-8 text decode) so the
    /// Missing-vs-malformed split can happen faithfully in [`super::classify`].
    // OPFS handles aren't `Send`, but wasm is single-threaded and the future is
    // driven by `spawn_local`, which never requires `Send`.
    #[allow(clippy::future_not_send)]
    pub async fn opfs_load_store() -> Result<StoreBytes, CurationStoreError> {
        let Some(dir) = curation_dir(false).await? else {
            return Ok(StoreBytes::Missing);
        };
        // `create` defaults to false: a missing file is a NotFound reject → absence.
        let opts = web_sys::FileSystemGetFileOptions::new();
        let handle = match JsFuture::from(dir.get_file_handle_with_options(FILE, &opts)).await {
            Ok(handle) => handle
                .dyn_into::<web_sys::FileSystemFileHandle>()
                .map_err(|e| transport(&e))?,
            Err(err) if is_not_found(&err) => return Ok(StoreBytes::Missing),
            Err(err) => return Err(transport(&err)),
        };
        let blob = JsFuture::from(handle.get_file())
            .await
            .map_err(|e| transport(&e))?
            .dyn_into::<web_sys::Blob>()
            .map_err(|e| transport(&e))?;
        // Raw bytes, not `Blob::text()`: a UTF-8 decode could rewrite malformed
        // input before `decode_store` ever sees it.
        let buffer = JsFuture::from(blob.array_buffer())
            .await
            .map_err(|e| transport(&e))?;
        let bytes = js_sys::Uint8Array::new(&buffer).to_vec();
        Ok(StoreBytes::Present(bytes))
    }

    /// Encodes `store` to its canonical bytes and writes them to
    /// `curation/store.json`, creating the directory and file as needed.
    ///
    /// An invalid store is refused by [`encode`] before any I/O (a typed
    /// [`CurationStoreError::Encode`]), so the transport only ever carries
    /// canonical bytes. Returns once the writable stream is closed.
    #[allow(clippy::future_not_send)]
    pub async fn opfs_write_store(store: &CurationStoreV1) -> Result<(), CurationStoreError> {
        let bytes = encode(store)?;
        let dir = curation_dir(true)
            .await?
            .ok_or_else(|| CurationStoreError::Io("no curation directory".to_owned()))?;
        let opts = web_sys::FileSystemGetFileOptions::new();
        opts.set_create(true);
        let file = JsFuture::from(dir.get_file_handle_with_options(FILE, &opts))
            .await
            .map_err(|e| transport(&e))?
            .dyn_into::<web_sys::FileSystemFileHandle>()
            .map_err(|e| transport(&e))?;
        let writable = JsFuture::from(file.create_writable())
            .await
            .map_err(|e| transport(&e))?
            .dyn_into::<web_sys::FileSystemWritableFileStream>()
            .map_err(|e| transport(&e))?;
        JsFuture::from(
            writable
                .write_with_u8_array(&bytes)
                .map_err(|e| transport(&e))?,
        )
        .await
        .map_err(|e| transport(&e))?;
        JsFuture::from(writable.close())
            .await
            .map_err(|e| transport(&e))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::unwrap_used,
        clippy::panic,
        clippy::indexing_slicing
    )]

    use super::{classify, encode, StoreBytes, StoreOnDisk};
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

    #[test]
    fn encode_produces_the_canonical_bytes_not_a_second_serializer() {
        // Events out of canonical order: the real encoder sorts and dedups
        // (that is what "canonical" means); a plain serializer preserves input
        // order, so `encode` must equal core's `encode_store`, byte for byte.
        let mut store = sample();
        store.events.push(CurationEvent {
            event_id: CurationEventId("e0".to_owned()),
            occurred_at: "2026-07-18T09:00:00Z".to_owned(),
            ..store.events[0].clone()
        });
        // Now [e1@10:00, e0@09:00] — unsorted.
        assert_eq!(
            encode(&store).expect("encodes"),
            encode_store(&store).expect("canonical"),
            "encode must route through the canonical encoder"
        );
    }

    #[test]
    fn encode_refuses_an_invalid_store() {
        // Two events share an id: the domain model rejects it, and that refusal
        // must surface as a typed Encode error before any transport runs.
        let mut store = sample();
        store.events.push(store.events[0].clone());
        assert!(matches!(
            encode(&store),
            Err(super::CurationStoreError::Encode(_))
        ));
    }
}
