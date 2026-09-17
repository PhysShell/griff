//! Fingering optimality experiments — the first optimization-phase subject.
//!
//! Three pieces, all pure and deterministic:
//!
//! - **Tablature lines** ([`tab_lines`]): monophonic runs of a Guitar Pro
//!   track that carry the tab author's own `(string, fret)` choices — the
//!   human reference a fingering model is measured against.
//! - **The production objective, mirrored** ([`v1_cost`], [`v1_problem`]):
//!   the exact cost `griff_core::fretboard::infer_positions` minimizes,
//!   re-implemented independently so an external solver's optimum can be
//!   compared with the production DP's path.
//! - **A hand-position model** ([`HandModel`], [`solve_hand`],
//!   [`hand_problem`]): a hidden index-finger position with a four-fret box,
//!   stretch, shift events and distances, and string distance — the
//!   finger-span layer ADR-0019 §7 defers. Experimental: calibration
//!   evidence only, no authority over production.

use std::ops::RangeInclusive;

use griff_core::event::{FretboardPosition, NoteMark, Pitch, SpanTechnique, Tuning};
use griff_core::fretboard::{FingeringWeights, STANDARD_MAX_FRET};
use griff_core::score::{AtomEvent, AtomNote, EventGroup, Score};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::ir::{fnv1a64, IntVar, VarId};
use crate::optir::{Hard, OptIrError, OptProblem, Term};
use crate::problems::LabError;

/// Variables per note in a [`v1_problem`]: `s{i}` (string), `f{i}` (fret).
pub const V1_VARS_PER_NOTE: usize = 2;
/// Variables per note in a [`hand_problem`]: `s{i}`, `f{i}`, `h{i}` (hand).
pub const HAND_VARS_PER_NOTE: usize = 3;

/// How a track is cut into tablature lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct LineCut {
    /// Lines shorter than this are dropped (and counted).
    pub min_notes: usize,
    /// A silence of at least this many quarters between a note's end and the
    /// next onset cuts the line; `0` disables rest cuts.
    pub max_rest_quarters: u32,
    /// Positions above this fret cut the line (and are counted).
    pub max_fret: u8,
}

impl LineCut {
    /// The experiment's baseline cut: ≥ 4 notes, a whole-bar-in-4/4 rest
    /// cuts, [`STANDARD_MAX_FRET`].
    #[must_use]
    pub const fn v1() -> Self {
        Self {
            min_notes: 4,
            max_rest_quarters: 4,
            max_fret: STANDARD_MAX_FRET,
        }
    }
}

/// Where the notes of a track went when it was cut into lines.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct CutStats {
    /// Note atoms read.
    pub notes_seen: u64,
    /// Onsets carrying more than one note (each cuts the line).
    pub chord_onsets: u64,
    /// Single notes without a position (each cuts the line).
    pub unpositioned: u64,
    /// Single notes positioned above `max_fret` (each cuts the line).
    pub beyond_max_fret: u64,
    /// Single notes whose position does not sound their pitch under the
    /// track tuning (each cuts the line).
    pub pitch_mismatch: u64,
    /// Rests long enough to cut a non-empty line.
    pub rest_cuts: u64,
    /// Non-empty lines dropped as shorter than `min_notes`.
    pub short_lines: u64,
    /// Notes inside those dropped lines.
    pub short_line_notes: u64,
    /// Lines kept.
    pub kept_lines: u64,
    /// Notes inside kept lines.
    pub kept_notes: u64,
    /// Tracks whose tuning was strictly ascending (string 1 = lowest, the GP6
    /// import orientation) and was mirrored to string 1 = highest.
    pub mirrored_tracks: u64,
    /// Legato origins (a hammer-on, pull-off or legato span) on the last note
    /// of a kept line: the note they lead to is not in the line, so they
    /// project onto no [`TechniqueEdge`].
    pub dangling_legato: u64,
}

impl CutStats {
    /// Adds another track's counts into this one.
    pub fn absorb(&mut self, other: &Self) {
        let Self {
            notes_seen,
            chord_onsets,
            unpositioned,
            beyond_max_fret,
            pitch_mismatch,
            rest_cuts,
            short_lines,
            short_line_notes,
            kept_lines,
            kept_notes,
            mirrored_tracks,
            dangling_legato,
        } = *other;
        self.notes_seen = self.notes_seen.saturating_add(notes_seen);
        self.chord_onsets = self.chord_onsets.saturating_add(chord_onsets);
        self.unpositioned = self.unpositioned.saturating_add(unpositioned);
        self.beyond_max_fret = self.beyond_max_fret.saturating_add(beyond_max_fret);
        self.pitch_mismatch = self.pitch_mismatch.saturating_add(pitch_mismatch);
        self.rest_cuts = self.rest_cuts.saturating_add(rest_cuts);
        self.short_lines = self.short_lines.saturating_add(short_lines);
        self.short_line_notes = self.short_line_notes.saturating_add(short_line_notes);
        self.kept_lines = self.kept_lines.saturating_add(kept_lines);
        self.kept_notes = self.kept_notes.saturating_add(kept_notes);
        self.mirrored_tracks = self.mirrored_tracks.saturating_add(mirrored_tracks);
        self.dangling_legato = self.dangling_legato.saturating_add(dangling_legato);
    }
}

/// What the tab joins a note to its predecessor with: a technique belongs to
/// the edge from note `i − 1` to note `i`, not to either note.
///
/// These are the imported span kinds. Guitar Pro stores one legato flag on the
/// note a hammer-on or pull-off starts from, without its direction, and the
/// import emits every such flag as [`SpanTechnique::HammerOn`]: a `HammerOn`
/// edge is an observed legato origin, not a known hammer-on. Direction can
/// only be derived from pitch ([`crate::technique::derived_direction`]).
///
/// [`SpanTechnique::HammerOn`]: griff_core::event::SpanTechnique::HammerOn
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum TechniqueEdge {
    /// No legato span leads into the note (always the case for note 0).
    #[default]
    Plain,
    /// The previous note carries a `HammerOn` span.
    HammerOn,
    /// The previous note carries a `PullOff` span.
    PullOff,
    /// The previous note carries a `Legato` span.
    Legato,
}

