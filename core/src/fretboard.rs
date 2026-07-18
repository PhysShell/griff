//! Fretboard position inference — the guitar fingering problem (ADR-0019).
//!
//! A small, local, deterministic DP assigns each pitch a playable
//! `(string, fret)` under a [`Tuning`], minimising hand movement and string
//! changes with an open-string bias. This is the monophonic first pass
//! (ADR-0019 P1): a pure function over a pitch sequence, with no `Score`
//! mutation and no evidence wiring yet.
//!
//! It is a *small local* DP over one voice's per-note candidates — distinct from
//! the S7 graph traversal (ADR-0013/0015) — linear in notes × strings, so no
//! beam search is needed. Out-of-range pitches yield `None` and reset the path.
//!
//! [`measure_playability`] summarises the DP's optimal path as a
//! [`PlayabilityReport`] — the per-part playability filter the S13 pair
//! validator and the S6 stage doc call for, and the seed of the S7
//! `playability` / `fret_jump_penalty` cost terms.

use std::collections::HashMap;

use crate::event::{ConfidenceBps, FretboardPosition, NotePosition, Pitch, Tuning};
use crate::score::{AtomEvent, AtomNote, Voice};

/// Conventional highest fret considered when enumerating candidates.
pub const STANDARD_MAX_FRET: u8 = 24;

/// Confidence assigned to a MIDI-inferred position. A documented placeholder —
/// a margin-based confidence (best vs runner-up path) is a future refinement
/// (ADR-0019).
const INFERRED_CONFIDENCE: ConfidenceBps = ConfidenceBps::HALF;

/// Named, versioned cost weights for the fingering DP.
///
/// Weights are *data* (ADR-0017 §3), so they can be tuned later — e.g. learned
/// from real tablatures ("path-difference learning", ADR-0019).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FingeringWeights {
    /// Cost per fret of a chosen position (mild preference for lower frets).
    pub fret: i64,
    /// Bonus subtracted for an open string (`fret == 0`).
    pub open_string: i64,
    /// Cost per fret of hand movement between consecutive notes.
    pub position_shift: i64,
    /// Cost of changing strings between consecutive notes.
    pub string_change: i64,
}

impl FingeringWeights {
    /// The baseline `v1` weights — untuned (ADR-0019).
    #[must_use]
    pub const fn v1() -> Self {
        Self {
            fret: 1,
            open_string: 1,
            position_shift: 2,
            string_change: 1,
        }
    }
}

/// Per-candidate cost: prefer lower frets, bonus for open strings.
fn candidate_cost(p: FretboardPosition, w: &FingeringWeights) -> i64 {
    let base = w.fret.saturating_mul(i64::from(p.fret));
    if p.fret == 0 {
        base.saturating_sub(w.open_string)
    } else {
        base
    }
}

/// Transition cost between two consecutive positions: hand movement + string change.
fn transition_cost(a: FretboardPosition, b: FretboardPosition, w: &FingeringWeights) -> i64 {
    let shift = w
        .position_shift
        .saturating_mul(i64::from(a.fret.abs_diff(b.fret)));
    let string = if a.string == b.string {
        0
    } else {
        w.string_change
    };
    shift.saturating_add(string)
}

/// One DP node: a candidate position, the min cumulative cost of reaching it, and
/// the index of its parent candidate in the previous layer.
#[derive(Debug, Clone, Copy)]
struct Node {
    pos: FretboardPosition,
    cost: i64,
    parent: usize,
}

/// Infers a plausible fretboard position for each pitch under `tuning`
/// (ADR-0019).
///
/// Returns one entry per input pitch, in order; `None` marks an out-of-range
/// pitch (no playable string), which also resets the hand-position context for
/// the run that follows. Deterministic: ties break toward the lowest string,
/// then the lowest fret (the order [`Tuning::candidates`] yields).
#[must_use]
// DP layer/parent indices are in-bounds by construction; costs use saturating
// arithmetic. The indexing is local to this self-contained routine.
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
pub fn infer_positions(
    pitches: &[Pitch],
    tuning: &Tuning,
    weights: &FingeringWeights,
    max_fret: u8,
) -> Vec<Option<FretboardPosition>> {
    let mut result: Vec<Option<FretboardPosition>> = vec![None; pitches.len()];
    let mut i = 0usize;

    while i < pitches.len() {
        let first = tuning.candidates(pitches[i], max_fret);
        if first.is_empty() {
            // Out of range: report None, reset, move on.
            i += 1;
            continue;
        }

        // Start a segment of consecutive playable notes; build DP layers.
        let start = i;
        let mut layers: Vec<Vec<Node>> = Vec::new();
        layers.push(
            first
                .into_iter()
                .map(|pos| Node {
                    pos,
                    cost: candidate_cost(pos, weights),
                    parent: usize::MAX,
                })
                .collect(),
        );
        i += 1;

        while i < pitches.len() {
            let cands = tuning.candidates(pitches[i], max_fret);
            if cands.is_empty() {
                break;
            }
            let prev = &layers[layers.len() - 1];
            let layer: Vec<Node> = cands
                .into_iter()
                .map(|pos| {
                    // Best parent: min cumulative cost + transition; ties → lowest index.
                    let mut best_parent = 0usize;
                    let mut best_cost = i64::MAX;
                    for (j, node) in prev.iter().enumerate() {
                        let c = node
                            .cost
                            .saturating_add(transition_cost(node.pos, pos, weights));
                        if c < best_cost {
                            best_cost = c;
                            best_parent = j;
                        }
                    }
                    Node {
                        pos,
                        cost: best_cost.saturating_add(candidate_cost(pos, weights)),
                        parent: best_parent,
                    }
                })
                .collect();
            layers.push(layer);
            i += 1;
        }

        // Backtrack: pick the min-cost node in the last layer (ties → lowest index).
        let last = &layers[layers.len() - 1];
        let mut bi = 0usize;
        let mut best = i64::MAX;
        for (j, node) in last.iter().enumerate() {
            if node.cost < best {
                best = node.cost;
                bi = j;
            }
        }
        for layer_idx in (0..layers.len()).rev() {
            let node = layers[layer_idx][bi];
            result[start + layer_idx] = Some(node.pos);
            bi = node.parent;
        }
    }

    result
}

