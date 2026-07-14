//! The corpus→generation compiler: one implementation, shared by every caller.
//!
//! `griff generate`, the cockpit's Generate panel, and any experimental A/B
//! harness compile their generation input here — same tab-seeded request, same
//! placed-`(offset, duration)` rhythm extraction, same median gesture
//! aggregation, same resting-chunk filter — so a frontend can never drift from
//! the CLI's musical behaviour.
//!
//! Everything here is **pure**: it takes already-parsed [`ChunkMeta`] records
//! and already-imported [`Score`]s. Reading a corpus off a filesystem (the CLI)
//! or out of OPFS (the web cockpit) is the caller's job — each frontend owns its
//! own I/O and hands the parsed material to [`corpus_material`].

use crate::corpus::ChunkMeta;
use crate::event::{Pitch, Ticks};
use crate::generate;
use crate::gesture::GestureControl;
use crate::rerank;
use crate::score::{AtomEvent, Score, Track};
use crate::scoring::{Scored, WeightPolicy};
use crate::slice;

/// Why building a generation input failed.
#[derive(Debug)]
pub enum GenerationInputError {
    /// The source could not seed a request (silent source, zero bars, …).
    Generation(generate::GenerationError),
    /// The candidate-set builder rejected the derived request.
    Set(rerank::SetError),
    /// A corpus could not be read. The payload is the caller's I/O message —
    /// this module performs no I/O itself.
    Corpus(String),
}

impl From<generate::GenerationError> for GenerationInputError {
    fn from(e: generate::GenerationError) -> Self {
        Self::Generation(e)
    }
}

impl From<rerank::SetError> for GenerationInputError {
    fn from(e: rerank::SetError) -> Self {
        Self::Set(e)
    }
}

/// What a corpus supplies to a generation pass.
#[derive(Debug)]
pub struct CorpusMaterial {
    /// Per-bar rhythm templates from the chunks' sliced sources, deduped in
    /// first-seen order.
    pub rhythms: Vec<generate::RhythmTemplate>,
    /// The sliced chunk scores — the novelty guard's reference set.
    pub references: Vec<Score>,
    /// Aggregated burst/rest gesture ask, when any chunk carries stats.
    pub gesture: Option<GestureControl>,
    /// Record names skipped because their source was missing/unreadable or
    /// carried no notes; reported to the curator, never silently dropped.
    pub skipped: Vec<String>,
}

/// One corpus record ready to contribute: its metadata, its source sliced to the
/// record's `bar_range` provenance, and the sounding track the slice measures.
#[derive(Debug)]
pub struct LoadedChunk {
    /// The record.
    pub meta: ChunkMeta,
    /// The source, sliced to the record's `bar_range` when it carries one.
    pub sliced: Score,
    /// Index of the first track whose primary voice sounds.
    pub track: usize,
}

/// Notes in a track's *primary* (first) voice — the track-selection predicate
/// shared by curation, splitting, and corpus loading, so selection and
/// measurement agree on which track sounds.
#[must_use]
pub fn primary_voice_note_count(track: &Track) -> usize {
    track.voices.first().map_or(0, |v| {
        v.event_groups
            .iter()
            .flat_map(|g| &g.atoms)
            .filter(|a| matches!(a, AtomEvent::Note(_)))
            .count()
    })
}

/// Slices `source` to `meta`'s `bar_range` provenance and picks the sounding
/// track — the pure half of loading one corpus record.
///
/// `None` when the slice carries no sounding track (the caller reports the
/// record as skipped); the caller has already handled a missing or unparseable
/// source file.
#[must_use]
pub fn prepare_chunk(meta: ChunkMeta, source: &Score) -> Option<LoadedChunk> {
    let sliced = match meta.source.bar_range {
        Some((first, last)) => {
            let first = usize::try_from(first).ok()?;
            let last = usize::try_from(last).ok()?;
            slice::extract_bars(source, first..last.checked_add(1)?)
        }
        None => source.clone(),
    };
    let track = sliced
        .tracks
        .iter()
        .position(|t| primary_voice_note_count(t) > 0)?;
    Some(LoadedChunk {
        meta,
        sliced,
        track,
    })
}

