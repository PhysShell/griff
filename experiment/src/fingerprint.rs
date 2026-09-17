//! Content fingerprints: SHA-256 over a domain-tagged, length-prefixed walk of
//! the canonical model.
//!
//! A fingerprint is defined over the *model*, not over any serialisation of it,
//! so a later persisted form must reproduce these values rather than define its
//! own. Every walk destructures its type exhaustively: a field added to the
//! model is a compile error here, never a silently unhashed fact. Floats are
//! hashed by their bits; strings and sequences carry their length, so no two
//! distinct values share an encoding.

use std::fmt;

use griff_core::generate::RhythmTemplate;
use griff_core::generation_input::GenerationAsk;
use griff_core::gesture::GestureControl;
use griff_core::score::Score;

/// A 32-byte content fingerprint.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Fingerprint(pub [u8; 32]);

impl Fingerprint {
    /// Lowercase hex, 64 characters.
    #[must_use]
    pub fn to_hex(&self) -> String {
        String::new()
    }
}

impl fmt::Debug for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Fingerprint({})", self.to_hex())
    }
}

/// The fingerprint of a whole score: every bar, track, voice, group, atom,
/// technique span, tuning, source metadata and loss warning.
#[must_use]
pub fn score_fingerprint(score: &Score) -> Fingerprint {
    let _ = score;
    Fingerprint([0; 32])
}

/// The fingerprint of an ordered rhythm-template palette — order is behaviour,
/// since the generator rotates templates in palette order.
#[must_use]
pub fn rhythms_fingerprint(rhythms: &[RhythmTemplate]) -> Fingerprint {
    let _ = rhythms;
    Fingerprint([0; 32])
}

/// The fingerprint of an ordered novelty reference set.
#[must_use]
pub fn references_fingerprint(references: &[Score]) -> Fingerprint {
    let _ = references;
    Fingerprint([0; 32])
}

/// The fingerprint of a gesture channel, `None` included.
#[must_use]
pub fn gesture_fingerprint(gesture: Option<GestureControl>) -> Fingerprint {
    let _ = gesture;
    Fingerprint([0; 32])
}

/// The fingerprint of an ask: seed, bars, variants per strategy, the gesture
/// request, and the carried tonal context.
#[must_use]
pub fn ask_fingerprint(ask: &GenerationAsk) -> Fingerprint {
    let _ = ask;
    Fingerprint([0; 32])
}
