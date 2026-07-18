//! Fundamental musical event types.

use core::{cmp::Ordering, num::NonZeroU32};

/// Maximum value accepted by MIDI 7-bit fields.
pub const MIDI_7_BIT_MAX: u8 = 127;

/// Validation error for core musical values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationError {
    /// MIDI pitch must be in the inclusive `0..=127` range.
    PitchOutOfRange { value: u8 },
    /// MIDI velocity must be in the inclusive `0..=127` range.
    VelocityOutOfRange { value: u8 },
    /// Tempo must be finite and greater than zero beats per minute.
    InvalidTempo,
    /// Time-signature numerator must be greater than zero.
    InvalidTimeSignatureNumerator,
    /// Time-signature denominator must be a non-zero power of two.
    InvalidTimeSignatureDenominator { value: u8 },
    /// Summing event durations exceeded the `Ticks` storage capacity.
    DurationOverflow,
    /// Tick ranges must be ordered as `start <= end`.
    InvalidTickRange,
    /// Counting musical items exceeded the platform `usize` capacity.
    CountOverflow,
    /// Confidence basis points must be in the inclusive `0..=10_000` range.
    ConfidenceOutOfRange { value: u16 },
    /// Generation requires at least one pitch to repeat.
    EmptyPitchSequence,
    /// PPQN resolution must be greater than zero.
    InvalidTicksPerQuarter,
    /// Generated note step duration must be greater than zero.
    InvalidStepDuration,
}

/// MIDI pitch number (0–127; 60 = middle C).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Pitch(pub u8);

impl Pitch {
    /// Creates a pitch after checking the MIDI 7-bit range.
    pub const fn new(value: u8) -> Result<Self, ValidationError> {
        if value <= MIDI_7_BIT_MAX {
            Ok(Self(value))
        } else {
            Err(ValidationError::PitchOutOfRange { value })
        }
    }
}

/// Duration in ticks (PPQN-relative; track resolution is carried externally).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Ticks(pub u32);

impl Ticks {
    /// Zero ticks.
    pub const ZERO: Self = Self(0);

    /// Adds two tick durations, returning an error on overflow.
    pub fn checked_add(self, rhs: Self) -> Result<Self, ValidationError> {
        self.0
            .checked_add(rhs.0)
            .map(Self)
            .ok_or(ValidationError::DurationOverflow)
    }

    /// Subtracts two tick durations, returning an error on underflow.
    pub fn checked_sub(self, rhs: Self) -> Result<Self, ValidationError> {
        self.0
            .checked_sub(rhs.0)
            .map(Self)
            .ok_or(ValidationError::InvalidTickRange)
    }
}

/// MIDI velocity (0–127).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Velocity(pub u8);

impl Velocity {
    /// Creates a velocity after checking the MIDI 7-bit range.
    pub const fn new(value: u8) -> Result<Self, ValidationError> {
        if value <= MIDI_7_BIT_MAX {
            Ok(Self(value))
        } else {
            Err(ValidationError::VelocityOutOfRange { value })
        }
    }
}

/// Microseconds in one minute — the MIDI tempo conversion constant.
const MICROS_PER_MINUTE: u32 = 60_000_000;

/// Greatest common divisor (Euclid), for canonical fraction reduction.
// The `%` cannot fault: the loop guard holds `b != 0` on every iteration.
#[allow(clippy::arithmetic_side_effects)]
const fn gcd_u32(mut a: u32, mut b: u32) -> u32 {
    while b != 0 {
        let t = a % b;
        a = b;
        b = t;
    }
    a
}

/// Tempo in beats per minute, held exactly as a GCD-reduced rational
/// `numerator / denominator` (S16 Phase 4-pre A).
///
/// Both real sources are integers — Guitar Pro stores integer BPM and MIDI
/// stores integer microseconds per quarter note — and the rational form
/// represents each exactly, so tempo equality is structural and no `f64`
/// lives in the model. [`Tempo::as_f64`] exists only for playback / UI /
/// validator boundaries; export back to MIDI goes through the split
/// [`Tempo::to_micros_per_quarter_exact`] /
/// [`Tempo::to_micros_per_quarter_nearest`] projections so an approximation
/// is a visible fact, never a silent rounding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Tempo {
    /// Reduced BPM numerator.
    num: NonZeroU32,
    /// Reduced BPM denominator.
    den: NonZeroU32,
}