/// Folds prepared chunks into the material a generation pass consumes: the
/// deduped rhythm-template palette, the novelty reference set, and the
/// aggregated gesture ask.
///
/// `loaded` order decides template order (first-seen dedup), so a caller that
/// wants a deterministic palette must hand records in a deterministic order.
/// `skipped` carries the names the caller could not load, unchanged.
#[must_use]
pub fn corpus_material(loaded: Vec<LoadedChunk>, skipped: Vec<String>) -> CorpusMaterial {
    let mut rhythms: Vec<generate::RhythmTemplate> = Vec::new();
    let mut references = Vec::new();
    let mut metas = Vec::new();

    for chunk in loaded {
        for template in bar_rhythms(&chunk.sliced, chunk.track) {
            if !rhythms.contains(&template) {
                rhythms.push(template);
            }
        }
        references.push(chunk.sliced);
        metas.push(chunk.meta);
    }

    CorpusMaterial {
        rhythms,
        references,
        gesture: gesture_control_from_chunks(&metas),
        skipped,
    }
}

/// What a caller asks a generation pass for.
#[derive(Debug, Clone, Copy)]
pub struct GenerationAsk {
    /// Deterministic seed — the same ask always yields the same set.
    pub seed: u64,
    /// Bars to generate.
    pub bars: usize,
    /// Seed variants *per strategy*; the set holds this many × 5 strategies
    /// (fewer only when rhythm-copy is skipped for want of a template).
    pub variants_per_strategy: usize,
    /// Carve the corpus's burst/rest gesture. Ignored without a corpus, and
    /// when no chunk carries gesture stats — a pass never invents a gesture.
    pub gesture: bool,
}

/// A ranked candidate set and the input it was generated from — enough to show
/// a candidate, explain its score, and reproduce it exactly.
#[derive(Debug)]
pub struct RankedSet {
    /// Candidates in rank order (aggregate descending), each carrying its axes,
    /// rationale, and variant-seed provenance.
    pub ranked: Vec<Scored<rerank::SetCandidate>>,
    /// The tab-seeded base request: pitch material, meter, tempo, range.
    pub base: generate::RuleGenerationRequest,
    /// The rhythm templates the pass actually rotated (explicit palette,
    /// corpus palette, or the source's first sounding bar as the fallback).
    /// For an explicit palette this is the caller's vector **verbatim**,
    /// silent templates included — provenance is never compressed.
    pub source_rhythms: Vec<generate::RhythmTemplate>,
    /// Whether `source_rhythms` is an explicit palette (ADR-0029 §7) — the
    /// separate scheduler that keeps silent bars in rotation — rather than
    /// the automatic corpus/source path.
    pub rhythm_explicit: bool,
    /// The gesture ask the pass carved against, when it carved.
    pub gesture: Option<GestureControl>,
    /// The rerank policy the aggregates were scored under.
    pub policy: WeightPolicy,
}

/// Generates and reranks the full candidate set for `score` under `ask`.
///
/// The one entry point every frontend generates through (`griff generate`, the
/// cockpit's Generate panel), so none of them can drift from the others' musical
/// behaviour.
///
/// Without `material` the rhythm template is the source's first sounding bar
/// and novelty has nothing to measure against (every candidate reads fully
/// novel). With it, rhythm templates, novelty references, and the gesture ask
/// come from the curated chunks — except that a corpus with no extractable
/// rhythm still generates, falling back to the source's first bar rather than
/// dropping rhythm-copy from the set.
///
/// # Errors
/// [`GenerationInputError::Generation`] when the source cannot seed a request,
/// [`GenerationInputError::Set`] when the candidate-set builder rejects it.
pub fn ranked_candidates(
    score: &Score,
    material: Option<&CorpusMaterial>,
    ask: &GenerationAsk,
    rhythm_override: Option<&[generate::RhythmTemplate]>,
) -> Result<RankedSet, GenerationInputError> {
    let base = generation_request_from_score(score, ask.seed, ask.bars)?;
    // Rhythm precedence (ADR-0029 §7): explicit pattern > corpus > source
    // first bar. Novelty references and gesture stay corpus-based either way.
    let explicit: Option<Vec<generate::RhythmTemplate>> = rhythm_override.map(<[_]>::to_vec);
    let (source_rhythms, gesture) = explicit.as_ref().map_or_else(
        || {
            material.map_or_else(
                || (base.source_rhythms.clone(), None),
                |m| {
                    let rhythms = if m.rhythms.is_empty() {
                        base.source_rhythms.clone()
                    } else {
                        m.rhythms.clone()
                    };
                    (rhythms, if ask.gesture { m.gesture } else { None })
                },
            )
        },
        |palette| {
            (
                palette.clone(),
                material.and_then(|m| if ask.gesture { m.gesture } else { None }),
            )
        },
    );
    let references: &[Score] = material.map_or(&[], |m| &m.references);

    let set = rerank::generate_candidate_set(&rerank::SetRequest {
        seed: base.seed,
        pitch_material: base.pitch_material.clone(),
        constraints: base.constraints,
        source_rhythms: source_rhythms.clone(),
        explicit_rhythms: explicit.clone(),
        variants_per_strategy: ask.variants_per_strategy,
        gesture,
    })?;
    let policy = rerank::rerank_weights_v1();
    let ranked = rerank::rerank_candidates(set, &base.pitch_material, references, &policy);

    Ok(RankedSet {
        ranked,
        base,
        source_rhythms,
        rhythm_explicit: explicit.is_some(),
        gesture,
        policy,
    })
}

