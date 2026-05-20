//! Deterministic phrase generation primitives.

use crate::event::{
    Articulation, Bar, Event, Note, Phrase, Pitch, Tempo, Ticks, TimeSignature, ValidationError,
    Velocity,
};

// ── S6: rule-based generator ──────────────────────────────────────────────────

/// Deterministic seed for rule-based generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GenerationSeed(pub u64);

/// A pitch scale expressed as a root MIDI note plus semitone offsets from it.
#[derive(Debug, Clone)]
pub struct PitchMaterial {
    /// Root MIDI pitch (0–127).
    pub root: Pitch,
    /// Semitone offsets from root (typically 0–11) that define the scale.
    pub intervals: Vec<u8>,
}

impl PitchMaterial {
    /// Maps a linear `degree` (unbounded) to a MIDI pitch, clamped to `[lo, hi]`.
    ///
    /// Successive degrees walk up the scale; each full cycle adds one octave.
    fn pitch_at(&self, degree: usize, lo: Pitch, hi: Pitch) -> Pitch {
        let scale_len = self.intervals.len();
        // scale_len >= 1 guaranteed by EmptyPitchMaterial guard in generate().
        let octave = degree.checked_div(scale_len).unwrap_or(0);
        let idx = degree.checked_rem(scale_len).unwrap_or(0);
        let interval = self.intervals.get(idx).copied().unwrap_or(0);
        // octave * 12 capped at 127 via u8 cast; saturating_add avoids overflow
        let octave_offset = u8::try_from(octave.saturating_mul(12)).unwrap_or(127_u8);
        let raw = self
            .root
            .0
            .saturating_add(interval)
            .saturating_add(octave_offset);
        let actual_lo = lo.0.min(hi.0);
        let actual_hi = lo.0.max(hi.0).min(127);
        Pitch(raw.clamp(actual_lo, actual_hi))
    }
}

/// Hard constraints that all generated phrases must satisfy.
#[derive(Debug, Clone, Copy)]
pub struct GenerationConstraints {
    /// Number of bars to generate (must be ≥ 1).
    pub bar_count: usize,
    /// Meter applied to every generated bar.
    pub time_signature: TimeSignature,
    /// Tempo applied to every generated bar.
    pub tempo: Tempo,
    /// Tick resolution (pulses per quarter note); must be > 0.
    pub ticks_per_quarter: Ticks,
    /// Inclusive lower bound on generated note pitches.
    pub pitch_lo: Pitch,
    /// Inclusive upper bound on generated note pitches.
    pub pitch_hi: Pitch,
}

/// Which rule-based strategy to apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GenerationStrategy {
    /// Copy a rhythm template from the corpus and substitute pitches from the scale.
    RhythmCopyPitchSubstitute,
    /// Build a short motif and transpose it for each bar.
    MotifTransposeVariation,
    /// Random walk on scale degrees, penalising large leaps (≤ 12 st).
    ConstrainedRandomWalk,
    /// Pick scale degrees at random and shuffle them bar by bar.
    ShuffleMotifs,
    /// Repeat the first bar, replacing the last note on subsequent bars.
    RepeatVariation,
}

/// A generated phrase with provenance metadata.
#[derive(Debug, Clone, PartialEq)]
pub struct GenerationCandidate {
    /// The generated phrase.
    pub phrase: Phrase,
    /// Strategy that produced this candidate.
    pub strategy: GenerationStrategy,
    /// Seed used for this generation pass.
    pub seed: GenerationSeed,
}

/// Everything a single rule-based generation pass needs.
#[derive(Debug, Clone)]
pub struct RuleGenerationRequest {
    /// Deterministic PRNG seed.
    pub seed: GenerationSeed,
    /// Scale to draw pitches from.
    pub pitch_material: PitchMaterial,
    /// Structural constraints.
    pub constraints: GenerationConstraints,
    /// Rhythm templates extracted from the corpus (inner `Vec` = note durations per bar).
    pub source_rhythms: Vec<Vec<Ticks>>,
    /// Strategy to apply.
    pub strategy: GenerationStrategy,
}

/// Errors the rule generator can emit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenerationError {
    /// `pitch_material.intervals` is empty.
    EmptyPitchMaterial,
    /// A constraint value (time signature, PPQN, …) is invalid or yields a zero bar duration.
    InvalidConstraints,
    /// `constraints.bar_count` is zero.
    BarCountZero,
    /// `RhythmCopyPitchSubstitute` requires at least one non-empty source rhythm.
    RhythmTemplateMissing,
}