impl TechniqueEdge {
    /// Whether a legato span of any kind joins the two notes.
    #[must_use]
    pub const fn is_legato(self) -> bool {
        !matches!(self, Self::Plain)
    }

    /// The legato edge a note in `group` starts: its group's first hammer-on,
    /// pull-off or legato span, or [`TechniqueEdge::Plain`].
    fn out_of(group: &EventGroup) -> Self {
        group
            .technique_spans
            .iter()
            .find_map(|span| match span.technique {
                SpanTechnique::HammerOn => Some(Self::HammerOn),
                SpanTechnique::PullOff => Some(Self::PullOff),
                SpanTechnique::Legato => Some(Self::Legato),
                _ => None,
            })
            .unwrap_or_default()
    }
}

/// One monophonic tablature line with the tab author's positions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabLine {
    /// Track index in the score.
    pub track: usize,
    /// Voice id within the track.
    pub voice: u8,
    /// Onset tick of the first note.
    pub start_tick: u32,
    /// The track tuning.
    pub tuning: Tuning,
    /// Pitches, in onset order.
    pub pitches: Vec<Pitch>,
    /// The tab author's positions — one per pitch, each sounding it.
    pub human: Vec<FretboardPosition>,
    /// Where the fretting hand was just before the line: the fret of the
    /// latest positioned, fretted note of this voice with an earlier onset
    /// (the lowest such fret when that onset is a chord). `None` when nothing
    /// fretted precedes the line. Taken from the tab — context for tab
    /// completion, not something MIDI-sourced material carries.
    pub anchor_fret: Option<u8>,
    /// Per note, whether the tab marks it tapped (`NoteMark::Tap`) — played by
    /// the picking hand on the fretboard, not fretted by the fretting hand.
    pub tapped: Vec<bool>,
    /// Per note, the technique edge from the previous note (`edges[0]` is
    /// always [`TechniqueEdge::Plain`]).
    pub edges: Vec<TechniqueEdge>,
}

/// Cuts one track into monophonic tablature lines, per voice.
///
/// A line is a maximal run of single-note onsets whose explicit positions
/// sound their pitch under the track tuning. A chord onset, an unpositioned
/// note, a position above `cut.max_fret`, a pitch/position mismatch, or a
/// long enough rest ends the current line; each cause is counted.
///
/// Lines always use griff's string orientation (string 1 = highest): a track
/// whose tuning is strictly ascending is mirrored — tuning reversed and every
/// position renumbered — and counted in [`CutStats::mirrored_tracks`].
///
/// # Errors
///
/// [`LabError::NoSuchTrack`] when `track_index` is out of range.
pub fn tab_lines(
    score: &Score,
    track_index: usize,
    cut: &LineCut,
) -> Result<(Vec<TabLine>, CutStats), LabError> {
    let track = score
        .tracks
        .get(track_index)
        .ok_or(LabError::NoSuchTrack { index: track_index })?;
    let rest_ticks = u64::from(cut.max_rest_quarters) * u64::from(score.ticks_per_quarter);
    let mut lines = Vec::new();
    let mut stats = CutStats::default();

    // Positions are checked against the imported tuning as-is, then emitted
    // in griff's orientation (string 1 = highest).
    let open = track.tuning.open_strings();
    let mirrored = open.len() >= 2 && open.windows(2).all(|w| matches!(w, [a, b] if a.0 < b.0));
    let tuning = if mirrored {
        stats.mirrored_tracks = 1;
        Tuning::new(open.iter().rev().copied().collect())
    } else {
        track.tuning.clone()
    };
    let string_count = u8::try_from(open.len()).unwrap_or(u8::MAX);
    let orient = |p: FretboardPosition| {
        if mirrored {
            FretboardPosition {
                string: string_count.saturating_add(1).saturating_sub(p.string),
                fret: p.fret,
            }
        } else {
            p
        }
    };

    for voice in &track.voices {
        // Each note with the legato edge it starts (its group's span).
        let mut notes: Vec<(&AtomNote, TechniqueEdge)> = voice
            .event_groups
            .iter()
            .flat_map(|g| {
                let out = TechniqueEdge::out_of(g);
                g.atoms.iter().filter_map(move |a| match a {
                    AtomEvent::Note(n) => Some((n, out)),
                    AtomEvent::Rest(_) => None,
                })
            })
            .collect();
        notes.sort_by_key(|(n, _)| n.absolute_start.0);

        let mut line = LineBuilder::new(track_index, voice.id, &tuning);
        let mut sounding_until: Option<u64> = None;
        // Lowest fretted position at the latest onset seen so far.
        let mut last_fretted: Option<u8> = None;
        let mut rest = notes.as_slice();
        while let Some((first, _)) = rest.first() {
            let onset = first.absolute_start.0;
            let width = rest
                .iter()
                .position(|(n, _)| n.absolute_start.0 != onset)
                .unwrap_or(rest.len());
            let (group, tail) = rest.split_at(width);
            let anchor_here = last_fretted;
            if let Some(fret) = group
                .iter()
                .filter_map(|(n, _)| n.position)
                .map(|p| p.position.fret)
                .filter(|&fret| fret > 0)
                .min()
            {
                last_fretted = Some(fret);
            }
            rest = tail;
            stats.notes_seen = stats.notes_seen.saturating_add(count(group.len()));

            let onset_ticks = u64::from(onset);
            let rest_cut = cut.max_rest_quarters > 0
                && sounding_until.is_some_and(|end| onset_ticks >= end.saturating_add(rest_ticks));
            let group_end = group
                .iter()
                .map(|(n, _)| onset_ticks.saturating_add(u64::from(n.duration.0)))
                .max()
                .unwrap_or(onset_ticks);
            sounding_until = Some(sounding_until.map_or(group_end, |end| end.max(group_end)));
            if rest_cut && !line.is_empty() {
                stats.rest_cuts = stats.rest_cuts.saturating_add(1);
                line.flush(cut, &mut lines, &mut stats);
            }

            let [(note, legato_out)] = group else {
                stats.chord_onsets = stats.chord_onsets.saturating_add(1);
                line.flush(cut, &mut lines, &mut stats);
                continue;
            };
            let Some(position) = note.position.map(|p| p.position) else {
                stats.unpositioned = stats.unpositioned.saturating_add(1);
                line.flush(cut, &mut lines, &mut stats);
                continue;
            };
            if position.fret > cut.max_fret {
                stats.beyond_max_fret = stats.beyond_max_fret.saturating_add(1);
                line.flush(cut, &mut lines, &mut stats);
                continue;
            }
            if track.tuning.pitch_at(position) != Some(note.pitch) {
                stats.pitch_mismatch = stats.pitch_mismatch.saturating_add(1);
                line.flush(cut, &mut lines, &mut stats);
                continue;
            }
            line.push(
                onset,
                note.pitch,
                orient(position),
                NoteContext {
                    anchor: anchor_here,
                    tapped: note.marks.contains(NoteMark::Tap),
                    legato_out: *legato_out,
                },
            );
        }
        line.flush(cut, &mut lines, &mut stats);
    }
    Ok((lines, stats))
}