/// Playability facts of one melodic line, measured on the optimal fingering
/// path (the S13 per-part filter; the S7 cost terms' seed).
///
/// The verdict is *reachability* — [`PlayabilityReport::is_playable`] holds
/// when every line note has a playable `(string, fret)` under the tuning.
/// Fret travel is reported as a fact, not a verdict: jump thresholds are
/// calibration data (corpus / S9), not code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayabilityReport {
    /// Line notes measured.
    pub note_count: usize,
    /// Line notes with no playable `(string, fret)` under the tuning.
    pub unpositionable: usize,
    /// Largest fret travel between consecutively positioned line notes
    /// (`0` when fewer than two notes are positioned in a row). An
    /// unpositionable note resets the path, so travel is never measured
    /// across the gap — mirroring [`infer_positions`].
    pub max_fret_jump: u8,
}

impl PlayabilityReport {
    /// `true` when every measured line note is reachable on the fretboard;
    /// an empty line is vacuously playable.
    #[must_use]
    pub const fn is_playable(&self) -> bool {
        self.unpositionable == 0
    }
}

/// Measures the playability of a melodic line under `tuning`: runs the
/// fingering DP ([`infer_positions`]) and summarises its optimal path as a
/// [`PlayabilityReport`]. Pure and deterministic (SPEC §6).
#[must_use]
pub fn measure_playability(
    pitches: &[Pitch],
    tuning: &Tuning,
    weights: &FingeringWeights,
    max_fret: u8,
) -> PlayabilityReport {
    let positions = infer_positions(pitches, tuning, weights, max_fret);
    let unpositionable = positions.iter().filter(|p| p.is_none()).count();
    let max_fret_jump = positions
        .windows(2)
        .filter_map(|w| match (w.first().copied(), w.get(1).copied()) {
            (Some(Some(a)), Some(Some(b))) => Some(a.fret.abs_diff(b.fret)),
            _ => None,
        })
        .max()
        .unwrap_or(0);

    PlayabilityReport {
        note_count: pitches.len(),
        unpositionable,
        max_fret_jump,
    }
}

/// Writes inferred `(string, fret)` positions onto a voice's notes (ADR-0019 P1b).
///
/// Runs the fingering DP over the voice's *monophonic* note pitches in order and
/// marks each result `InferredFromMidi`. Returns the number of out-of-range notes
/// (no playable position) so the importer can report the loss (ADR-0019 §1).
///
/// Notes that share an onset (a chord) are left unknown rather than misvoiced
/// onto one string — chord voicing is a deferred follow-up (ADR-0019 §7) — and
/// are *not* counted as losses. Intended for MIDI-sourced material; Guitar Pro
/// keeps its own `Explicit` positions.
#[must_use]
pub fn assign_inferred_positions(
    voice: &mut Voice,
    tuning: &Tuning,
    weights: &FingeringWeights,
    max_fret: u8,
) -> usize {
    // Count notes per onset: a note is monophonic iff its onset is unique.
    let mut per_onset: HashMap<u32, usize> = HashMap::new();
    for group in &voice.event_groups {
        for atom in &group.atoms {
            if let AtomEvent::Note(n) = atom {
                let c = per_onset.entry(n.absolute_start.0).or_insert(0);
                *c = c.saturating_add(1);
            }
        }
    }
    let is_mono = |n: &AtomNote| per_onset.get(&n.absolute_start.0).copied().unwrap_or(0) == 1;

    let pitches: Vec<Pitch> = voice
        .event_groups
        .iter()
        .flat_map(|g| g.atoms.iter())
        .filter_map(|a| match a {
            AtomEvent::Note(n) if is_mono(n) => Some(n.pitch),
            _ => None,
        })
        .collect();

    let inferred = infer_positions(&pitches, tuning, weights, max_fret);

    let mut idx = 0usize;
    let mut out_of_range = 0usize;
    for group in &mut voice.event_groups {
        for atom in &mut group.atoms {
            if let AtomEvent::Note(note) = atom {
                if is_mono(note) {
                    match inferred.get(idx).copied() {
                        Some(Some(pos)) => {
                            note.position = Some(NotePosition::inferred(pos, INFERRED_CONFIDENCE));
                        }
                        // Monophonic but no playable string: a genuine loss.
                        Some(None) => out_of_range = out_of_range.saturating_add(1),
                        None => {}
                    }
                    idx = idx.saturating_add(1);
                }
                // Chord notes are left as-is (unknown), not a loss.
            }
        }
    }
    out_of_range
}