/// Generates a single candidate phrase using the requested rule-based strategy.
///
/// The output is fully deterministic: the same `request` always produces the
/// same `GenerationCandidate`.
pub fn generate(request: &RuleGenerationRequest) -> Result<GenerationCandidate, GenerationError> {
    if request.pitch_material.intervals.is_empty() {
        return Err(GenerationError::EmptyPitchMaterial);
    }
    if request.constraints.bar_count == 0 {
        return Err(GenerationError::BarCountZero);
    }
    if request.strategy == GenerationStrategy::RhythmCopyPitchSubstitute
        && request.source_rhythms.first().map_or(true, Vec::is_empty)
    {
        return Err(GenerationError::RhythmTemplateMissing);
    }

    let bar_duration = bar_duration_ticks(
        request.constraints.time_signature,
        request.constraints.ticks_per_quarter,
    )
    .map_err(|_| GenerationError::InvalidConstraints)?;
    if bar_duration == Ticks::ZERO {
        return Err(GenerationError::InvalidConstraints);
    }

    let mut prng = Xorshift64::new(request.seed.0);
    let c = &request.constraints;
    let pm = &request.pitch_material;

    let bars = match request.strategy {
        GenerationStrategy::RhythmCopyPitchSubstitute => {
            strategy_rhythm_copy(c, pm, &request.source_rhythms, &mut prng, bar_duration)
        }
        GenerationStrategy::MotifTransposeVariation => {
            strategy_motif_transpose(c, pm, &mut prng, bar_duration)
        }
        GenerationStrategy::ConstrainedRandomWalk => {
            strategy_constrained_walk(c, pm, &mut prng, bar_duration)
        }
        GenerationStrategy::ShuffleMotifs => {
            strategy_shuffle_motifs(c, pm, &mut prng, bar_duration)
        }
        GenerationStrategy::RepeatVariation => {
            strategy_repeat_variation(c, pm, &mut prng, bar_duration)
        }
    };

    Ok(GenerationCandidate {
        phrase: Phrase { bars },
        strategy: request.strategy,
        seed: request.seed,
    })
}

// ── PRNG ──────────────────────────────────────────────────────────────────────

/// Xorshift64 deterministic PRNG.  Requires nonzero state.
struct Xorshift64(u64);

impl Xorshift64 {
    const fn new(seed: u64) -> Self {
        // Xorshift requires nonzero state; fall back to a fixed nonzero constant.
        Self(if seed == 0 {
            6_364_136_223_846_793_005_u64
        } else {
            seed
        })
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x.wrapping_shl(13);
        x ^= x.wrapping_shr(7);
        x ^= x.wrapping_shl(17);
        self.0 = x;
        x
    }

    /// Returns a value in `0..n`; returns 0 when `n <= 1`.
    fn next_mod(&mut self, n: usize) -> usize {
        if n <= 1 {
            return 0;
        }
        let n64 = u64::try_from(n).unwrap_or(u64::MAX);
        let rem = self.next_u64().checked_rem(n64).unwrap_or(0);
        // rem < n64 <= usize::MAX as u64, so the cast is lossless.
        usize::try_from(rem).unwrap_or(0)
    }
}

// ── helpers ───────────────────────────────────────────────────────────────────

/// Returns the next degree in the scale, wrapping at `scale_len`.
const fn advance_degree(degree: usize, scale_len: usize) -> usize {
    let next = degree.wrapping_add(1);
    if next >= scale_len {
        0
    } else {
        next
    }
}

/// Returns the next index into a slice of length `len`, wrapping at `len`.
const fn advance_idx(idx: usize, len: usize) -> usize {
    let next = idx.wrapping_add(1);
    if next >= len {
        0
    } else {
        next
    }
}

/// Clamps a tick duration so it does not exceed `remaining`.
const fn fit_duration(raw: Ticks, remaining: Ticks) -> Ticks {
    if raw.0 > remaining.0 {
        remaining
    } else {
        raw
    }
}

// ── strategies ────────────────────────────────────────────────────────────────

