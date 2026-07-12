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

    /// A contiguous window of the ladder spanning at most one octave (12
    /// semitones), positioned by `selector` (a candidate-derived index).
    ///
    /// The window's low anchor is chosen from the rungs that leave a full
    /// octave above them (or the bottom rung when the whole ladder is narrower
    /// than an octave), so a top-edge selector does not shrink the window;
    /// across selectors the windows cover the entire ladder. Never empty.
    #[must_use]
    pub fn octave_window(&self, selector: usize) -> LadderWindow<'_> {
        const SPAN: u8 = 12;
        let top = self.pitches.last().map_or(0, |p| p.0);
        // Anchors that leave a full octave above them: rungs at or below
        // `top - SPAN`. When the ladder is narrower than an octave, only the
        // bottom rung anchors (the window is then the whole ladder).
        let anchor_ceiling = top.saturating_sub(SPAN);
        let anchor_count = self
            .pitches
            .iter()
            .take_while(|p| p.0 <= anchor_ceiling)
            .count()
            .max(1);
        let anchor = selector.checked_rem(anchor_count).unwrap_or(0);
        let anchor_pitch = self.pitches.get(anchor).map_or(top, |p| p.0);
        let window_ceiling = anchor_pitch.saturating_add(SPAN);
        let end = self
            .pitches
            .iter()
            .rposition(|p| p.0 <= window_ceiling)
            .unwrap_or(anchor);
        LadderWindow {
            ladder: self,
            start: anchor,
            len: end.saturating_sub(anchor).saturating_add(1),
        }
    }
}

/// A contiguous, at-most-one-octave slice of a [`ScaleLadder`].
///
/// The local register one candidate stays within. The full ladder remains the
/// source of reachability; the window is the locally-coherent subset.
#[derive(Debug, Clone, Copy)]
pub struct LadderWindow<'a> {
    ladder: &'a ScaleLadder,
    start: usize,
    len: usize,
}

impl LadderWindow<'_> {
    /// Number of rungs in the window (always ≥ 1).
    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Always `false` — a window is never empty; present for lint parity.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The pitch at window-relative `degree`, clamped to the window ends.
    #[must_use]
    pub fn at(&self, degree: usize) -> Pitch {
        let within = degree.min(self.len.saturating_sub(1));
        self.ladder.at(self.start.saturating_add(within))
    }

    /// The window's rungs, ascending.
    #[must_use]
    pub fn pitches(&self) -> &[Pitch] {
        let end = self.start.saturating_add(self.len).min(self.ladder.len());
        self.ladder.pitches().get(self.start..end).unwrap_or(&[])
    }
}

/// Diagnostic register statistics of a pitch line.
///
/// For tests and the A/B harness (arbiter). **Purely observational**: never
/// wired into [`rerank_weights_v1`](crate::rerank::rerank_weights_v1);
/// repairing candidate generation is separate from ranking.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RegisterStats {
    /// Mean absolute interval between successive notes, in semitones.
    pub mean_abs_interval: f64,
    /// Largest absolute interval between successive notes, in semitones.
    pub max_abs_interval: u8,
    /// Share of successive intervals larger than an octave (> 12 semitones).
    pub octave_leap_share: f64,
    /// Population standard deviation of the pitches.
    pub pitch_stddev: f64,
}

impl RegisterStats {
    /// Measures the register shape of `pitches` (in program order). An empty or
    /// single-note line reports zeros.
    #[must_use]
    pub fn measure(pitches: &[u8]) -> Self {
        let intervals: Vec<u16> = pitches
            .windows(2)
            .filter_map(|w| match w {
                [a, b] => Some(u16::from((*a).abs_diff(*b))),
                _ => None,
            })
            .collect();
        let (mean_abs_interval, max_abs_interval, octave_leap_share) = if intervals.is_empty() {
            (0.0, 0, 0.0)
        } else {
            #[allow(clippy::cast_precision_loss)] // counts are tiny
            let n = intervals.len() as f64;
            let sum: u32 = intervals.iter().map(|&i| u32::from(i)).sum();
            let max = intervals.iter().copied().max().unwrap_or(0);
            let leaps = intervals.iter().filter(|&&i| i > 12).count();
            #[allow(clippy::cast_precision_loss)]
            let mean = f64::from(sum) / n;
            #[allow(clippy::cast_precision_loss)]
            let share = leaps as f64 / n;
            #[allow(clippy::cast_possible_truncation)] // ≤ 127 by MIDI range
            (mean, max.min(255) as u8, share)
        };
        Self {
            mean_abs_interval,
            max_abs_interval,
            octave_leap_share,
            pitch_stddev: stddev(pitches),
        }
    }
}

/// Population standard deviation of a pitch line (0 for < 2 notes).
fn stddev(pitches: &[u8]) -> f64 {
    if pitches.len() < 2 {
        return 0.0;
    }
    #[allow(clippy::cast_precision_loss)] // small counts / MIDI range
    let n = pitches.len() as f64;
    let mean = pitches.iter().map(|&p| f64::from(p)).sum::<f64>() / n;
    let var = pitches
        .iter()
        .map(|&p| (f64::from(p) - mean).powi(2))
        .sum::<f64>()
        / n;
    var.sqrt()
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