impl Tempo {
    /// 120 BPM — the fallback importers substitute when a source names no
    /// usable tempo (the adapters' long-standing default).
    pub const FALLBACK_120: Self = Self {
        num: NonZeroU32::MIN.saturating_add(119),
        den: NonZeroU32::MIN,
    };

    /// Creates a tempo from an integer BPM (the Guitar Pro source form).
    pub const fn from_bpm_integer(bpm: u32) -> Result<Self, ValidationError> {
        match NonZeroU32::new(bpm) {
            Some(num) => Ok(Self {
                num,
                den: NonZeroU32::MIN,
            }),
            None => Err(ValidationError::InvalidTempo),
        }
    }

    /// Creates a tempo from microseconds per quarter note (the MIDI source
    /// form), reduced to the canonical fraction.
    // The divisions cannot fault: `gcd_u32(MICROS_PER_MINUTE, _)` divides a
    // non-zero constant, so `g >= 1`.
    #[allow(clippy::arithmetic_side_effects)]
    pub const fn from_micros_per_quarter(micros: u32) -> Result<Self, ValidationError> {
        let g = gcd_u32(MICROS_PER_MINUTE, micros);
        match (
            NonZeroU32::new(MICROS_PER_MINUTE / g),
            NonZeroU32::new(micros / g),
        ) {
            (Some(num), Some(den)) => Ok(Self { num, den }),
            _ => Err(ValidationError::InvalidTempo),
        }
    }

    /// The reduced BPM numerator.
    #[must_use]
    pub const fn bpm_numerator(self) -> u32 {
        self.num.get()
    }

    /// The reduced BPM denominator (`1` for an integer BPM).
    #[must_use]
    pub const fn bpm_denominator(self) -> u32 {
        self.den.get()
    }

    /// The exact microseconds-per-quarter form, when one exists.
    ///
    /// `None` means this tempo has no integral MIDI representation; callers
    /// must then choose [`Tempo::to_micros_per_quarter_nearest`] explicitly
    /// and report the approximation.
    // Bounded arithmetic: `total <= 60e6 * (2^32 - 1) < 2^58` fits `u64`,
    // and `num >= 1` (NonZero), so `%` and `/` cannot fault.
    #[allow(clippy::arithmetic_side_effects)]
    #[must_use]
    pub fn to_micros_per_quarter_exact(self) -> Option<u32> {
        let total = u64::from(MICROS_PER_MINUTE) * u64::from(self.den.get());
        let num = u64::from(self.num.get());
        if total % num == 0 {
            u32::try_from(total / num).ok()
        } else {
            None
        }
    }

    /// The nearest microseconds-per-quarter form (round half up, clamped to
    /// `1..=u32::MAX`). An approximation — pair it with a reported loss.
    // Bounded arithmetic: `total < 2^58` and `num / 2 < 2^32`, so the sum
    // fits `u64`; `num >= 1` (NonZero), so `/` cannot fault.
    #[allow(clippy::arithmetic_side_effects)]
    #[must_use]
    pub fn to_micros_per_quarter_nearest(self) -> u32 {
        let total = u64::from(MICROS_PER_MINUTE) * u64::from(self.den.get());
        let num = u64::from(self.num.get());
        let rounded = (total + num / 2) / num;
        u32::try_from(rounded).unwrap_or(u32::MAX).max(1)
    }

    /// The BPM as a float — a boundary projection for playback, UI, and
    /// validator dumps only; never an input to model semantics.
    #[must_use]
    pub fn as_f64(self) -> f64 {
        f64::from(self.num.get()) / f64::from(self.den.get())
    }
}

impl PartialOrd for Tempo {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Tempo {
    /// Orders by musical speed via exact cross-multiplication (no floats).
    // Bounded arithmetic: `(2^32 - 1)^2 < 2^64` fits `u64`.
    #[allow(clippy::arithmetic_side_effects)]
    fn cmp(&self, other: &Self) -> Ordering {
        let lhs = u64::from(self.num.get()) * u64::from(other.den.get());
        let rhs = u64::from(other.num.get()) * u64::from(self.den.get());
        lhs.cmp(&rhs)
    }
}

/// Confidence in basis points, `0..=10_000` (S16 Phase 4-pre A).
///
/// The exact replacement for the former `f64` confidence in `[0, 1]`:
/// `10_000` bps is full (explicit) confidence, `5_000` the documented
/// MIDI-inference placeholder. Integer, ordered, hashable — so evidence
/// participates in structural equality.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ConfidenceBps(u16);

impl ConfidenceBps {
    /// Full confidence: `10_000` bps, carried by explicit evidence.
    pub const MAX: Self = Self(10_000);