fn strategy_rhythm_copy(
    c: &GenerationConstraints,
    pm: &PitchMaterial,
    source_rhythms: &[Vec<Ticks>],
    prng: &mut Xorshift64,
    bar_duration: Ticks,
) -> Vec<Bar> {
    // Validated non-empty before entering; first() is guaranteed Some.
    let template: &[Ticks] = source_rhythms.first().map_or(&[], Vec::as_slice);

    let scale_len = pm.intervals.len();
    let mut degree = prng.next_mod(scale_len);
    let mut bars = Vec::with_capacity(c.bar_count);

    for _ in 0..c.bar_count {
        let mut events = Vec::new();
        let mut remaining = bar_duration;
        let mut tidx = 0_usize;

        while remaining > Ticks::ZERO {
            let raw = template.get(tidx).copied().unwrap_or(Ticks(480));
            let duration = fit_duration(raw, remaining);
            if duration == Ticks::ZERO {
                break;
            }
            events.push(Event::Note(Note {
                pitch: pm.pitch_at(degree, c.pitch_lo, c.pitch_hi),
                duration,
                velocity: Velocity(90),
                articulation: None,
            }));
            remaining = Ticks(remaining.0.saturating_sub(duration.0));
            degree = advance_degree(degree, scale_len);
            tidx = advance_idx(tidx, template.len().max(1));
        }

        bars.push(Bar {
            time_signature: c.time_signature,
            tempo: c.tempo,
            events,
        });
    }
    bars
}

fn strategy_motif_transpose(
    c: &GenerationConstraints,
    pm: &PitchMaterial,
    prng: &mut Xorshift64,
    bar_duration: Ticks,
) -> Vec<Bar> {
    const MOTIF_LEN: usize = 4;
    // Transpositions in semitones applied cyclically per bar.
    const TRANSPOSES: [i8; 7] = [0, 3, 5, 7, -3, -5, -7];

    let scale_len = pm.intervals.len();
    let start_degree = prng.next_mod(scale_len);
    let step = Ticks(c.ticks_per_quarter.0);

    let mut bars = Vec::with_capacity(c.bar_count);

    for bi in 0..c.bar_count {
        let transpose = TRANSPOSES
            .get(bi.checked_rem(TRANSPOSES.len()).unwrap_or(0))
            .copied()
            .unwrap_or(0_i8);
        let mut events = Vec::new();
        let mut cursor = Ticks::ZERO;
        let mut note_idx = 0_usize;

        while cursor < bar_duration {
            let motif_pos = note_idx.checked_rem(MOTIF_LEN).unwrap_or(0);
            let degree = (start_degree.wrapping_add(motif_pos))
                .checked_rem(scale_len)
                .unwrap_or(0);
            let base = pm.pitch_at(degree, c.pitch_lo, c.pitch_hi);
            let transposed = apply_transpose(base, transpose, c.pitch_lo, c.pitch_hi);

            let remaining = Ticks(bar_duration.0.saturating_sub(cursor.0));
            let duration = fit_duration(step, remaining);
            if duration == Ticks::ZERO {
                break;
            }
            events.push(Event::Note(Note {
                pitch: transposed,
                duration,
                velocity: Velocity(85),
                articulation: None,
            }));
            cursor = Ticks(cursor.0.saturating_add(duration.0));
            note_idx = note_idx.wrapping_add(1);
        }

        bars.push(Bar {
            time_signature: c.time_signature,
            tempo: c.tempo,
            events,
        });
    }
    bars
}

fn strategy_constrained_walk(
    c: &GenerationConstraints,
    pm: &PitchMaterial,
    prng: &mut Xorshift64,
    bar_duration: Ticks,
) -> Vec<Bar> {
    let scale_len = pm.intervals.len();
    // Two-octave degree range keeps consecutive leaps ≤ 12 semitones.
    let max_degree = scale_len.saturating_mul(2).saturating_sub(1);
    let step = Ticks(c.ticks_per_quarter.0);
    let mut degree = prng.next_mod(scale_len);

    let mut bars = Vec::with_capacity(c.bar_count);

    for _ in 0..c.bar_count {
        let mut events = Vec::new();
        let mut cursor = Ticks::ZERO;

        while cursor < bar_duration {
            let pitch = pm.pitch_at(degree, c.pitch_lo, c.pitch_hi);
            let remaining = Ticks(bar_duration.0.saturating_sub(cursor.0));
            let duration = fit_duration(step, remaining);
            if duration == Ticks::ZERO {
                break;
            }
            events.push(Event::Note(Note {
                pitch,
                duration,
                velocity: Velocity(80),
                articulation: None,
            }));
            cursor = Ticks(cursor.0.saturating_add(duration.0));

            // Walk ±1 degree, bounded to [0, max_degree].
            if prng.next_u64() & 1 == 0 {
                degree = degree.saturating_sub(1);
            } else {
                degree = degree.saturating_add(1).min(max_degree);
            }
        }

        bars.push(Bar {
            time_signature: c.time_signature,
            tempo: c.tempo,
            events,
        });
    }
    bars
}

