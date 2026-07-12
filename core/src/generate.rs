//! Deterministic phrase generation primitives.

use crate::event::{
    NoteMarks, Pitch, Tempo, Ticks, TimeSignature, Tuning, ValidationError, Velocity,
};
use crate::score::{
    AtomEvent, AtomNote, EventGroup, EventGroupKind, LossReport, MasterBar, RepeatMarker, Score,
    Track, Voice,
};
use crate::slice::TickRange;

// ── S6: rule-based generator ──────────────────────────────────────────────────

/// A single generated note, the internal intermediate of the rule strategies.
///
/// Strategies emit `Vec<Vec<GenNote>>` (one inner `Vec` per bar); `bars_to_score`
/// lowers these onto the canonical model at `bar start + offset`, so in-bar
/// silence (rests, syncopation) survives generation. Meter and tempo are not
/// carried here — they live on the master bars (ADR-0003), derived from the
/// request constraints.
#[derive(Debug, Clone, Copy)]
struct GenNote {
    /// Onset offset from the bar start (from the rhythm grid).
    offset: Ticks,
    pitch: Pitch,
    duration: Ticks,
    velocity: Velocity,
}

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

// ── rhythm grid ───────────────────────────────────────────────────────────────

/// One placed note of a bar-length rhythm template.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TemplateNote {
    /// Onset offset from the bar start.
    pub offset: Ticks,
    /// Note duration.
    pub duration: Ticks,
}

/// A one-bar rhythm pattern of onset-*placed* notes.
///
/// Gaps — rests and syncopation — survive extraction and generation (offsets
/// need not be contiguous). Every strategy lays its pitches onto this grid;
/// the pitch logic stays the strategy's own.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RhythmTemplate {
    /// The placed notes, expected in ascending offset order.
    pub notes: Vec<TemplateNote>,
}

impl RhythmTemplate {
    /// Rebuilds the legacy wall-to-wall shape: back-to-back durations become
    /// notes whose offsets accumulate from the bar start.
    #[must_use]
    pub fn from_durations(durations: &[Ticks]) -> Self {
        let mut notes = Vec::with_capacity(durations.len());
        let mut offset = Ticks::ZERO;
        for &duration in durations {
            notes.push(TemplateNote { offset, duration });
            offset = Ticks(offset.0.saturating_add(duration.0));
        }
        Self { notes }
    }
}

/// The *effective* grids: one clamped grid per template that survives
/// empty-removal and clamping to the bar, in input order. May be empty (the
/// caller falls back to the quarter grid). This is the set the per-bar
/// scheduler rotates, and the set [`rhythm_diagnostics`] reports.
fn effective_grids(templates: &[RhythmTemplate], bar_duration: Ticks) -> Vec<Vec<TemplateNote>> {
    templates
        .iter()
        .filter(|t| !t.notes.is_empty())
        .map(|t| clamp_template(t, bar_duration))
        .filter(|g| !g.is_empty())
        .collect()
}

/// The per-bar placement grids: the effective grids, or a single quarter-note
/// grid when none is usable — the no-corpus case keeps today's wall-to-wall
/// quarter behaviour. Never empty.
fn bar_grids(
    templates: &[RhythmTemplate],
    bar_duration: Ticks,
    ticks_per_quarter: Ticks,
) -> Vec<Vec<TemplateNote>> {
    let grids = effective_grids(templates, bar_duration);
    if grids.is_empty() {
        vec![quarter_grid(bar_duration, ticks_per_quarter)]
    } else {
        grids
    }
}

/// A deterministic diagnostic of how rhythm templates resolve for a given bar
/// duration — for CLI generation-summary transparency (making a corpus A/B
/// interpretable), not a runtime generation input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RhythmDiagnostics {
    /// Templates passed in, before empty-removal.
    pub loaded: usize,
    /// Effective grids: templates surviving empty-removal and clamping to the
    /// bar (the grids the per-bar scheduler rotates). Zero means the quarter
    /// fallback was used.
    pub effective: usize,
    /// A stable fingerprint per effective grid, in scheduler order — the
    /// FNV-1a hash of its `(offset, duration)` pairs. Equal rhythms share a
    /// fingerprint; distinct rhythms differ.
    pub fingerprints: Vec<u64>,
}