/// The production fingering objective (ADR-0019 `v1`), re-implemented
/// independently of `infer_positions`: per note `fret·w.fret − [open]·w.open_string`,
/// per step `|Δfret|·w.position_shift + [string changed]·w.string_change`.
#[must_use]
pub fn v1_cost(line: &[FretboardPosition], weights: &FingeringWeights) -> i64 {
    let unary = line
        .iter()
        .map(|&p| v1_unary(p.fret, weights))
        .fold(0_i64, i64::saturating_add);
    let steps = line
        .windows(2)
        .map(|pair| match pair {
            [a, b] => weights
                .position_shift
                .saturating_mul(i64::from(a.fret.abs_diff(b.fret)))
                .saturating_add(if a.string == b.string {
                    0
                } else {
                    weights.string_change
                }),
            _ => 0,
        })
        .fold(0_i64, i64::saturating_add);
    unary.saturating_add(steps)
}

/// The production objective as an [`OptProblem`]: per note `s{i}` and
/// `f{i}` tied by the candidate table, unary fret costs, `AbsDiff` fret
/// travel and `NotEqual` string change between neighbours. Zero-weight terms
/// and zero-cost table entries are omitted.
///
/// # Errors
///
/// [`LabError::EmptyLine`] for no pitches; [`LabError::UnpositionablePitch`]
/// when a pitch has no candidate at or below `max_fret`.
pub fn v1_problem(
    pitches: &[Pitch],
    tuning: &Tuning,
    weights: &FingeringWeights,
    max_fret: u8,
) -> Result<OptProblem, LabError> {
    if pitches.is_empty() {
        return Err(LabError::EmptyLine);
    }
    let mut vars = Vec::with_capacity(pitches.len().saturating_mul(V1_VARS_PER_NOTE));
    let mut hard = Vec::with_capacity(pitches.len());
    let mut objective = Vec::new();
    for (index, &pitch) in pitches.iter().enumerate() {
        let candidates = candidates_or_refuse(index, pitch, tuning, max_fret)?;
        let (s, f) = push_position_vars(&mut vars, &mut hard, index, &candidates);
        let costs: Vec<(i64, i64)> = distinct_frets(&candidates)
            .into_iter()
            .map(|fret| (i64::from(fret), v1_unary(fret, weights)))
            .filter(|&(_, cost)| cost != 0)
            .collect();
        if !costs.is_empty() {
            objective.push(Term::Unary { var: f, costs });
        }
        if index > 0 {
            let (prev_s, prev_f) = (VarId(s.0 - V1_VARS_PER_NOTE), VarId(f.0 - V1_VARS_PER_NOTE));
            if weights.position_shift != 0 {
                objective.push(Term::AbsDiff {
                    a: prev_f,
                    b: f,
                    weight: weights.position_shift,
                });
            }
            if weights.string_change != 0 {
                objective.push(Term::NotEqual {
                    a: prev_s,
                    b: s,
                    weight: weights.string_change,
                });
            }
        }
    }
    Ok(build("fingering-v1", vars, hard, objective))
}

/// Encodes positions as a [`v1_problem`] witness (`s0, f0, s1, f1, …`).
#[must_use]
pub fn encode_v1_witness(line: &[FretboardPosition]) -> Vec<i64> {
    line.iter()
        .flat_map(|p| [i64::from(p.string), i64::from(p.fret)])
        .collect()
}

/// Encodes positions and hands as a [`hand_problem`] witness
/// (`s0, f0, h0, s1, …`); `None` when the lengths differ.
#[must_use]
pub fn encode_hand_witness(line: &[FretboardPosition], hands: &[u8]) -> Option<Vec<i64>> {
    if line.len() != hands.len() {
        return None;
    }
    Some(
        line.iter()
            .zip(hands)
            .flat_map(|(p, &h)| [i64::from(p.string), i64::from(p.fret), i64::from(h)])
            .collect(),
    )
}

/// Decodes the per-note positions of a witness laid out with
/// `vars_per_note` variables per note, string then fret first; `None` for a
/// ragged length or out-of-range values.
#[must_use]
pub fn decode_positions(witness: &[i64], vars_per_note: usize) -> Option<Vec<FretboardPosition>> {
    if vars_per_note < 2 || !witness.len().is_multiple_of(vars_per_note) {
        return None;
    }
    witness
        .chunks(vars_per_note)
        .map(|chunk| match chunk {
            [string, fret, ..] => Some(FretboardPosition {
                string: u8::try_from(*string).ok()?,
                fret: u8::try_from(*fret).ok()?,
            }),
            _ => None,
        })
        .collect()
}