fn strategy_shuffle_motifs(
    c: &GenerationConstraints,
    pm: &PitchMaterial,
    prng: &mut Xorshift64,
    bar_duration: Ticks,
) -> Vec<Bar> {
    let scale_len = pm.intervals.len();
    let step = Ticks(c.ticks_per_quarter.0);

    let mut bars = Vec::with_capacity(c.bar_count);

    for _ in 0..c.bar_count {
        let mut events = Vec::new();
        let mut cursor = Ticks::ZERO;

        while cursor < bar_duration {
            let degree = prng.next_mod(scale_len);
            let pitch = pm.pitch_at(degree, c.pitch_lo, c.pitch_hi);
            let remaining = Ticks(bar_duration.0.saturating_sub(cursor.0));
            let duration = fit_duration(step, remaining);
            if duration == Ticks::ZERO {
                break;
            }
            events.push(Event::Note(Note {
                pitch,
                duration,
                velocity: Velocity(88),
                articulation: None,
            }));
            cursor = Ticks(cursor.0.saturating_add(duration.0));
        }

        bars.push(Bar {
            time_signature: c.time_signature,
            tempo: c.tempo,
            events,
        });
    }
    bars
}

fn strategy_repeat_variation(
    c: &GenerationConstraints,
    pm: &PitchMaterial,
    prng: &mut Xorshift64,
    bar_duration: Ticks,
) -> Vec<Bar> {
    let scale_len = pm.intervals.len();
    let step = Ticks(c.ticks_per_quarter.0);
    let base_degree = prng.next_mod(scale_len);

    let base_bar = build_ascending_bar(c, pm, base_degree, step, bar_duration);
    let mut bars = Vec::with_capacity(c.bar_count);
    bars.push(base_bar.clone());

    // Variation degree: advance 2 steps from base so the pitch always differs.
    let var_degree = {
        let d = base_degree.wrapping_add(2);
        if d < scale_len {
            d
        } else {
            d.saturating_sub(scale_len)
        }
    };
    let var_pitch = pm.pitch_at(var_degree, c.pitch_lo, c.pitch_hi);

    for _ in 1..c.bar_count {
        let mut varied = base_bar.clone();
        // Replace the last event (always a Note) with the variation pitch.
        if let Some(Event::Note(ref mut n)) = varied.events.last_mut() {
            n.pitch = var_pitch;
        }
        bars.push(varied);
    }
    bars
}

/// Builds one bar of ascending scale-degree notes at `step` duration each.
fn build_ascending_bar(
    c: &GenerationConstraints,
    pm: &PitchMaterial,
    start_degree: usize,
    step: Ticks,
    bar_duration: Ticks,
) -> Bar {
    let scale_len = pm.intervals.len();
    // Two-octave ceiling mirrors the walk strategy.
    let max_degree = scale_len.saturating_mul(2).saturating_sub(1);
    let mut events = Vec::new();
    let mut cursor = Ticks::ZERO;
    let mut degree = start_degree;

    while cursor < bar_duration {
        let pitch = pm.pitch_at(degree, c.pitch_lo, c.pitch_hi);
        let remaining = Ticks(bar_duration.0.saturating_sub(cursor.0));
        let duration = fit_duration(step, remaining);
        if duration == Ticks::ZERO {
            break;
        }
        events.push(Event::Note(Note {
            pitch,
            duration,
            velocity: Velocity(92),
            articulation: None,
        }));
        cursor = Ticks(cursor.0.saturating_add(duration.0));
        degree = degree.saturating_add(1).min(max_degree);
    }

    Bar {
        time_signature: c.time_signature,
        tempo: c.tempo,
        events,
    }
}