/// Builds a tab-seeded [`generate::RuleGenerationRequest`]: the scale is the
/// source's distinct pitch classes, the rhythm template its first sounding bar,
/// and meter / tempo / range its transport.
///
/// # Errors
/// [`GenerationInputError::Generation`] when `bars` is zero, the source is
/// silent (no pitch material), or it carries no master bars.
pub fn generation_request_from_score(
    score: &Score,
    seed: u64,
    bars: usize,
) -> Result<generate::RuleGenerationRequest, GenerationInputError> {
    if bars == 0 {
        return Err(generate::GenerationError::BarCountZero.into());
    }
    let pitches = all_pitches(score);
    let (lo, hi) = pitch_range(&pitches)?;
    let first_bar = score
        .master_bars
        .first()
        .ok_or(generate::GenerationError::InvalidConstraints)?;
    let constraints = generate::GenerationConstraints {
        bar_count: bars,
        time_signature: first_bar.time_signature,
        tempo: first_bar.tempo,
        ticks_per_quarter: Ticks(u32::from(score.ticks_per_quarter)),
        pitch_lo: lo,
        pitch_hi: hi,
    };
    Ok(generate::RuleGenerationRequest {
        seed: generate::GenerationSeed(seed),
        pitch_material: pitch_material_from(lo, &pitches),
        constraints,
        explicit_rhythms: None,
        source_rhythms: vec![generate::RhythmTemplate::from_durations(&first_bar_rhythm(
            score,
        ))],
        strategy: generate::GenerationStrategy::RhythmCopyPitchSubstitute,
    })
}

/// Every note pitch across all tracks and voices, in track/voice order.
fn all_pitches(score: &Score) -> Vec<u8> {
    score
        .tracks
        .iter()
        .flat_map(|t| &t.voices)
        .flat_map(|v| &v.event_groups)
        .flat_map(|g| &g.atoms)
        .filter_map(|a| match a {
            AtomEvent::Note(n) => Some(n.pitch.0),
            AtomEvent::Rest(_) => None,
        })
        .collect()
}

/// The lowest and highest pitch present; errors (no pitch material) when the
/// source is silent.
fn pitch_range(pitches: &[u8]) -> Result<(Pitch, Pitch), GenerationInputError> {
    let lo = pitches
        .iter()
        .min()
        .copied()
        .ok_or(generate::GenerationError::EmptyPitchMaterial)?;
    let hi = pitches.iter().max().copied().unwrap_or(lo);
    Ok((Pitch(lo), Pitch(hi)))
}

/// A scale rooted at `lo` whose intervals are the distinct semitone classes the
/// source uses, so the generated riff stays in the tab's pitch palette.
fn pitch_material_from(lo: Pitch, pitches: &[u8]) -> generate::PitchMaterial {
    let mut intervals: Vec<u8> = pitches
        .iter()
        .map(|&p| p.saturating_sub(lo.0).checked_rem(12).unwrap_or(0))
        .collect();
    intervals.sort_unstable();
    intervals.dedup();
    if intervals.is_empty() {
        intervals.push(0);
    }
    generate::PitchMaterial {
        root: lo,
        intervals,
    }
}