/// Weights of the hand-position model. Transition weights and `stretch` are
/// non-negative; `height` and `open_string` may be negative (a preference
/// for high positions, a bonus for open strings).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandWeights {
    /// Per fret of hand height above the first position, per note.
    pub height: i64,
    /// Per open-string note.
    pub open_string: i64,
    /// Per note played one fret outside the four-fret box.
    pub stretch: i64,
    /// Per hand shift (the position changes at all).
    pub shift: i64,
    /// Per fret of hand travel.
    pub shift_distance: i64,
    /// Per string crossed between consecutive notes.
    pub string_distance: i64,
}

/// How a fret is reached from a hand position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reach {
    /// An open string — reachable from any hand position.
    Open,
    /// Inside the four-fret box `[hand, hand + 3]`.
    InBox,
    /// One fret outside the box: `hand − 1` (≥ 1) or `hand + 4`.
    Stretch,
}

/// Typed refusals for a hand model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum HandModelError {
    /// A weight that must be non-negative is negative.
    #[error("weight {name} must be non-negative, got {value}")]
    NegativeWeight {
        /// The weight's field name.
        name: &'static str,
        /// Its value.
        value: i64,
    },
    /// The neck is too short for a four-fret box.
    #[error("max_fret {max_fret} leaves no room for a four-fret box")]
    NoRoom {
        /// The refused fret range.
        max_fret: u8,
    },
}

/// A validated hand-position model over frets `0..=max_fret`; hand
/// positions are `1..=max_fret − 3`, so the box stays on the neck.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HandModel {
    weights: HandWeights,
    max_fret: u8,
}

impl HandModel {
    /// Frets covered by the hand without a stretch.
    pub const BOX_FRETS: u8 = 4;

    /// Validates the weights and the neck range.
    ///
    /// # Errors
    ///
    /// [`HandModelError::NegativeWeight`] for a negative `stretch`, `shift`,
    /// `shift_distance`, or `string_distance`; [`HandModelError::NoRoom`]
    /// when `max_fret < 4`.
    pub fn new(weights: HandWeights, max_fret: u8) -> Result<Self, HandModelError> {
        for (name, value) in [
            ("stretch", weights.stretch),
            ("shift", weights.shift),
            ("shift_distance", weights.shift_distance),
            ("string_distance", weights.string_distance),
        ] {
            if value < 0 {
                return Err(HandModelError::NegativeWeight { name, value });
            }
        }
        if max_fret < Self::BOX_FRETS {
            return Err(HandModelError::NoRoom { max_fret });
        }
        Ok(Self { weights, max_fret })
    }

    /// The weights.
    #[must_use]
    pub const fn weights(&self) -> HandWeights {
        self.weights
    }

    /// The highest fret.
    #[must_use]
    pub const fn max_fret(&self) -> u8 {
        self.max_fret
    }

    /// Admissible hand positions, ascending.
    #[must_use]
    pub fn hands(&self) -> RangeInclusive<u8> {
        1..=self.max_fret.saturating_sub(Self::BOX_FRETS - 1)
    }

    /// How `fret` is reached from `hand`; `None` when it is not reachable
    /// (or `hand` is not an admissible position).
    #[must_use]
    pub fn reach(&self, fret: u8, hand: u8) -> Option<Reach> {
        if !self.hands().contains(&hand) || fret > self.max_fret {
            return None;
        }
        if fret == 0 {
            return Some(Reach::Open);
        }
        let top = hand.saturating_add(Self::BOX_FRETS - 1);
        if (hand..=top).contains(&fret) {
            Some(Reach::InBox)
        } else if fret == top.saturating_add(1) || fret.saturating_add(1) == hand {
            Some(Reach::Stretch)
        } else {
            None
        }
    }
}

/// Why a `(positions, hands)` pair cannot be scored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum HandError {
    /// Positions and hands differ in length.
    #[error("{positions} positions but {hands} hands")]
    Length {
        /// Positions supplied.
        positions: usize,
        /// Hands supplied.
        hands: usize,
    },
    /// A note is not reachable from its hand position.
    #[error("note {index} is not reachable from its hand position")]
    Unreachable {
        /// Index of the note.
        index: usize,
    },
}

/// Scores a complete `(positions, hands)` realization under the model:
/// per note `height·(hand − 1) + [open]·open_string + [stretch]·stretch`,
/// per step `[hand changed]·shift + |Δhand|·shift_distance + |Δstring|·string_distance`.
///
/// # Errors
///
/// See [`HandError`].
pub fn hand_cost(
    line: &[FretboardPosition],
    hands: &[u8],
    model: &HandModel,
) -> Result<i64, HandError> {
    if line.len() != hands.len() {
        return Err(HandError::Length {
            positions: line.len(),
            hands: hands.len(),
        });
    }
    let mut total = 0_i64;
    for (index, (p, &hand)) in line.iter().zip(hands).enumerate() {
        let unary = hand_unary(model, p.fret, hand).ok_or(HandError::Unreachable { index })?;
        total = total.saturating_add(unary);
    }
    for (pair, hand_pair) in line.windows(2).zip(hands.windows(2)) {
        if let ([a, b], [ha, hb]) = (pair, hand_pair) {
            total = total.saturating_add(hand_transition(&model.weights, *a, *ha, *b, *hb));
        }
    }
    Ok(total)
}