/// Transposes a pitch by `semitones`, clamping the result to `[lo, hi]`.
fn apply_transpose(pitch: Pitch, semitones: i8, lo: Pitch, hi: Pitch) -> Pitch {
    let raw = i16::from(pitch.0).saturating_add(i16::from(semitones));
    let actual_lo = i16::from(lo.0.min(hi.0));
    let actual_hi = i16::from(lo.0.max(hi.0).min(127));
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    Pitch(raw.clamp(actual_lo, actual_hi) as u8)
}

/// Pattern repeated by the built-in phrase generator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepeatingPattern {
    /// Ordered pitches to emit as notes.
    pub pitches: Vec<Pitch>,
    /// Duration of each generated note.
    pub step: Ticks,
    /// Velocity used for every generated note.
    pub velocity: Velocity,
    /// Optional articulation copied to every generated note.
    pub articulation: Option<Articulation>,
}

/// Request for deterministic phrase generation.
#[derive(Debug, Clone, PartialEq)]
pub struct GeneratePhraseRequest {
    /// Number of bars to generate.
    pub bar_count: usize,
    /// Time signature copied to each generated bar.
    pub time_signature: TimeSignature,
    /// Tempo copied to each generated bar.
    pub tempo: Tempo,
    /// PPQN resolution used to compute each bar duration.
    pub ticks_per_quarter: Ticks,
    /// Repeating note pattern.
    pub pattern: RepeatingPattern,
}

/// Computes the duration of one bar for a PPQN resolution and meter.
pub fn bar_duration_ticks(
    time_signature: TimeSignature,
    ticks_per_quarter: Ticks,
) -> Result<Ticks, ValidationError> {
    TimeSignature::new(time_signature.numerator, time_signature.denominator)?;

    if ticks_per_quarter == Ticks::ZERO {
        return Err(ValidationError::InvalidTicksPerQuarter);
    }

    let numerator_ticks = ticks_per_quarter
        .0
        .checked_mul(u32::from(time_signature.numerator))
        .ok_or(ValidationError::DurationOverflow)?;
    let whole_note_scaled = numerator_ticks
        .checked_mul(4)
        .ok_or(ValidationError::DurationOverflow)?;

    Ok(Ticks(
        whole_note_scaled
            .checked_div(u32::from(time_signature.denominator))
            .ok_or(ValidationError::InvalidTimeSignatureDenominator {
                value: time_signature.denominator,
            })?,
    ))
}

/// Generates a phrase by repeating a pitch pattern into every requested bar.
pub fn generate_repeating_phrase(
    request: &GeneratePhraseRequest,
) -> Result<Phrase, ValidationError> {
    validate_request(request)?;

    let bar_duration = bar_duration_ticks(request.time_signature, request.ticks_per_quarter)?;
    let mut bars = Vec::new();
    let mut pitch_index = 0_usize;

    for _ in 0..request.bar_count {
        let mut events = Vec::new();
        let mut cursor = Ticks::ZERO;

        while cursor < bar_duration {
            let remaining = bar_duration.checked_sub(cursor)?;
            let duration = request.pattern.step.min(remaining);
            let pitch = select_pitch(&request.pattern.pitches, pitch_index)?;

            events.push(Event::Note(Note {
                pitch,
                duration,
                velocity: request.pattern.velocity,
                articulation: request.pattern.articulation,
            }));
            cursor = cursor.checked_add(duration)?;
            pitch_index = increment_wrapping(pitch_index, request.pattern.pitches.len())?;
        }

        bars.push(Bar {
            time_signature: request.time_signature,
            tempo: request.tempo,
            events,
        });
    }

    Ok(Phrase { bars })
}

fn validate_request(request: &GeneratePhraseRequest) -> Result<(), ValidationError> {
    if request.pattern.pitches.is_empty() {
        return Err(ValidationError::EmptyPitchSequence);
    }

    if request.pattern.step == Ticks::ZERO {
        return Err(ValidationError::InvalidStepDuration);
    }

    let _bar_duration = bar_duration_ticks(request.time_signature, request.ticks_per_quarter)?;

    Ok(())
}

fn select_pitch(pitches: &[Pitch], index: usize) -> Result<Pitch, ValidationError> {
    pitches
        .get(index)
        .copied()
        .ok_or(ValidationError::EmptyPitchSequence)
}

