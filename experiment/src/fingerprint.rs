//! Content fingerprints: SHA-256 over a domain-tagged, length-prefixed walk of
//! the canonical semantic projection ([`crate::projection`]).
//!
//! There is one canonicalization (ADR-0034 decision 5): the bundle serialises
//! the projection and every fingerprint walks it. Walks destructure the
//! projection types exhaustively, so a projection field is never silently
//! unhashed. Floats are hashed by their bits; strings and sequences carry their
//! length, so no two distinct values share an encoding.

use std::fmt;
use std::fmt::Write as _;

use griff_core::generate::RhythmTemplate;
use griff_core::generation_input::GenerationAsk;
use griff_core::gesture::GestureControl;
use griff_core::score::Score;
use sha2::{Digest, Sha256};

use crate::projection::{GenerationAskV1, GestureControlV1, RhythmTemplateV1, ScoreV1};

/// A 32-byte content fingerprint.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Fingerprint(pub [u8; 32]);

impl Fingerprint {
    /// Lowercase hex, 64 characters.
    #[must_use]
    pub fn to_hex(&self) -> String {
        self.0.iter().fold(String::with_capacity(64), |mut acc, b| {
            write!(acc, "{b:02x}").ok();
            acc
        })
    }
}

impl fmt::Debug for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Fingerprint({})", self.to_hex())
    }
}

/// An incremental, domain-tagged fingerprint builder.
pub(crate) struct Hasher(Sha256);

impl Hasher {
    /// A builder whose every output is separated from other domains' by `domain`.
    pub(crate) fn new(domain: &str) -> Self {
        let mut hasher = Self(Sha256::new());
        hasher.str(domain);
        hasher
    }

    pub(crate) fn u8(&mut self, v: u8) {
        self.0.update([v]);
    }

    pub(crate) fn u16(&mut self, v: u16) {
        self.0.update(v.to_le_bytes());
    }

    pub(crate) fn u32(&mut self, v: u32) {
        self.0.update(v.to_le_bytes());
    }

    pub(crate) fn u64(&mut self, v: u64) {
        self.0.update(v.to_le_bytes());
    }

    pub(crate) fn usize(&mut self, v: usize) {
        self.u64(u64::try_from(v).unwrap_or(u64::MAX));
    }

    pub(crate) fn bool(&mut self, v: bool) {
        self.u8(u8::from(v));
    }

    pub(crate) fn f64(&mut self, v: f64) {
        self.u64(v.to_bits());
    }

    pub(crate) fn str(&mut self, s: &str) {
        self.usize(s.len());
        self.0.update(s.as_bytes());
    }

    pub(crate) fn fingerprint(&mut self, fp: Fingerprint) {
        self.0.update(fp.0);
    }

    pub(crate) fn option_str(&mut self, s: Option<&str>) {
        match s {
            None => self.u8(0),
            Some(s) => {
                self.u8(1);
                self.str(s);
            }
        }
    }

    pub(crate) fn option_fingerprint(&mut self, fp: Option<Fingerprint>) {
        match fp {
            None => self.u8(0),
            Some(fp) => {
                self.u8(1);
                self.fingerprint(fp);
            }
        }
    }

    pub(crate) fn finish(self) -> Fingerprint {
        Fingerprint(self.0.finalize().into())
    }
}

/// The fingerprint of a whole score — the walk of its projection
/// ([`ScoreV1::fingerprint`]).
#[must_use]
pub fn score_fingerprint(score: &Score) -> Fingerprint {
    ScoreV1::from(score).fingerprint()
}

/// The fingerprint of an ordered rhythm-template palette — order is behaviour,
/// since the generator rotates templates in palette order.
#[must_use]
pub fn rhythms_fingerprint(rhythms: &[RhythmTemplate]) -> Fingerprint {
    RhythmTemplateV1::fingerprint_all(
        &rhythms
            .iter()
            .map(RhythmTemplateV1::from)
            .collect::<Vec<_>>(),
    )
}

/// The fingerprint of an ordered novelty reference set.
#[must_use]
pub fn references_fingerprint(references: &[Score]) -> Fingerprint {
    references_fingerprint_of(references.iter().map(score_fingerprint))
}

/// The reference-set fingerprint over already-computed score fingerprints, in
/// order.
pub(crate) fn references_fingerprint_of(
    scores: impl ExactSizeIterator<Item = Fingerprint>,
) -> Fingerprint {
    let mut h = Hasher::new("griff.references.v1");
    h.usize(scores.len());
    for score in scores {
        h.fingerprint(score);
    }
    h.finish()
}

/// The fingerprint of a gesture channel, `None` included.
#[must_use]
pub fn gesture_fingerprint(gesture: Option<GestureControl>) -> Fingerprint {
    GestureControlV1::fingerprint_option(gesture.map(GestureControlV1::from))
}

/// The fingerprint of an ask: seed, bars, variants per strategy, the gesture
/// request, and the carried tonal context.
#[must_use]
pub fn ask_fingerprint(ask: &GenerationAsk) -> Fingerprint {
    GenerationAskV1::from(ask).fingerprint()
}
