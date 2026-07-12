//! Reusable generation-input seam (arbiter 2026-07-12).
//!
//! One implementation of the corpus→generation compiler, shared by the
//! `griff generate` command and any experimental A/B harness (an
//! `examples/` binary), so a scan can never drift from production — same
//! placed-`(offset, duration)` rhythm extraction, same median gesture
//! aggregation, same resting-chunk filter.
//!
//! **Experimental, `#[doc(hidden)]`**: this is a stability-exempt seam for
//! tooling, not a public library surface. File I/O stays here (not in
//! `griff-core`); the pure musical transforms live in `griff-core`.

use std::fs;
use std::path::Path;

use griff_core::corpus::ChunkMeta;
use griff_core::event::{Pitch, Ticks};
use griff_core::generate;
use griff_core::gesture::GestureControl;
use griff_core::import;
use griff_core::score::{AtomEvent, Score};
use griff_core::slice;

use crate::primary_voice_note_count;

/// Why building a generation input failed.
#[derive(Debug)]
pub enum GenerationInputError {
    /// The source could not seed a request (silent source, zero bars, …).
    Generation(generate::GenerationError),
    /// A corpus directory could not be read.
    Corpus(String),
}

impl From<generate::GenerationError> for GenerationInputError {
    fn from(e: generate::GenerationError) -> Self {
        Self::Generation(e)
    }
}

/// What a corpus directory supplies to a generation pass.
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

/// Loads every `*.chunk.json` record in `dir` with its source tab.
///
/// Records are sorted by name (deterministic result); each source tab is
/// expected next to it under `source.filename`, sliced to the record's
/// `bar_range` provenance when one is present. Group records
/// (`*.group.json`) are curation metadata, not chunks, and are ignored.
///
/// # Errors
/// [`GenerationInputError::Corpus`] when `dir` cannot be read.
pub fn load_corpus_material(dir: &Path) -> Result<CorpusMaterial, GenerationInputError> {
    let entries = fs::read_dir(dir).map_err(|e| {
        GenerationInputError::Corpus(format!("cannot read corpus dir {}: {e}", dir.display()))
    })?;
    let mut record_names: Vec<String> = entries
        .filter_map(Result::ok)
        .filter_map(|e| e.file_name().to_str().map(ToOwned::to_owned))
        .filter(|n| n.ends_with(".chunk.json"))
        .collect();
    record_names.sort_unstable();

    let mut rhythms: Vec<generate::RhythmTemplate> = Vec::new();
    let mut references = Vec::new();
    let mut loaded_chunks = Vec::new();
    let mut skipped = Vec::new();

    for name in record_names {
        let Some(chunk) = load_chunk_source(dir, &name) else {
            skipped.push(name);
            continue;
        };
        let (meta, sliced, track) = chunk;
        for template in bar_rhythms(&sliced, track) {
            if !rhythms.contains(&template) {
                rhythms.push(template);
            }
        }
        references.push(sliced);
        loaded_chunks.push(meta);
    }

    Ok(CorpusMaterial {
        rhythms,
        references,
        gesture: gesture_control_from_chunks(&loaded_chunks),
        skipped,
    })
}

/// Reads one chunk record and its source tab, slicing the record's
/// `bar_range`. `None` when the record does not parse, the source is
/// missing/unimportable, or the slice carries no notes — the caller reports
/// the record as skipped.
fn load_chunk_source(dir: &Path, record_name: &str) -> Option<(ChunkMeta, Score, usize)> {
    let meta: ChunkMeta =
        serde_json::from_str(&fs::read_to_string(dir.join(record_name)).ok()?).ok()?;
    let source =
        import::import_score_auto(&fs::read(dir.join(&meta.source.filename)).ok()?).ok()?;
    let sliced = match meta.source.bar_range {
        Some((first, last)) => {
            let first = usize::try_from(first).ok()?;
            let last = usize::try_from(last).ok()?;
            slice::extract_bars(&source, first..last.checked_add(1)?)
        }
        None => source,
    };
    let track = sliced
        .tracks
        .iter()
        .position(|t| primary_voice_note_count(t) > 0)?;
    Some((meta, sliced, track))
}

/// One placement template per *sounding* bar of the track's primary voice.
///
/// Each note keeps its in-bar offset, so rests and syncopation survive into
/// the grid. Silent bars are phrase rests, not templates; identical templates
/// are deduped so a looped riff does not drown the set's template rotation in
/// copies of one rhythm.
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
        (lo + hi) / 2.0
    }
}