fn increment_wrapping(value: usize, len: usize) -> Result<usize, ValidationError> {
    let next = value.checked_add(1).ok_or(ValidationError::CountOverflow)?;
    if next == len {
        Ok(0)
    } else {
        Ok(next)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        bar_duration_ticks, generate_repeating_phrase, GeneratePhraseRequest, RepeatingPattern,
    };
    use crate::event::{
        Articulation, Bar, Event, Note, Phrase, Pitch, Tempo, Ticks, TimeSignature,
        ValidationError, Velocity,
    };

    fn request() -> GeneratePhraseRequest {
        GeneratePhraseRequest {
            bar_count: 2,
            time_signature: TimeSignature {
                numerator: 4,
                denominator: 4,
            },
            tempo: Tempo(120.0),
            ticks_per_quarter: Ticks(480),
            pattern: RepeatingPattern {
                pitches: vec![Pitch(60), Pitch(64), Pitch(67)],
                step: Ticks(480),
                velocity: Velocity(96),
                articulation: Some(Articulation::PalmMute),
            },
        }
    }

    fn meter() -> TimeSignature {
        TimeSignature {
            numerator: 4,
            denominator: 4,
        }
    }

    fn note(pitch: u8, duration: u32) -> Event {
        Event::Note(Note {
            pitch: Pitch(pitch),
            duration: Ticks(duration),
            velocity: Velocity(96),
            articulation: Some(Articulation::PalmMute),
        })
    }

    fn generated_bar(events: Vec<Event>) -> Bar {
        Bar {
            time_signature: meter(),
            tempo: Tempo(120.0),
            events,
        }
    }

    #[test]
    fn bar_duration_ticks_uses_meter_and_ppqn() {
        assert_eq!(
            bar_duration_ticks(
                TimeSignature {
                    numerator: 4,
                    denominator: 4,
                },
                Ticks(480),
            ),
            Ok(Ticks(1920)),
        );
        assert_eq!(
            bar_duration_ticks(
                TimeSignature {
                    numerator: 7,
                    denominator: 8,
                },
                Ticks(480),
            ),
            Ok(Ticks(1680)),
        );
    }

    #[test]
    fn bar_duration_ticks_rejects_invalid_inputs() {
        assert_eq!(
            bar_duration_ticks(
                TimeSignature {
                    numerator: 4,
                    denominator: 4,
                },
                Ticks(0),
            ),
            Err(ValidationError::InvalidTicksPerQuarter),
        );
        assert_eq!(
            bar_duration_ticks(
                TimeSignature {
                    numerator: 4,
                    denominator: 3,
                },
                Ticks(480),
            ),
            Err(ValidationError::InvalidTimeSignatureDenominator { value: 3 }),
        );
    }

    #[test]
    fn generate_repeating_phrase_fills_requested_bars() {
        assert_eq!(
            generate_repeating_phrase(&request()),
            Ok(Phrase {
                bars: vec![
                    generated_bar(vec![
                        note(60, 480),
                        note(64, 480),
                        note(67, 480),
                        note(60, 480),
                    ]),
                    generated_bar(vec![
                        note(64, 480),
                        note(67, 480),
                        note(60, 480),
                        note(64, 480),
                    ]),
                ],
            }),
        );
    }

    #[test]
    fn generate_repeating_phrase_truncates_last_step_to_bar() {
        let mut source = request();
        source.bar_count = 1;
        source.pattern.step = Ticks(700);

        assert_eq!(
            generate_repeating_phrase(&source),
            Ok(Phrase {
                bars: vec![generated_bar(vec![
                    note(60, 700),
                    note(64, 700),
                    note(67, 520),
                ])],
            }),
        );
    }

    #[test]
    fn generate_repeating_phrase_rejects_empty_pitch_sequence() {
        let mut source = request();
        source.pattern.pitches = Vec::new();

        assert_eq!(
            generate_repeating_phrase(&source),
            Err(ValidationError::EmptyPitchSequence),
        );
    }

    #[test]
    fn generate_repeating_phrase_rejects_zero_step() {
        let mut source = request();
        source.pattern.step = Ticks::ZERO;

        assert_eq!(
            generate_repeating_phrase(&source),
            Err(ValidationError::InvalidStepDuration),
        );
    }
}
