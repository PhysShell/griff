//! Shared pitch-selection primitives for the generator and the complement
//! arranger (register increment, 2026-07-12).
//!
//! One degree→pitch mapper, not two: a [`ScaleLadder`] is the ascending list
//! of every pitch in a [`PitchRange`] whose pitch class is in a
//! [`PitchClassSet`], and a linear *degree* indexes into it. This replaces the
//! octave-walking `PitchMaterial::pitch_at`, whose degree window pinned some
//! strategies to a single octave above the material's anchor, and the private
//! `band_scale_ladder` in `complement` (which now delegates here).
//!
//! The anchor pitch of the source material contributes only its *pitch class*
//! to the palette — it is **not** a tonal center. Tonal-center inference is a
//! separate, later increment; until then no code treats the input's minimum
//! pitch as a tonic.

use crate::event::Pitch;

/// An inclusive MIDI pitch range, normalised so `lo <= hi` and both are valid
/// MIDI notes (`0..=127`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PitchRange {
    /// Inclusive lower bound.
    pub lo: Pitch,
    /// Inclusive upper bound.
    pub hi: Pitch,
}

impl PitchRange {
    /// Builds a range from two bounds in any order, clamped to valid MIDI.
    #[must_use]
    pub fn new(a: Pitch, b: Pitch) -> Self {
        let lo = a.0.min(b.0);
        let hi = a.0.max(b.0).min(127);
        Self {
            lo: Pitch(lo.min(hi)),
            hi: Pitch(hi),
        }
    }
}

/// A set of pitch classes (`0..=11`), sorted and deduplicated.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PitchClassSet {
    classes: Vec<u8>,
}

impl PitchClassSet {
    /// Builds a class set from arbitrary semitone values, folded to `0..=11`.
    #[must_use]
    pub fn new(classes: impl IntoIterator<Item = u8>) -> Self {
        let mut classes: Vec<u8> = classes.into_iter().map(|c| c % 12).collect();
        classes.sort_unstable();
        classes.dedup();
        Self { classes }
    }

    /// `true` when the set holds no classes.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.classes.is_empty()
    }

    /// `true` when `pitch`'s class is in the set.
    #[must_use]
    pub fn contains_pitch(&self, pitch: Pitch) -> bool {
        self.classes.contains(&(pitch.0 % 12))
    }

    /// The classes, ascending.
    #[must_use]
    pub fn classes(&self) -> &[u8] {
        &self.classes
    }
}

/// Why a [`ScaleLadder`] cannot be built: the palette selects no pitch, so no
/// degree can resolve in-class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PitchSelectionError {
    /// The [`PitchClassSet`] is empty — no classes to select.
    EmptyPitchClassSet,
    /// The palette is non-empty, but no allowed pitch falls in the range.
    NoAllowedPitchInRange,
}

/// The ascending ladder of every pitch in a [`PitchRange`] whose class is in a
/// [`PitchClassSet`].
///
/// Never empty by construction: [`build`](ScaleLadder::build) returns
/// [`PitchSelectionError`] rather than fall back to an out-of-palette pitch, so
/// every rung — and every degree — is guaranteed in-class.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScaleLadder {
    pitches: Vec<Pitch>,
}

impl ScaleLadder {
    /// Builds the ladder over `range` from the `classes` palette, or reports
    /// why no in-class pitch is available (empty palette, or none in range).
    ///
    /// # Errors
    /// [`PitchSelectionError::EmptyPitchClassSet`] when `classes` is empty;
    /// [`PitchSelectionError::NoAllowedPitchInRange`] when the palette is
    /// non-empty but no allowed pitch falls in `range`.
    pub fn build(range: &PitchRange, classes: &PitchClassSet) -> Result<Self, PitchSelectionError> {
        if classes.is_empty() {
            return Err(PitchSelectionError::EmptyPitchClassSet);
        }
        let pitches: Vec<Pitch> = (range.lo.0..=range.hi.0)
            .map(Pitch)
            .filter(|&p| classes.contains_pitch(p))
            .collect();
        if pitches.is_empty() {
            return Err(PitchSelectionError::NoAllowedPitchInRange);
        }
        Ok(Self { pitches })
    }

    /// Number of rungs (always ≥ 1).
    #[must_use]
    pub fn len(&self) -> usize {
        self.pitches.len()
    }

    /// Always `false` — the ladder is never empty; present for lint parity.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.pitches.is_empty()
    }

    /// The pitch at `degree`, **clamped** to the ladder ends: a degree past the
    /// top lands on the highest rung (it does not wrap or leave the range).
    #[must_use]
    pub fn at(&self, degree: usize) -> Pitch {
        let idx = degree.min(self.pitches.len().saturating_sub(1));
        // `idx < len` and the ladder is non-empty, so `get` is always `Some`;
        // the fallback is unreachable.
        self.pitches.get(idx).copied().unwrap_or(Pitch(0))
    }

    /// The rungs, ascending.
    #[must_use]
    pub fn pitches(&self) -> &[Pitch] {
        &self.pitches
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::indexing_slicing)]

    use super::{PitchClassSet, PitchRange, ScaleLadder};
    use crate::event::Pitch;

    #[test]
    fn ladder_at_clamps_past_the_top() {
        let classes = PitchClassSet::new([0, 7]); // C, G
        let ladder =
            ScaleLadder::build(&PitchRange::new(Pitch(48), Pitch(60)), &classes).expect("in-class");
        // C3(48) G3(55) C4(60)
        assert_eq!(ladder.pitches(), &[Pitch(48), Pitch(55), Pitch(60)]);
        assert_eq!(ladder.at(0), Pitch(48));
        assert_eq!(ladder.at(2), Pitch(60));
        assert_eq!(ladder.at(99), Pitch(60), "clamps to the top rung");
    }
}