/// The cheapest hand sequence for **fixed** positions (e.g. a human tab):
/// the model's score of that fingering. `None` when some position is
/// unreachable from every hand position.
#[must_use]
pub fn best_hands(line: &[FretboardPosition], model: &HandModel) -> Option<(i64, Vec<u8>)> {
    let hands: Vec<u8> = model.hands().collect();
    let mut layers: Vec<Vec<Scored<usize>>> = Vec::with_capacity(line.len());
    for (index, p) in line.iter().enumerate() {
        let layer: Vec<Scored<usize>> = hands
            .iter()
            .map(|&h| {
                let unary = hand_unary(model, p.fret, h)?;
                let Some(prev_layer) = index.checked_sub(1).and_then(|i| layers.get(i)) else {
                    return Some((unary, usize::MAX));
                };
                let prev = line.get(index - 1).copied()?;
                let mut best: Scored<usize> = None;
                for (j, cell) in prev_layer.iter().enumerate() {
                    let (Some((cost, _)), Some(&ph)) = (cell, hands.get(j)) else {
                        continue;
                    };
                    let total = cost
                        .saturating_add(hand_transition(&model.weights, prev, ph, *p, h))
                        .saturating_add(unary);
                    if best.is_none_or(|(b, _)| total < b) {
                        best = Some((total, j));
                    }
                }
                best
            })
            .collect();
        if layer.iter().all(Option::is_none) {
            return None;
        }
        layers.push(layer);
    }
    let Some(last) = layers.last() else {
        return Some((0, Vec::new()));
    };
    let mut best: Scored<usize> = None;
    for (j, cell) in last.iter().enumerate() {
        if let Some((cost, _)) = cell {
            if best.is_none_or(|(b, _)| *cost < b) {
                best = Some((*cost, j));
            }
        }
    }
    let (cost, mut j) = best?;
    let mut out = vec![0_u8; line.len()];
    for (layer, slot) in layers.iter().zip(out.iter_mut()).rev() {
        let (_, parent) = (*layer.get(j)?)?;
        *slot = *hands.get(j)?;
        j = parent;
    }
    Some((cost, out))
}

/// An optimal realization under a [`HandModel`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandSolution {
    /// The optimal cost.
    pub cost: i64,
    /// One position per pitch.
    pub positions: Vec<FretboardPosition>,
    /// One hand position per pitch.
    pub hands: Vec<u8>,
}

/// Exact joint DP over `(candidate, hand)` states — the in-repo reference
/// optimum of the hand model. Deterministic. `None` when some pitch has no
/// candidate at or below the model's `max_fret`; an empty line costs `0`.
#[must_use]
pub fn solve_hand(pitches: &[Pitch], tuning: &Tuning, model: &HandModel) -> Option<HandSolution> {
    let hands: Vec<u8> = model.hands().collect();
    let weights = model.weights;
    let mut layers: Vec<HandLayer> = Vec::with_capacity(pitches.len());
    for &pitch in pitches {
        let candidates = tuning.candidates(pitch, model.max_fret);
        if candidates.is_empty() {
            return None;
        }
        let unary: Vec<Vec<Option<i64>>> = candidates
            .iter()
            .map(|c| {
                hands
                    .iter()
                    .map(|&h| hand_unary(model, c.fret, h))
                    .collect()
            })
            .collect();
        let cells = match layers.last() {
            None => unary
                .iter()
                .map(|row| row.iter().map(|u| u.map(|u| (u, (0, 0)))).collect())
                .collect(),
            Some(prev) => hand_step(prev, &candidates, &unary, &hands, &weights),
        };
        let layer = HandLayer { candidates, cells };
        if layer.cells.iter().flatten().all(Option::is_none) {
            return None;
        }
        layers.push(layer);
    }

    let Some(last) = layers.last() else {
        return Some(HandSolution {
            cost: 0,
            positions: Vec::new(),
            hands: Vec::new(),
        });
    };
    let mut best: Scored<(usize, usize)> = None;
    for (ci, row) in last.cells.iter().enumerate() {
        for (hi, cell) in row.iter().enumerate() {
            if let Some((cost, _)) = cell {
                if best.is_none_or(|(b, _)| *cost < b) {
                    best = Some((*cost, (ci, hi)));
                }
            }
        }
    }
    let (cost, (mut ci, mut hi)) = best?;
    let mut positions = vec![FretboardPosition { string: 0, fret: 0 }; layers.len()];
    let mut chosen = vec![0_u8; layers.len()];
    for ((layer, position), hand) in layers
        .iter()
        .zip(positions.iter_mut())
        .zip(chosen.iter_mut())
        .rev()
    {
        let (_, parent) = (*layer.cells.get(ci)?.get(hi)?)?;
        *position = *layer.candidates.get(ci)?;
        *hand = *hands.get(hi)?;
        (ci, hi) = parent;
    }
    Some(HandSolution {
        cost,
        positions,
        hands: chosen,
    })
}

