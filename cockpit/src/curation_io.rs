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
    // STUB (red): every read reads as absence, so the loaded/malformed tests fail.
    let _ = (
        read,
        decode_store as fn(&[u8]) -> Result<CurationStoreV1, StoreDecodeError>,
    );
    Ok(StoreOnDisk::Missing)
}

/// Loads the store from `path` on the native desktop cockpit.
///
/// # Errors
/// [`CurationStoreError::Decode`] if the file exists but is invalid;
/// [`CurationStoreError::Io`] for any read error other than "not found" (which
/// is [`StoreOnDisk::Missing`], not an error).
#[cfg(not(target_arch = "wasm32"))]
pub fn load_store(path: &std::path::Path) -> Result<StoreOnDisk, CurationStoreError> {
    // STUB (red): reports absence regardless, so the round-trip test fails.
    let _ = path;
    Ok(StoreOnDisk::Missing)
}

/// Writes the store's canonical `bytes` to `path` on the native desktop cockpit.
///
/// # Errors
/// [`CurationStoreError::Io`] if the write fails.
#[cfg(not(target_arch = "wasm32"))]
pub fn write_store(path: &std::path::Path, bytes: &[u8]) -> Result<(), CurationStoreError> {
    // STUB (red): writes nothing, so the read-back test fails.
    let _ = (path, bytes);
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

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