    /// Half confidence: `5_000` bps — the documented MIDI-inference
    /// placeholder baseline (ADR-0019).
    pub const HALF: Self = Self(5_000);

    /// Creates a confidence after checking the `0..=10_000` range.
    pub const fn new(value: u16) -> Result<Self, ValidationError> {
        if value <= Self::MAX.0 {
            Ok(Self(value))
        } else {
            Err(ValidationError::ConfidenceOutOfRange { value })
        }
    }

    /// The raw basis-points value.
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }
}

/// Time signature, e.g. 4/4 or 7/8.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TimeSignature {
    /// Beats per measure.
    pub numerator: u8,
    /// Beat unit as a power of two (2 = half-note, 4 = quarter-note, …).
    pub denominator: u8,
}

impl TimeSignature {
    /// Creates a time signature with a non-zero numerator and power-of-two denominator.
    pub const fn new(numerator: u8, denominator: u8) -> Result<Self, ValidationError> {
        if numerator == 0 {
            Err(ValidationError::InvalidTimeSignatureNumerator)
        } else if !denominator.is_power_of_two() {
            Err(ValidationError::InvalidTimeSignatureDenominator { value: denominator })
        } else {
            Ok(Self {
                numerator,
                denominator,
            })
        }
    }
}

/// A spanning guitar technique — one that relates notes or covers a range
/// (ADR-0018).
///
/// Per-note techniques (accent, ghost, harmonics, …) are [`NoteMark`]s;
/// harmonics are deliberately absent here so "a span is a harmonic" is
/// unrepresentable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SpanTechnique {
    /// Slide into or out of the note.
    Slide,
    /// Pitch bend up or down.
    Bend,
    /// Legato (slur).
    Legato,
    /// Palm mute.
    PalmMute,
    /// Hammer-on.
    HammerOn,
    /// Pull-off.
    PullOff,
    /// Vibrato.
    Vibrato,
    /// Let ring — the note is left to sustain into following beats.
    LetRing,
}

/// Where a technique came from — import-side provenance (ADR-0018; the
/// import-side sense of *evidence* reserved by ADR-0017).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TechniqueSource {
    /// Read from a source-of-truth format that stores it explicitly (Guitar Pro).
    Explicit,
    /// Inferred heuristically from MIDI evidence.
    InferredFromMidi,
}

/// Provenance on a technique: its [`TechniqueSource`] plus a confidence in
/// basis points (ADR-0018; exact scalars per S16 Phase 4-pre A). Keeps
/// inferred techniques from masquerading as fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TechniqueEvidence {
    /// Where the technique came from.
    pub source: TechniqueSource,
    /// Confidence in basis points; [`ConfidenceBps::MAX`] for `Explicit`.
    pub confidence: ConfidenceBps,
}

impl TechniqueEvidence {
    /// Source-of-truth evidence (e.g. Guitar Pro): `Explicit`, full
    /// confidence.
    #[must_use]
    pub const fn explicit() -> Self {
        Self {
            source: TechniqueSource::Explicit,
            confidence: ConfidenceBps::MAX,
        }
    }

    /// MIDI-inferred evidence with the given confidence.
    #[must_use]
    pub const fn inferred(confidence: ConfidenceBps) -> Self {
        Self {
            source: TechniqueSource::InferredFromMidi,
            confidence,
        }
    }
}

// ── per-note technique marks ──────────────────────────────────────────────────

/// A per-note guitar technique, intrinsic to a single note and able to co-occur
/// with others (ADR-0018). Spanning techniques (slide, legato, palm-mute, …)
/// live on [`crate::score::TechniqueSpan`] instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NoteMark {
    /// Accented (emphasised) note.
    Accent,
    /// Ghost (soft / muted) note.
    Ghost,
    /// Shortened, detached note.
    Staccato,
    /// Dead / fully-muted note.
    DeadNote,
    /// Natural harmonic.
    HarmonicNatural,
    /// Pinch (artificial) harmonic.
    HarmonicPinch,
    /// Tapped note.
    Tap,
}