/// The hand model as an [`OptProblem`]: per note `s{i}`, `f{i}`, `h{i}`;
/// the candidate table ties string to fret, a reach table ties fret to hand;
/// unary height and open-string costs, a fret×hand stretch table, and
/// `NotEqual` / `AbsDiff` hand shifts plus `AbsDiff` string distance between
/// neighbours. Zero-weight terms and zero-cost entries are omitted.
///
/// # Errors
///
/// [`LabError::EmptyLine`] for no pitches; [`LabError::UnpositionablePitch`]
/// when a pitch has no candidate at or below the model's `max_fret`.
pub fn hand_problem(
    pitches: &[Pitch],
    tuning: &Tuning,
    model: &HandModel,
) -> Result<OptProblem, LabError> {
    if pitches.is_empty() {
        return Err(LabError::EmptyLine);
    }
    let hands: Vec<u8> = model.hands().collect();
    let weights = model.weights;
    let mut vars = Vec::with_capacity(pitches.len().saturating_mul(HAND_VARS_PER_NOTE));
    let mut hard = Vec::with_capacity(pitches.len().saturating_mul(2));
    let mut objective = Vec::new();
    for (index, &pitch) in pitches.iter().enumerate() {
        let candidates = candidates_or_refuse(index, pitch, tuning, model.max_fret)?;
        let (s, f) = push_position_vars(&mut vars, &mut hard, index, &candidates);
        let h = VarId(vars.len());
        vars.push(IntVar::new(
            format!("h{index}"),
            hands.iter().map(|&x| i64::from(x)).collect(),
        ));
        let frets = distinct_frets(&candidates);
        let mut reach_tuples = Vec::new();
        let mut stretch_costs = Vec::new();
        for &fret in &frets {
            for &hand in &hands {
                match model.reach(fret, hand) {
                    None => {}
                    Some(reach) => {
                        reach_tuples.push((i64::from(fret), i64::from(hand)));
                        if reach == Reach::Stretch && weights.stretch != 0 {
                            stretch_costs.push((i64::from(fret), i64::from(hand), weights.stretch));
                        }
                    }
                }
            }
        }
        hard.push(Hard::Allowed {
            a: f,
            b: h,
            tuples: reach_tuples,
        });
        if weights.height != 0 {
            objective.push(Term::Unary {
                var: h,
                costs: hands
                    .iter()
                    .map(|&x| {
                        (
                            i64::from(x),
                            weights.height.saturating_mul(i64::from(x) - 1),
                        )
                    })
                    .filter(|&(_, cost)| cost != 0)
                    .collect(),
            });
        }
        if weights.open_string != 0 && frets.contains(&0) {
            objective.push(Term::Unary {
                var: f,
                costs: vec![(0, weights.open_string)],
            });
        }
        if !stretch_costs.is_empty() {
            objective.push(Term::Pair {
                a: f,
                b: h,
                costs: stretch_costs,
            });
        }
        if index > 0 {
            let (prev_s, prev_h) = (
                VarId(s.0 - HAND_VARS_PER_NOTE),
                VarId(h.0 - HAND_VARS_PER_NOTE),
            );
            if weights.shift != 0 {
                objective.push(Term::NotEqual {
                    a: prev_h,
                    b: h,
                    weight: weights.shift,
                });
            }
            if weights.shift_distance != 0 {
                objective.push(Term::AbsDiff {
                    a: prev_h,
                    b: h,
                    weight: weights.shift_distance,
                });
            }
            if weights.string_distance != 0 {
                objective.push(Term::AbsDiff {
                    a: prev_s,
                    b: s,
                    weight: weights.string_distance,
                });
            }
        }
    }
    Ok(build("fingering-hand", vars, hard, objective))
}

/// A song identity for holdout splits: the file stem, lowercased, with
/// trailing parenthesized groups (e.g. `(ver 2 by …)`) and the extension
/// removed, so arrangements of one song share a key.
#[must_use]
pub fn song_key(file_name: &str) -> String {
    let name = file_name.rsplit(['/', '\\']).next().unwrap_or(file_name);
    let stem = match name.rfind('.') {
        Some(dot) if dot > 0 && !name[dot..].contains(' ') => &name[..dot],
        _ => name,
    };
    let mut key = stem.trim();
    while key.ends_with(')') {
        match key.rfind('(') {
            Some(open) => key = key[..open].trim_end(),
            None => break,
        }
    }
    key.to_lowercase()
}

/// A deterministic holdout bucket in `0..buckets` for a [`song_key`]
/// (FNV-1a 64 modulo `buckets`); `0` when `buckets` is `0`.
#[must_use]
pub fn holdout_bucket(key: &str, buckets: u64) -> u64 {
    if buckets == 0 {
        return 0;
    }
    fnv1a64(key.as_bytes()) % buckets
}

/// Repeated figures inside one line: start indices `(i, j)` with
/// `i + window <= j` of identical pitch windows, scanning left to right —
/// for each start `i` the first later non-overlapping occurrence `j`, after
/// which the scan resumes at `i + window`. Single-pitch windows (ostinato on
/// one note) are skipped: they carry no fingering shape. Empty for
/// `window == 0`.
#[must_use]
pub fn repeat_pairs(pitches: &[Pitch], window: usize) -> Vec<(usize, usize)> {
    let n = pitches.len();
    let mut pairs = Vec::new();
    if window == 0 {
        return pairs;
    }
    let mut i = 0;
    while i + 2 * window <= n {
        let Some(figure) = pitches.get(i..i + window) else {
            break;
        };
        if figure.windows(2).all(|w| matches!(w, [a, b] if a == b)) {
            i += 1;
            continue;
        }
        let next = (i + window..=n - window).find(|&j| pitches.get(j..j + window) == Some(figure));
        match next {
            Some(j) => {
                pairs.push((i, j));
                i += window;
            }
            None => i += 1,
        }
    }
    pairs
}

/// Adds the **repeat-consistency** global constraint to a fingering problem
/// laid out with `vars_per_note` variables per note (string first): for each
/// pair from [`repeat_pairs`] and each offset `k < window`, notes `i + k` and
/// `j + k` must use the same string. Not expressible in a chain DP's local
/// state; expressed in the IR as equal-value hard tables.
///
/// # Errors
///
/// [`OptIrError`] when a pair indexes past the problem (dangling variable) or
/// two aligned notes share no string.
pub fn with_repeat_consistency(
    problem: &OptProblem,
    vars_per_note: usize,
    pairs: &[(usize, usize)],
    window: usize,
) -> Result<OptProblem, OptIrError> {
    let vars = problem.vars();
    let mut hard = problem.hard().to_vec();
    for &(i, j) in pairs {
        for k in 0..window {
            let (a, b) = ((i + k) * vars_per_note, (j + k) * vars_per_note);
            let tuples = match (vars.get(a), vars.get(b)) {
                (Some(x), Some(y)) => x
                    .domain
                    .iter()
                    .filter(|v| y.domain.binary_search(v).is_ok())
                    .map(|&v| (v, v))
                    .collect(),
                // Let validation name the dangling id.
                _ => vec![(0, 0)],
            };
            hard.push(Hard::Allowed {
                a: VarId(a),
                b: VarId(b),
                tuples,
            });
        }
    }
    OptProblem::try_new(
        problem.name(),
        vars.to_vec(),
        hard,
        problem.objective().to_vec(),
    )
}