/// Reports how `templates` resolve at `bar_duration` (see [`RhythmDiagnostics`]).
#[must_use]
pub fn rhythm_diagnostics(templates: &[RhythmTemplate], bar_duration: Ticks) -> RhythmDiagnostics {
    let grids = effective_grids(templates, bar_duration);
    let fingerprints = grids.iter().map(|g| grid_fingerprint(g)).collect();
    RhythmDiagnostics {
        loaded: templates.len(),
        effective: grids.len(),
        fingerprints,
    }
}

/// FNV-1a (64-bit) hash of a grid's `(offset, duration)` pairs — a stable,
/// order-sensitive fingerprint of one bar rhythm.
fn grid_fingerprint(grid: &[TemplateNote]) -> u64 {
    const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = OFFSET_BASIS;
    for note in grid {
        for byte in note
            .offset
            .0
            .to_le_bytes()
            .into_iter()
            .chain(note.duration.0.to_le_bytes())
        {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(PRIME);
        }
    }
    hash
}

/// The grid for bar `bar_index`, cycling through `grids` (guaranteed
/// non-empty by [`bar_grids`]).
fn grid_for_bar(grids: &[Vec<TemplateNote>], bar_index: usize) -> &[TemplateNote] {
    let idx = bar_index.checked_rem(grids.len()).unwrap_or(0);
    grids.get(idx).map_or(&[], Vec::as_slice)
}

/// Clamps a template to one bar: notes at or past the bar end drop, durations
/// clamp to the bar end, zero durations drop, and the result sorts by offset
/// so onsets stay non-decreasing.
fn clamp_template(template: &RhythmTemplate, bar_duration: Ticks) -> Vec<TemplateNote> {
    let mut notes: Vec<TemplateNote> = template
        .notes
        .iter()
        .filter(|n| n.offset < bar_duration)
        .map(|n| TemplateNote {
            offset: n.offset,
            duration: fit_duration(n.duration, Ticks(bar_duration.0.saturating_sub(n.offset.0))),
        })
        .filter(|n| n.duration > Ticks::ZERO)
        .collect();
    notes.sort_by_key(|n| n.offset.0);
    notes
}