/// The note durations of the first *sounding* bar — the earliest master bar
/// holding any note across all tracks and voices — in onset order, as the
/// rhythm template the generator copies. Falls back to four quarter notes only
/// when the source is entirely silent.
fn first_bar_rhythm(score: &Score) -> Vec<Ticks> {
    for bar in &score.master_bars {
        let mut notes: Vec<(u32, Ticks)> = score
            .tracks
            .iter()
            .flat_map(|t| &t.voices)
            .flat_map(|v| &v.event_groups)
            .flat_map(|g| &g.atoms)
            .filter_map(|a| match a {
                AtomEvent::Note(n)
                    if n.absolute_start.0 >= bar.tick_range.start.0
                        && n.absolute_start.0 < bar.tick_range.end.0 =>
                {
                    Some((n.absolute_start.0, n.duration))
                }
                _ => None,
            })
            .collect();
        if !notes.is_empty() {
            notes.sort_by_key(|&(onset, _)| onset);
            return notes.into_iter().map(|(_, dur)| dur).collect();
        }
    }
    let quarter = Ticks(u32::from(score.ticks_per_quarter));
    vec![quarter; 4]
}

/// One placement template per *sounding* bar of the track's primary voice.
///
/// Each note keeps its in-bar offset, so rests and syncopation survive into
/// the grid. Silent bars are phrase rests, not templates; identical templates
/// are deduped so a looped riff does not drown the set's template rotation in
/// copies of one rhythm.
#[must_use]
pub fn bar_rhythms(score: &Score, track: usize) -> Vec<generate::RhythmTemplate> {
    let Some(voice) = score.tracks.get(track).and_then(|t| t.voices.first()) else {
        return Vec::new();
    };
    let mut notes: Vec<(u32, Ticks)> = voice
        .event_groups
        .iter()
        .flat_map(|g| &g.atoms)
        .filter_map(|a| match a {
            AtomEvent::Note(n) => Some((n.absolute_start.0, n.duration)),
            AtomEvent::Rest(_) => None,
        })
        .collect();
    notes.sort_unstable_by_key(|&(onset, _)| onset);

    let mut templates = Vec::new();
    for bar in &score.master_bars {
        let placed: Vec<generate::TemplateNote> = notes
            .iter()
            .filter(|(onset, _)| *onset >= bar.tick_range.start.0 && *onset < bar.tick_range.end.0)
            .map(|&(onset, duration)| generate::TemplateNote {
                offset: Ticks(onset.saturating_sub(bar.tick_range.start.0)),
                duration,
            })
            .collect();
        if placed.is_empty() {
            continue;
        }
        let template = generate::RhythmTemplate { notes: placed };
        if !templates.contains(&template) {
            templates.push(template);
        }
    }
    templates
}

/// Aggregates the chunks' gesture statistics into one ask.
///
/// The per-axis *median* of the per-chunk [`GestureControl`]s (each already
/// clamped by [`GestureControl::from_stats`]), rounded back to whole burst
/// notes.
///
/// Only chunks that actually rest vote: a wall-to-wall riff's stats describe
/// one giant burst (mean burst = the whole chunk) and would inflate the ask
/// past ever carving (2026-07-11 playtest: burst 69 over a 32-note request
/// carved nothing). The median keeps one long-burst outlier from dragging
/// the ask out of carving range. `None` when no resting chunk carries stats —
/// the caller then generates wall-to-wall, it does not invent a gesture.
#[must_use]
pub fn gesture_control_from_chunks(chunks: &[ChunkMeta]) -> Option<GestureControl> {
    let controls: Vec<GestureControl> = chunks
        .iter()
        .filter_map(|c| c.gesture.as_ref())
        .filter(|s| s.rest_count > 0 && s.mean_rest_quarters > 0.0)
        .map(GestureControl::from_stats)
        .collect();
    if controls.is_empty() {
        return None;
    }
    #[allow(clippy::cast_precision_loss)] // burst lengths are tiny
    let burst = median(controls.iter().map(|c| c.burst_notes as f64).collect());
    let rest = median(controls.iter().map(|c| c.rest_quarters).collect());
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // rounded, clamped ≥ 1
    Some(GestureControl {
        burst_notes: burst.round().max(1.0) as usize,
        rest_quarters: rest.max(1.0),
    })
}

/// The median of `values` (mean of the two middles for an even count).
/// Deterministic: ties order by `total_cmp`. Caller guarantees non-empty.
fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    let mid = values.len() / 2;
    if values.len() % 2 == 1 {
        values.get(mid).copied().unwrap_or(0.0)
    } else {
        let hi = values.get(mid).copied().unwrap_or(0.0);
        let lo = values.get(mid.saturating_sub(1)).copied().unwrap_or(0.0);
        f64::midpoint(lo, hi)
    }
}