/// A deterministic tie-break for comparing solver witnesses: every objective
/// weight is multiplied by `scale = notes · max_string + 1` and each note's
/// string value is added, so `evaluate' = scale · evaluate + Σ string` — the
/// cost-optimal set is unchanged and ties resolve toward lower string numbers.
/// Returns the new problem and `scale`.
///
/// # Errors
///
/// [`OptIrError`] if the rebuilt problem is refused.
pub fn with_string_tiebreak(
    problem: &OptProblem,
    vars_per_note: usize,
) -> Result<(OptProblem, i64), OptIrError> {
    let vars = problem.vars();
    let step = vars_per_note.max(1);
    let strings: Vec<VarId> = (0..vars.len()).step_by(step).map(VarId).collect();
    let max_string = strings
        .iter()
        .filter_map(|id| vars.get(id.0))
        .filter_map(|v| v.domain.last().copied())
        .max()
        .unwrap_or(0);
    let notes = i64::try_from(strings.len()).unwrap_or(i64::MAX);
    let scale = notes.saturating_mul(max_string).saturating_add(1);
    let mut objective: Vec<Term> = problem
        .objective()
        .iter()
        .map(|term| match term {
            Term::Unary { var, costs } => Term::Unary {
                var: *var,
                costs: costs
                    .iter()
                    .map(|&(v, c)| (v, c.saturating_mul(scale)))
                    .collect(),
            },
            Term::Pair { a, b, costs } => Term::Pair {
                a: *a,
                b: *b,
                costs: costs
                    .iter()
                    .map(|&(x, y, c)| (x, y, c.saturating_mul(scale)))
                    .collect(),
            },
            Term::AbsDiff { a, b, weight } => Term::AbsDiff {
                a: *a,
                b: *b,
                weight: weight.saturating_mul(scale),
            },
            Term::NotEqual { a, b, weight } => Term::NotEqual {
                a: *a,
                b: *b,
                weight: weight.saturating_mul(scale),
            },
        })
        .collect();
    for id in strings {
        if let Some(var) = vars.get(id.0) {
            objective.push(Term::Unary {
                var: id,
                costs: var.domain.iter().map(|&v| (v, v)).collect(),
            });
        }
    }
    let rebuilt = OptProblem::try_new(
        problem.name(),
        vars.to_vec(),
        problem.hard().to_vec(),
        objective,
    )?;
    Ok((rebuilt, scale))
}

// ── private helpers ───────────────────────────────────────────────────────────

/// Per-note context captured while a voice is scanned.
#[derive(Clone, Copy)]
struct NoteContext {
    /// The hand anchor before this note's onset.
    anchor: Option<u8>,
    /// Whether the tab marks the note tapped.
    tapped: bool,
    /// The legato edge the note starts, towards the next note of its voice.
    legato_out: TechniqueEdge,
}

/// Accumulates one tablature line while a voice is scanned.
struct LineBuilder<'a> {
    track: usize,
    voice: u8,
    tuning: &'a Tuning,
    start_tick: u32,
    anchor: Option<u8>,
    tapped: Vec<bool>,
    edges: Vec<TechniqueEdge>,
    /// The legato edge the last pushed note starts.
    pending: TechniqueEdge,
    pitches: Vec<Pitch>,
    human: Vec<FretboardPosition>,
}

impl<'a> LineBuilder<'a> {
    const fn new(track: usize, voice: u8, tuning: &'a Tuning) -> Self {
        Self {
            track,
            voice,
            tuning,
            start_tick: 0,
            anchor: None,
            tapped: Vec::new(),
            edges: Vec::new(),
            pending: TechniqueEdge::Plain,
            pitches: Vec::new(),
            human: Vec::new(),
        }
    }

    fn is_empty(&self) -> bool {
        self.pitches.is_empty()
    }

    fn push(
        &mut self,
        onset: u32,
        pitch: Pitch,
        position: FretboardPosition,
        context: NoteContext,
    ) {
        if self.pitches.is_empty() {
            self.start_tick = onset;
            self.anchor = context.anchor;
        }
        self.pitches.push(pitch);
        self.human.push(position);
        self.tapped.push(context.tapped);
        self.edges.push(self.pending);
        self.pending = context.legato_out;
    }

    /// Ends the current line: kept when long enough, otherwise counted as
    /// dropped. An empty line is a no-op.
    fn flush(&mut self, cut: &LineCut, lines: &mut Vec<TabLine>, stats: &mut CutStats) {
        let len = self.pitches.len();
        if len == 0 {
            return;
        }
        let pitches = std::mem::take(&mut self.pitches);
        let human = std::mem::take(&mut self.human);
        let tapped = std::mem::take(&mut self.tapped);
        let edges = std::mem::take(&mut self.edges);
        let dangling = std::mem::take(&mut self.pending).is_legato();
        if len < cut.min_notes {
            stats.short_lines = stats.short_lines.saturating_add(1);
            stats.short_line_notes = stats.short_line_notes.saturating_add(count(len));
            return;
        }
        stats.kept_lines = stats.kept_lines.saturating_add(1);
        stats.kept_notes = stats.kept_notes.saturating_add(count(len));
        if dangling {
            stats.dangling_legato = stats.dangling_legato.saturating_add(1);
        }
        lines.push(TabLine {
            track: self.track,
            voice: self.voice,
            start_tick: self.start_tick,
            tuning: self.tuning.clone(),
            pitches,
            human,
            anchor_fret: self.anchor,
            tapped,
            edges,
        });
    }
}

fn count(n: usize) -> u64 {
    u64::try_from(n).unwrap_or(u64::MAX)
}

/// The `v1` per-note cost (mirrors production `candidate_cost`).
pub(crate) fn v1_unary(fret: u8, weights: &FingeringWeights) -> i64 {
    let base = weights.fret.saturating_mul(i64::from(fret));
    if fret == 0 {
        base.saturating_sub(weights.open_string)
    } else {
        base
    }
}