impl NoteMark {
    /// All marks in declaration order — the order [`NoteMarks::iter`] yields.
    pub const ALL: [Self; 7] = [
        Self::Accent,
        Self::Ghost,
        Self::Staccato,
        Self::DeadNote,
        Self::HarmonicNatural,
        Self::HarmonicPinch,
        Self::Tap,
    ];

    /// This mark's bit within a [`NoteMarks`] set.
    const fn bit(self) -> u16 {
        match self {
            Self::Accent => 0x01,
            Self::Ghost => 0x02,
            Self::Staccato => 0x04,
            Self::DeadNote => 0x08,
            Self::HarmonicNatural => 0x10,
            Self::HarmonicPinch => 0x20,
            Self::Tap => 0x40,
        }
    }
}

/// A `Copy` set of co-occurring [`NoteMark`]s on a note, stored as a bitset
/// (ADR-0018). Evidence-free — provenance lives on spans and positions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct NoteMarks(u16);

impl NoteMarks {
    /// The empty mark set.
    #[must_use]
    pub const fn empty() -> Self {
        Self(0)
    }

    /// Whether no marks are set.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Whether `mark` is present.
    #[must_use]
    pub const fn contains(self, mark: NoteMark) -> bool {
        self.0 & mark.bit() != 0
    }

    /// Adds `mark` to the set.
    pub const fn insert(&mut self, mark: NoteMark) {
        self.0 |= mark.bit();
    }

    /// Returns this set with `mark` added (builder style).
    #[must_use]
    pub const fn with(self, mark: NoteMark) -> Self {
        Self(self.0 | mark.bit())
    }

    /// The number of marks set.
    #[must_use]
    pub const fn len(self) -> u32 {
        self.0.count_ones()
    }

    /// Iterates the marks present, in [`NoteMark::ALL`] order.
    pub fn iter(self) -> impl Iterator<Item = NoteMark> {
        NoteMark::ALL.into_iter().filter(move |&m| self.contains(m))
    }
}

// ── fretboard ───────────────────────────────────────────────────────────────

/// A `(string, fret)` location on the fretboard, interpreted under a [`Tuning`]
/// (ADR-0018).
///
/// `string` is 1-indexed in Guitar Pro order: string 1 is the highest-pitched
/// (thinnest). Carried optionally on a note — Guitar Pro supplies it, MIDI
/// usually cannot recover it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FretboardPosition {
    /// 1-indexed string number (string 1 = highest-pitched).
    pub string: u8,
    /// Fret number (0 = open string).
    pub fret: u8,
}

/// A note's chosen fretboard position together with its import-side evidence
/// (ADR-0019). Guitar Pro positions are `Explicit`; positions inferred from MIDI
/// carry `InferredFromMidi` plus a confidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NotePosition {
    /// The `(string, fret)` location.
    pub position: FretboardPosition,
    /// Where the position came from.
    pub evidence: TechniqueEvidence,
}

impl NotePosition {
    /// A source-of-truth position (e.g. Guitar Pro): `Explicit`, confidence 1.0.
    #[must_use]
    pub const fn explicit(position: FretboardPosition) -> Self {
        Self {
            position,
            evidence: TechniqueEvidence::explicit(),
        }
    }

    /// A MIDI-inferred position with the given confidence.
    #[must_use]
    pub const fn inferred(position: FretboardPosition, confidence: ConfidenceBps) -> Self {
        Self {
            position,
            evidence: TechniqueEvidence::inferred(confidence),
        }
    }
}

/// A guitar tuning: the open-string pitches ordered by string number
/// (index 0 = string 1, the highest-pitched).
///
/// The reference that maps pitch ↔ (string, fret) (ADR-0018). Carried per
/// [`crate::score::Track`]; defaults to [`Tuning::standard_e`] (ADR-0006).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Tuning {
    open_strings: Vec<Pitch>,
}

impl Tuning {
    /// Builds a tuning from open-string pitches, string 1 (highest) first.
    #[must_use]
    pub const fn new(open_strings: Vec<Pitch>) -> Self {
        Self { open_strings }
    }

    /// Standard E tuning (ADR-0006): strings 1..6 = E4 B3 G3 D3 A2 E2.
    #[must_use]
    pub fn standard_e() -> Self {
        Self {
            open_strings: vec![
                Pitch(64), // 1: E4 (high E)
                Pitch(59), // 2: B3
                Pitch(55), // 3: G3
                Pitch(50), // 4: D3
                Pitch(45), // 5: A2
                Pitch(40), // 6: E2 (low E)
            ],
        }
    }