/// The fallback grid: quarter notes from the bar start, the last clamped to
/// the bar end (e.g. 7/8 ends on an eighth).
fn quarter_grid(bar_duration: Ticks, ticks_per_quarter: Ticks) -> Vec<TemplateNote> {
    let step = ticks_per_quarter.max(Ticks(1));
    let mut notes = Vec::new();
    let mut offset = Ticks::ZERO;
    while offset < bar_duration {
        let remaining = Ticks(bar_duration.0.saturating_sub(offset.0));
        notes.push(TemplateNote {
            offset,
            duration: fit_duration(step, remaining),
        });
        offset = Ticks(offset.0.saturating_add(step.0));
    }
    notes
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
///
/// Every strategy lays its pitches onto the rhythm grids built from
/// `source_rhythms` (quarter notes when none are usable); the four
/// per-bar-independent strategies rotate the grids by bar index, while
/// [`RepeatVariation`](GenerationStrategy::RepeatVariation) deliberately holds
/// the first grid (repetition is its identity). The strategies differ in
/// *pitch* behaviour only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GenerationStrategy {
    /// Ascending scale degrees over the corpus rhythm (requires a template).
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

/// A generated part with provenance metadata.
///
/// The output is the canonical model: a [`Score`] owning its own
/// [`MasterBar`]s plus a single [`Track`] with one [`Voice`]. A complementary
/// part can later be added as another `Track` on the same master timeline.
#[derive(Debug, Clone)]
pub struct GenerationCandidate {
    /// The generated score (one track, one voice).
    pub score: Score,
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
    /// Rhythm templates extracted from the corpus — the whole palette. Each
    /// non-empty template becomes one effective bar grid (empty templates and
    /// templates that clamp away are dropped); the per-bar-independent
    /// strategies rotate the effective grids by bar index, so a single
    /// generation carries the corpus's rhythmic variety across its bars.
    /// Empty or all-unusable → the quarter-note fallback grid.
    pub source_rhythms: Vec<RhythmTemplate>,
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
        && !request.source_rhythms.iter().any(|t| !t.notes.is_empty())
    {
        return Err(GenerationError::RhythmTemplateMissing);
    }

    // `Score::ticks_per_quarter` is a `u16`; a PPQN that cannot be represented
    // there would make the score's resolution disagree with the onsets we
    // compute at the real PPQN. Reject rather than clamp.
    if u16::try_from(request.constraints.ticks_per_quarter.0).is_err() {
        return Err(GenerationError::InvalidConstraints);
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
    let grids = bar_grids(&request.source_rhythms, bar_duration, c.ticks_per_quarter);

    let bars = match request.strategy {
        GenerationStrategy::RhythmCopyPitchSubstitute => {
            strategy_rhythm_copy(c, pm, &grids, &mut prng)
        }
        GenerationStrategy::MotifTransposeVariation => {
            strategy_motif_transpose(c, pm, &grids, &mut prng)
        }
        GenerationStrategy::ConstrainedRandomWalk => {
            strategy_constrained_walk(c, pm, &grids, &mut prng)
        }
        GenerationStrategy::ShuffleMotifs => strategy_shuffle_motifs(c, pm, &grids, &mut prng),
        GenerationStrategy::RepeatVariation => strategy_repeat_variation(c, pm, &grids, &mut prng),
    };

    let score = bars_to_score(&bars, c, bar_duration)?;
    Ok(GenerationCandidate {
        score,
        strategy: request.strategy,
        seed: request.seed,
    })
}

/// Lays the generated bars (each a `Vec<GenNote>`) onto the canonical model.
///
/// Builds `MasterBar`s back-to-back from `bar_duration` and places each bar's
/// notes at `bar start + note offset` in one `Voice` of `Single` event groups —
/// grid gaps become real silence. Tempo and meter live on the master bars
/// (ADR-0003), not on notes.
///
/// Returns [`GenerationError::InvalidConstraints`] when the absolute timeline
/// cannot be represented in the `u32` tick space (rather than silently
/// truncating later bars). The caller guarantees PPQN fits a `u16`.
fn bars_to_score(
    bars: &[Vec<GenNote>],
    c: &GenerationConstraints,
    bar_duration: Ticks,
) -> Result<Score, GenerationError> {
    let mut master_bars = Vec::with_capacity(bars.len());
    let mut event_groups = Vec::new();
    let mut bar_start = Ticks::ZERO;

    for (index, bar) in bars.iter().enumerate() {
        let bar_end = bar_start
            .checked_add(bar_duration)
            .map_err(|_| GenerationError::InvalidConstraints)?;
        let tick_range =
            TickRange::new(bar_start, bar_end).map_err(|_| GenerationError::InvalidConstraints)?;
        master_bars.push(MasterBar {
            index,
            tick_range,
            time_signature: c.time_signature,
            tempo: c.tempo,
            repeat: RepeatMarker::default(),
        });

        for note in bar {
            let absolute_start = bar_start
                .checked_add(note.offset)
                .map_err(|_| GenerationError::InvalidConstraints)?;
            event_groups.push(EventGroup {
                kind: EventGroupKind::Single,
                atoms: vec![AtomEvent::Note(AtomNote {
                    absolute_start,
                    duration: note.duration,
                    pitch: note.pitch,
                    velocity: note.velocity,
                    marks: NoteMarks::empty(),
                    position: None,
                })],
                technique_spans: Vec::new(),
            });
        }

        bar_start = bar_end;
    }

    let ticks_per_quarter =
        u16::try_from(c.ticks_per_quarter.0).map_err(|_| GenerationError::InvalidConstraints)?;

    Ok(Score {
        ticks_per_quarter,
        master_bars,
        tracks: vec![Track {
            name: None,
            channel: 0,
            voices: vec![Voice {
                id: 0,
                event_groups,
            }],
            tuning: Tuning::standard_e(),
        }],
        source_meta: None,
        loss: LossReport::new(),
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

/// Clamps a tick duration so it does not exceed `remaining`.
const fn fit_duration(raw: Ticks, remaining: Ticks) -> Ticks {
    if raw.0 > remaining.0 {
        remaining
    } else {
        raw
    }
}

// ── strategies ────────────────────────────────────────────────────────────────
//
// Every strategy writes one pitch per grid slot; the grid carries the rhythm
// (offsets + durations), the strategy carries the pitch logic. The four
// per-bar-independent strategies select their bar's grid via `grid_for_bar`
// (rotation by bar index); `strategy_repeat_variation` holds `grids[0]`.

fn strategy_rhythm_copy(
    c: &GenerationConstraints,
    pm: &PitchMaterial,
    grids: &[Vec<TemplateNote>],
    prng: &mut Xorshift64,
) -> Vec<Vec<GenNote>> {
    let scale_len = pm.intervals.len();
    let mut degree = prng.next_mod(scale_len);
    let mut bars = Vec::with_capacity(c.bar_count);

    for bar_index in 0..c.bar_count {
        let grid = grid_for_bar(grids, bar_index);
        let mut notes = Vec::with_capacity(grid.len());
        for slot in grid {
            notes.push(GenNote {
                offset: slot.offset,
                pitch: pm.pitch_at(degree, c.pitch_lo, c.pitch_hi),
                duration: slot.duration,
                velocity: Velocity(90),
            });
            degree = advance_degree(degree, scale_len);
        }
        bars.push(notes);
    }
    bars
}

fn strategy_motif_transpose(
    c: &GenerationConstraints,
    pm: &PitchMaterial,
    grids: &[Vec<TemplateNote>],
    prng: &mut Xorshift64,
) -> Vec<Vec<GenNote>> {
    const MOTIF_LEN: usize = 4;
    // Transpositions in semitones applied cyclically per bar.
    const TRANSPOSES: [i8; 7] = [0, 3, 5, 7, -3, -5, -7];

    let scale_len = pm.intervals.len();
    let start_degree = prng.next_mod(scale_len);

    let mut bars = Vec::with_capacity(c.bar_count);

    for bi in 0..c.bar_count {
        let grid = grid_for_bar(grids, bi);
        let transpose = TRANSPOSES
            .get(bi.checked_rem(TRANSPOSES.len()).unwrap_or(0))
            .copied()
            .unwrap_or(0_i8);
        let mut notes = Vec::with_capacity(grid.len());

        for (slot_idx, slot) in grid.iter().enumerate() {
            let motif_pos = slot_idx.checked_rem(MOTIF_LEN).unwrap_or(0);
            let degree = (start_degree.wrapping_add(motif_pos))
                .checked_rem(scale_len)
                .unwrap_or(0);
            let base = pm.pitch_at(degree, c.pitch_lo, c.pitch_hi);
            notes.push(GenNote {
                offset: slot.offset,
                pitch: apply_transpose(base, transpose, c.pitch_lo, c.pitch_hi),
                duration: slot.duration,
                velocity: Velocity(85),
            });
        }

        bars.push(notes);
    }
    bars
}

fn strategy_constrained_walk(
    c: &GenerationConstraints,
    pm: &PitchMaterial,
    grids: &[Vec<TemplateNote>],
    prng: &mut Xorshift64,
) -> Vec<Vec<GenNote>> {
    let scale_len = pm.intervals.len();
    // Two-octave degree range keeps consecutive leaps ≤ 12 semitones.
    let max_degree = scale_len.saturating_mul(2).saturating_sub(1);
    let mut degree = prng.next_mod(scale_len);

    let mut bars = Vec::with_capacity(c.bar_count);

    for bar_index in 0..c.bar_count {
        let grid = grid_for_bar(grids, bar_index);
        let mut notes = Vec::with_capacity(grid.len());
        for slot in grid {
            notes.push(GenNote {
                offset: slot.offset,
                pitch: pm.pitch_at(degree, c.pitch_lo, c.pitch_hi),
                duration: slot.duration,
                velocity: Velocity(80),
            });

            // Walk ±1 degree, bounded to [0, max_degree].
            if prng.next_u64() & 1 == 0 {
                degree = degree.saturating_sub(1);
            } else {
                degree = degree.saturating_add(1).min(max_degree);
            }
        }
        bars.push(notes);
    }
    bars
}

fn strategy_shuffle_motifs(
    c: &GenerationConstraints,
    pm: &PitchMaterial,
    grids: &[Vec<TemplateNote>],
    prng: &mut Xorshift64,
) -> Vec<Vec<GenNote>> {
    let scale_len = pm.intervals.len();

    let mut bars = Vec::with_capacity(c.bar_count);

    for bar_index in 0..c.bar_count {
        let grid = grid_for_bar(grids, bar_index);
        let mut notes = Vec::with_capacity(grid.len());
        for slot in grid {
            let degree = prng.next_mod(scale_len);
            notes.push(GenNote {
                offset: slot.offset,
                pitch: pm.pitch_at(degree, c.pitch_lo, c.pitch_hi),
                duration: slot.duration,
                velocity: Velocity(88),
            });
        }
        bars.push(notes);
    }
    bars
}

fn strategy_repeat_variation(
    c: &GenerationConstraints,
    pm: &PitchMaterial,
    grids: &[Vec<TemplateNote>],
    prng: &mut Xorshift64,
) -> Vec<Vec<GenNote>> {
    let scale_len = pm.intervals.len();
    let base_degree = prng.next_mod(scale_len);

    // Repetition is this strategy's identity (call/response), so it stays on
    // the first bar's rhythm rather than rotating templates.
    let grid = grid_for_bar(grids, 0);
    let base_bar = build_ascending_bar(c, pm, base_degree, grid);
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
        // Replace the last note with the variation pitch.
        if let Some(n) = varied.last_mut() {
            n.pitch = var_pitch;
        }
        bars.push(varied);
    }
    bars
}

/// Builds one bar of ascending scale-degree notes over the grid.
fn build_ascending_bar(
    c: &GenerationConstraints,
    pm: &PitchMaterial,
    start_degree: usize,
    grid: &[TemplateNote],
) -> Vec<GenNote> {
    let scale_len = pm.intervals.len();
    // Two-octave ceiling mirrors the walk strategy.
    let max_degree = scale_len.saturating_mul(2).saturating_sub(1);
    let mut notes = Vec::with_capacity(grid.len());
    let mut degree = start_degree;

    for slot in grid {
        notes.push(GenNote {
            offset: slot.offset,
            pitch: pm.pitch_at(degree, c.pitch_lo, c.pitch_hi),
            duration: slot.duration,
            velocity: Velocity(92),
        });
        degree = degree.saturating_add(1).min(max_degree);
    }

    notes
}

/// Transposes a pitch by `semitones`, clamping the result to `[lo, hi]`.
fn apply_transpose(pitch: Pitch, semitones: i8, lo: Pitch, hi: Pitch) -> Pitch {
    let raw = i16::from(pitch.0).saturating_add(i16::from(semitones));
    let actual_lo = i16::from(lo.0.min(hi.0));
    let actual_hi = i16::from(lo.0.max(hi.0).min(127));
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    Pitch(raw.clamp(actual_lo, actual_hi) as u8)
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

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::{bar_duration_ticks, bar_grids, RhythmTemplate, TemplateNote};
    use crate::event::{Ticks, TimeSignature, ValidationError};

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
    fn quarter_fallback_grid_clamps_the_last_slot() {
        // 7/8 at 480 PPQN: 1680-tick bar → three quarters and one eighth.
        // No template → a single quarter-grid fallback.
        let grids = bar_grids(&[], Ticks(1680), Ticks(480));
        assert_eq!(grids.len(), 1, "no template yields one fallback grid");
        let grid = grids.first().expect("one fallback grid");
        let expected: Vec<TemplateNote> = vec![
            TemplateNote {
                offset: Ticks(0),
                duration: Ticks(480),
            },
            TemplateNote {
                offset: Ticks(480),
                duration: Ticks(480),
            },
            TemplateNote {
                offset: Ticks(960),
                duration: Ticks(480),
            },
            TemplateNote {
                offset: Ticks(1440),
                duration: Ticks(240),
            },
        ];
        assert_eq!(grid, &expected);
    }

    #[test]
    fn unsorted_template_grid_sorts_by_offset() {
        let template = RhythmTemplate {
            notes: vec![
                TemplateNote {
                    offset: Ticks(960),
                    duration: Ticks(240),
                },
                TemplateNote {
                    offset: Ticks(0),
                    duration: Ticks(240),
                },
            ],
        };
        let grids = bar_grids(&[template], Ticks(1920), Ticks(480));
        let offsets: Vec<u32> = grids
            .first()
            .expect("one grid")
            .iter()
            .map(|n| n.offset.0)
            .collect();
        assert_eq!(offsets, vec![0, 960], "onsets must come back sorted");
    }
}