fn candidates_or_refuse(
    index: usize,
    pitch: Pitch,
    tuning: &Tuning,
    max_fret: u8,
) -> Result<Vec<FretboardPosition>, LabError> {
    let candidates = tuning.candidates(pitch, max_fret);
    if candidates.is_empty() {
        return Err(LabError::UnpositionablePitch {
            index,
            pitch: pitch.0,
        });
    }
    Ok(candidates)
}

/// Declares `s{index}` and `f{index}` and the candidate table tying them.
fn push_position_vars(
    vars: &mut Vec<IntVar>,
    hard: &mut Vec<Hard>,
    index: usize,
    candidates: &[FretboardPosition],
) -> (VarId, VarId) {
    let s = VarId(vars.len());
    vars.push(IntVar::new(
        format!("s{index}"),
        candidates.iter().map(|c| i64::from(c.string)).collect(),
    ));
    let f = VarId(vars.len());
    vars.push(IntVar::new(
        format!("f{index}"),
        candidates.iter().map(|c| i64::from(c.fret)).collect(),
    ));
    hard.push(Hard::Allowed {
        a: s,
        b: f,
        tuples: candidates
            .iter()
            .map(|c| (i64::from(c.string), i64::from(c.fret)))
            .collect(),
    });
    (s, f)
}

fn distinct_frets(candidates: &[FretboardPosition]) -> Vec<u8> {
    let mut frets: Vec<u8> = candidates.iter().map(|c| c.fret).collect();
    frets.sort_unstable();
    frets.dedup();
    frets
}

/// Builds an IR problem the builders above make valid by construction:
/// unique safe names, validated ids, non-empty domains and tables, and
/// duplicate-free cost tables.
#[allow(clippy::panic)] // documented invariant, exercised by the contract suite
fn build(name: &str, vars: Vec<IntVar>, hard: Vec<Hard>, objective: Vec<Term>) -> OptProblem {
    match OptProblem::try_new(name, vars, hard, objective) {
        Ok(problem) => problem,
        Err(e) => panic!("fingering problem builder produced invalid IR: {e}"),
    }
}

/// Per-note hand-model cost, or `None` when unreachable.
fn hand_unary(model: &HandModel, fret: u8, hand: u8) -> Option<i64> {
    let reach = model.reach(fret, hand)?;
    let w = model.weights;
    let mut cost = w.height.saturating_mul(i64::from(hand) - 1);
    match reach {
        Reach::Open => cost = cost.saturating_add(w.open_string),
        Reach::Stretch => cost = cost.saturating_add(w.stretch),
        Reach::InBox => {}
    }
    Some(cost)
}

fn hand_shift(w: &HandWeights, from: u8, to: u8) -> i64 {
    if from == to {
        0
    } else {
        w.shift.saturating_add(
            w.shift_distance
                .saturating_mul(i64::from(from.abs_diff(to))),
        )
    }
}

fn hand_transition(
    w: &HandWeights,
    a: FretboardPosition,
    ha: u8,
    b: FretboardPosition,
    hb: u8,
) -> i64 {
    hand_shift(w, ha, hb).saturating_add(
        w.string_distance
            .saturating_mul(i64::from(a.string.abs_diff(b.string))),
    )
}

/// A DP cell: the best cost reaching a state and its parent, or `None` when
/// the state is unreachable.
type Scored<P> = Option<(i64, P)>;

/// Per candidate, per hand: a [`solve_hand`] layer's cells.
type HandCells = Vec<Vec<Scored<(usize, usize)>>>;

/// One [`solve_hand`] layer transition, factored: the string term depends
/// only on the candidates and the hand term only on the hands, so
/// `min over (c, h) of D[c][h] + σ|s_c − s_c'| + τ(h, h')` equals
/// `min over h of (min over c of D[c][h] + σ|s_c − s_c'|) + τ(h, h')` —
/// `O(K²·H + K·H²)` instead of `O(K²·H²)` per step. Ties keep the lowest
/// candidate, then the lowest hand.
fn hand_step(
    prev: &HandLayer,
    candidates: &[FretboardPosition],
    unary: &[Vec<Option<i64>>],
    hands: &[u8],
    weights: &HandWeights,
) -> HandCells {
    candidates
        .iter()
        .zip(unary)
        .map(|(next, row)| {
            let via_string: Vec<Scored<usize>> = (0..hands.len())
                .map(|hi| {
                    let mut best: Scored<usize> = None;
                    for (ci, (cand, prev_row)) in
                        prev.candidates.iter().zip(&prev.cells).enumerate()
                    {
                        let Some(Some((cost, _))) = prev_row.get(hi) else {
                            continue;
                        };
                        let total = cost.saturating_add(
                            weights
                                .string_distance
                                .saturating_mul(i64::from(cand.string.abs_diff(next.string))),
                        );
                        if best.is_none_or(|(b, _)| total < b) {
                            best = Some((total, ci));
                        }
                    }
                    best
                })
                .collect();
            row.iter()
                .enumerate()
                .map(|(hj, u)| {
                    let u = (*u)?;
                    let to = *hands.get(hj)?;
                    let mut best: Scored<(usize, usize)> = None;
                    for (hi, cell) in via_string.iter().enumerate() {
                        let (Some((cost, ci)), Some(&from)) = (cell, hands.get(hi)) else {
                            continue;
                        };
                        let total = cost
                            .saturating_add(hand_shift(weights, from, to))
                            .saturating_add(u);
                        if best.is_none_or(|(b, _)| total < b) {
                            best = Some((total, (*ci, hi)));
                        }
                    }
                    best
                })
                .collect()
        })
        .collect()
}

/// One DP layer of [`solve_hand`]: per candidate, per hand, the best cost and
/// its parent `(candidate, hand)` in the previous layer.
struct HandLayer {
    candidates: Vec<FretboardPosition>,
    cells: HandCells,
}