    /// The open-string pitches, string 1 (highest) first.
    #[must_use]
    pub fn open_strings(&self) -> &[Pitch] {
        &self.open_strings
    }

    /// The pitch at `pos` under this tuning, or `None` when the string is out of
    /// range or the resulting MIDI pitch would exceed 127.
    #[must_use]
    pub fn pitch_at(&self, pos: FretboardPosition) -> Option<Pitch> {
        let idx = usize::from(pos.string.checked_sub(1)?);
        let open = self.open_strings.get(idx)?;
        let midi = open.0.checked_add(pos.fret)?;
        Pitch::new(midi).ok()
    }

    /// Every playable `(string, fret)` for `pitch` under this tuning, with `fret`
    /// in `[0, max_fret]`, ordered by ascending string number (ADR-0019). Empty
    /// when the pitch is below every open string or beyond `max_fret` on all.
    #[must_use]
    pub fn candidates(&self, pitch: Pitch, max_fret: u8) -> Vec<FretboardPosition> {
        self.open_strings
            .iter()
            .enumerate()
            .filter_map(|(idx, open)| {
                let fret = pitch.0.checked_sub(open.0)?;
                if fret > max_fret {
                    return None;
                }
                let string = u8::try_from(idx).ok()?.checked_add(1)?;
                Some(FretboardPosition { string, fret })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::{Pitch, Tempo, Ticks, TimeSignature, ValidationError, Velocity};

    #[test]
    fn pitch_accepts_midi_range() {
        assert_eq!(Pitch::new(0), Ok(Pitch(0)));
        assert_eq!(Pitch::new(127), Ok(Pitch(127)));
    }

    #[test]
    fn pitch_rejects_values_outside_midi_range() {
        assert_eq!(
            Pitch::new(128),
            Err(ValidationError::PitchOutOfRange { value: 128 }),
        );
    }

    #[test]
    fn velocity_accepts_midi_range() {
        assert_eq!(Velocity::new(0), Ok(Velocity(0)));
        assert_eq!(Velocity::new(127), Ok(Velocity(127)));
    }

    #[test]
    fn velocity_rejects_values_outside_midi_range() {
        assert_eq!(
            Velocity::new(128),
            Err(ValidationError::VelocityOutOfRange { value: 128 }),
        );
    }

    #[test]
    fn tempo_accepts_positive_integer_bpm() {
        let tempo = Tempo::from_bpm_integer(120);
        assert_eq!(tempo.map(Tempo::bpm_numerator), Ok(120));
        assert_eq!(tempo.map(Tempo::bpm_denominator), Ok(1));
        assert_eq!(tempo, Ok(Tempo::FALLBACK_120));
    }

    #[test]
    fn tempo_rejects_zero_inputs() {
        assert_eq!(
            Tempo::from_bpm_integer(0),
            Err(ValidationError::InvalidTempo)
        );
        assert_eq!(
            Tempo::from_micros_per_quarter(0),
            Err(ValidationError::InvalidTempo)
        );
    }

    #[test]
    fn time_signature_accepts_common_meter() {
        assert_eq!(
            TimeSignature::new(4, 4),
            Ok(TimeSignature {
                numerator: 4,
                denominator: 4
            }),
        );
    }

    #[test]
    fn time_signature_rejects_invalid_components() {
        assert_eq!(
            TimeSignature::new(0, 4),
            Err(ValidationError::InvalidTimeSignatureNumerator),
        );
        assert_eq!(
            TimeSignature::new(4, 3),
            Err(ValidationError::InvalidTimeSignatureDenominator { value: 3 }),
        );
    }

    #[test]
    fn ticks_checked_add_reports_overflow() {
        assert_eq!(Ticks(1).checked_add(Ticks(2)), Ok(Ticks(3)));
        assert_eq!(
            Ticks(u32::MAX).checked_add(Ticks(1)),
            Err(ValidationError::DurationOverflow),
        );
    }

    #[test]
    fn ticks_checked_sub_reports_underflow() {
        assert_eq!(Ticks(3).checked_sub(Ticks(1)), Ok(Ticks(2)));
        assert_eq!(
            Ticks(1).checked_sub(Ticks(3)),
            Err(ValidationError::InvalidTickRange),
        );
    }
}
