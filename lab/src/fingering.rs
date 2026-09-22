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

use crate::forensics::ExactRatio;
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
    /// of a kept line with no later note on the same imported string anywhere
    /// in the voice.
    pub dangling_legato: u64,
    /// Legato origins in a kept line whose same-string target exists in the
    /// imported voice but falls outside that same kept line.
    pub cross_line_legato: u64,
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
            cross_line_legato,
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
        self.cross_line_legato = self.cross_line_legato.saturating_add(cross_line_legato);
    }
}

/// An imported legato kind. Guitar Pro attaches it to the origin note; the
/// target is the first later note on the same imported string.
///
/// These are the imported span kinds. Guitar Pro stores one legato flag on the
/// note a hammer-on or pull-off starts from, without its direction, and the
/// import emits every such flag as [`SpanTechnique::HammerOn`]: a `HammerOn`
/// edge is an observed legato origin, not a known hammer-on. Direction can
/// only be derived from pitch ([`crate::technique::derived_direction`]).
///
/// [`SpanTechnique::HammerOn`]: griff_core::event::SpanTechnique::HammerOn
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TechniqueKind {
    /// The origin note carries a `HammerOn` span.
    HammerOn,
    /// The origin note carries a `PullOff` span.
    PullOff,
    /// The origin note carries a `Legato` span.
    Legato,
}

impl TechniqueKind {
    /// The legato kind a note in `group` starts, if any.
    fn out_of(group: &EventGroup) -> Option<Self> {
        group
            .technique_spans
            .iter()
            .find_map(|span| match span.technique {
                SpanTechnique::HammerOn => Some(Self::HammerOn),
                SpanTechnique::PullOff => Some(Self::PullOff),
                SpanTechnique::Legato => Some(Self::Legato),
                _ => None,
            })
    }
}

/// One observed legato relation between two notes in the same kept line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TechniqueEdge {
    /// Origin note index in the line.
    pub from: usize,
    /// Target note index in the line.
    pub to: usize,
    /// Imported technique kind on the origin note.
    pub kind: TechniqueKind,
}

impl TechniqueEdge {
    /// Builds an edge whose target is strictly later than its origin.
    #[must_use]
    pub const fn new(from: usize, to: usize, kind: TechniqueKind) -> Self {
        Self { from, to, kind }
    }
}

/// Imported context for a resolved legato target outside its origin's kept
/// line. The position keeps the source file's string orientation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TechniqueTarget {
    /// Stable index in the imported voice, assigned before line slicing.
    pub note_id: usize,
    /// Absolute onset tick in the imported score.
    pub onset: u32,
    /// Imported note duration in ticks.
    pub duration: u32,
    /// Imported pitch.
    pub pitch: Pitch,
    /// Imported, unoriented string and fret.
    pub original_position: FretboardPosition,
    /// Whether the target carries `NoteMark::Tap`.
    pub tapped: bool,
}

/// Exact descriptive measurements between a legato origin and its resolved
/// same-string target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub struct TechniqueSpanStats {
    /// Target atom id minus origin atom id.
    pub note_distance: usize,
    /// Distinct voice onsets strictly between origin and target onsets.
    pub intervening_onsets: usize,
    /// Imported note atoms strictly between the two stable note ids.
    pub intervening_note_atoms: usize,
    /// Positioned notes on the origin string at strictly intervening onsets.
    pub intervening_origin_string_notes: usize,
    /// Positioned notes on other strings at strictly intervening onsets.
    pub intervening_other_string_notes: usize,
    /// Unpositioned notes at strictly intervening onsets.
    pub intervening_unpositioned_notes: usize,
    /// Target onset minus origin onset, in ticks.
    pub delta_ticks: u32,
    /// Exact reduced `delta_ticks / ticks_per_quarter`.
    pub delta_quarters: ExactRatio,
    /// Signed target-minus-origin pitch interval in semitones.
    pub pitch_interval_semitones: i16,
    /// Absolute fret distance in the imported positions.
    pub fret_distance: u8,
    /// Whether the imported target is an open string.
    pub target_open: bool,
}

/// A real reason the slicing control flow ended or separated a fragment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LineBoundaryCause {
    /// Silence reached the configured rest threshold.
    RestCut,
    /// More than one note atom shared the onset.
    ChordOnset,
    /// The single note had no imported position.
    Unpositioned,
    /// The imported fret exceeded the configured maximum.
    BeyondMaxFret,
    /// The imported position did not sound the imported pitch.
    PitchMismatch,
}

/// One boundary location; more than one real cause may apply at the same
/// onset (for example, a long rest followed by a chord).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct LineBoundary {
    /// First stable voice-note id at or after the boundary.
    pub before_note_id: usize,
    /// End of the atom-id range excluded at this onset. Equal to
    /// `before_note_id` for a pure rest cut, which excludes no note atom.
    pub excluded_note_ids_end: usize,
    /// Absolute onset at the boundary.
    pub onset: u32,
    /// Causes in slicing-control-flow order.
    pub causes: Vec<LineBoundaryCause>,
}

/// Where the resolved target went under the unchanged line cut.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetDisposition {
    /// The target belongs to a retained line.
    KeptLine,
    /// The target belonged to a valid fragment dropped for being too short.
    DroppedShortLine,
    /// The target onset itself was excluded by a typed boundary cause.
    Excluded,
}

/// Diagnostic boundary context for a cross-line relation.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct CrossLineBoundary {
    /// Ordered unique boundary locations crossed by the relation.
    pub boundaries: Vec<LineBoundary>,
    /// `boundaries.len()`, stored explicitly in the artifact contract.
    pub line_boundaries_crossed: usize,
    /// Retained fragments strictly between origin and target fragments.
    pub intervening_kept_fragments: usize,
    /// Dropped short fragments strictly between origin and target, including a
    /// dropped target fragment when applicable.
    pub intervening_dropped_fragments: usize,
    /// Target disposition under the unchanged cut.
    pub target_disposition: TargetDisposition,
    /// Start tick of the target's retained line, when it has one.
    pub target_line_start_tick: Option<u32>,
    /// Whether the target is in the next retained line of the same voice.
    pub target_in_next_kept_line: bool,
}

/// One atom of the imported onset containing a cross-line target. Stable ids
/// are assigned before slicing; positions retain the imported string order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ImportedChordAtom {
    /// Stable imported-voice note id.
    pub note_id: usize,
    /// Imported duration in ticks.
    pub duration: u32,
    /// Imported pitch.
    pub pitch: Pitch,
    /// Imported explicit position, when present.
    pub original_position: Option<FretboardPosition>,
    /// Whether the atom is marked tapped.
    pub tapped: bool,
}

/// A resolved legato relation whose target does not survive in the same kept
/// line as its origin. It is forensic context, not an objective edge.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CrossLineTechniqueEdge {
    /// Origin index in the kept line.
    pub from: usize,
    /// Stable origin index in the imported voice.
    pub origin_note_id: usize,
    /// Resolved target in the imported voice.
    pub target: TechniqueTarget,
    /// Imported technique kind on the origin note.
    pub kind: TechniqueKind,
    /// Exact temporal/note context across the imported voice.
    pub span: TechniqueSpanStats,
    /// Why the target does not belong to the origin line.
    pub boundary: CrossLineBoundary,
    /// Every note atom at the target onset when it is a chord, ordered by
    /// stable imported id. Empty for non-chord targets.
    pub target_chord: Vec<ImportedChordAtom>,
    /// Latest fretting-hand anchor strictly before the target onset. Tapped
    /// and open notes do not move/establish the anchor; `None` is retained.
    pub target_anchor_fret: Option<u8>,
}

/// One monophonic tablature line with the tab author's positions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabLine {
    /// Track index in the score.
    pub track: usize,
    /// Voice id within the track.
    pub voice: u8,
    /// Imported score resolution, used to normalize forensic onset gaps.
    pub ticks_per_quarter: u32,
    /// Onset tick of the first note.
    pub start_tick: u32,
    /// The track tuning.
    pub tuning: Tuning,
    /// Imported tuning before possible low-first orientation normalization.
    pub original_tuning: Tuning,
    /// Pitches, in onset order.
    pub pitches: Vec<Pitch>,
    /// Stable imported-voice note ids, one per pitch.
    pub note_ids: Vec<usize>,
    /// Absolute onset ticks, one per pitch.
    pub onsets: Vec<u32>,
    /// Imported duration ticks, one per pitch.
    pub durations: Vec<u32>,
    /// Imported positions before any string-orientation normalization.
    pub original_positions: Vec<FretboardPosition>,
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
    /// Legato relations whose origin and same-string target both survive in
    /// this line. Absence means a plain transition; edges may skip intervening
    /// notes on other strings.
    pub edges: Vec<TechniqueEdge>,
    /// Resolved relations whose target lies outside this kept line. These are
    /// retained only for projection auditing and never enter an objective.
    pub cross_line_edges: Vec<CrossLineTechniqueEdge>,
}

/// Computes exact diagnostic span measurements for a retained relation.
/// Returns `None` when the edge or parallel line metadata is invalid.
#[must_use]
pub fn within_line_span(tab: &TabLine, edge: TechniqueEdge) -> Option<TechniqueSpanStats> {
    if edge.from >= edge.to || edge.to >= tab.pitches.len() {
        return None;
    }
    let origin_onset = *tab.onsets.get(edge.from)?;
    let target_onset = *tab.onsets.get(edge.to)?;
    let origin_id = *tab.note_ids.get(edge.from)?;
    let target_id = *tab.note_ids.get(edge.to)?;
    let origin = *tab.original_positions.get(edge.from)?;
    let target = *tab.original_positions.get(edge.to)?;
    let between = edge.from + 1..edge.to;
    let intervening_onsets = distinct_count(&tab.onsets[between.clone()]);
    let mut origin_string = 0;
    let mut other_strings = 0;
    for position in &tab.original_positions[between] {
        if position.string == origin.string {
            origin_string += 1;
        } else {
            other_strings += 1;
        }
    }
    Some(TechniqueSpanStats {
        note_distance: target_id.checked_sub(origin_id)?,
        intervening_onsets,
        intervening_note_atoms: target_id.checked_sub(origin_id)?.saturating_sub(1),
        intervening_origin_string_notes: origin_string,
        intervening_other_string_notes: other_strings,
        intervening_unpositioned_notes: 0,
        delta_ticks: target_onset.checked_sub(origin_onset)?,
        delta_quarters: ExactRatio::new(
            u64::from(target_onset.checked_sub(origin_onset)?),
            u64::from(tab.ticks_per_quarter),
        ),
        pitch_interval_semitones: i16::from(tab.pitches[edge.to].0)
            - i16::from(tab.pitches[edge.from].0),
        fret_distance: origin.fret.abs_diff(target.fret),
        target_open: target.fret == 0,
    })
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
        // Each note with the legato origin flag carried by its event group.
        let mut notes: Vec<(&AtomNote, Option<TechniqueKind>)> = voice
            .event_groups
            .iter()
            .flat_map(|g| {
                let out = TechniqueKind::out_of(g);
                g.atoms.iter().filter_map(move |a| match a {
                    AtomEvent::Note(n) => Some((n, out)),
                    AtomEvent::Rest(_) => None,
                })
            })
            .collect();
        notes.sort_by_key(|(n, _)| n.absolute_start.0);

        // Resolve Guitar Pro's origin flag before slicing: its target is the
        // first strictly later note in this imported voice on the same
        // original string. Notes at the same onset never target each other.
        let mut targets = vec![None; notes.len()];
        let mut next_on_string: [Option<usize>; 256] = [None; 256];
        let mut end = notes.len();
        while end > 0 {
            let onset = notes[end - 1].0.absolute_start.0;
            let start = notes[..end]
                .iter()
                .rposition(|(note, _)| note.absolute_start.0 != onset)
                .map_or(0, |i| i + 1);
            for i in start..end {
                if notes[i].1.is_some() {
                    targets[i] = notes[i]
                        .0
                        .position
                        .and_then(|p| next_on_string[usize::from(p.position.string)])
                        .and_then(|note_id| {
                            let target = notes[note_id].0;
                            target.position.and_then(|position| {
                                imported_span(
                                    &notes,
                                    i,
                                    note_id,
                                    u32::from(score.ticks_per_quarter),
                                )
                                .map(|span| {
                                    ResolvedTechniqueTarget {
                                        target: TechniqueTarget {
                                            note_id,
                                            onset: target.absolute_start.0,
                                            duration: target.duration.0,
                                            pitch: target.pitch,
                                            original_position: position.position,
                                            tapped: target.marks.contains(NoteMark::Tap),
                                        },
                                        span,
                                    }
                                })
                            })
                        });
                }
            }
            for (i, (note, _)) in notes[start..end].iter().enumerate() {
                if let Some(position) = note.position {
                    next_on_string[usize::from(position.position.string)] = Some(start + i);
                }
            }
            end = start;
        }

        let mut line = LineBuilder::new(
            track_index,
            voice.id,
            u32::from(score.ticks_per_quarter),
            &tuning,
            &track.tuning,
        );
        let mut sounding_until: Option<u64> = None;
        let voice_line_start = lines.len();
        let mut fragments = Vec::new();
        let mut boundaries = Vec::new();
        // Lowest fretted position at the latest onset seen so far.
        let mut last_fretted: Option<u8> = None;
        let mut note_index = 0;
        let mut rest = notes.as_slice();
        while let Some((first, _)) = rest.first() {
            let onset = first.absolute_start.0;
            let width = rest
                .iter()
                .position(|(n, _)| n.absolute_start.0 != onset)
                .unwrap_or(rest.len());
            let (group, tail) = rest.split_at(width);
            let group_start = note_index;
            note_index += width;
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
                record_boundary(
                    &mut boundaries,
                    group_start,
                    group_start,
                    onset,
                    LineBoundaryCause::RestCut,
                );
                line.flush(cut, &mut lines, &mut stats, &mut fragments);
            }

            let [(note, legato_out)] = group else {
                stats.chord_onsets = stats.chord_onsets.saturating_add(1);
                record_boundary(
                    &mut boundaries,
                    group_start,
                    group_start.saturating_add(width),
                    onset,
                    LineBoundaryCause::ChordOnset,
                );
                line.flush(cut, &mut lines, &mut stats, &mut fragments);
                continue;
            };
            let Some(position) = note.position.map(|p| p.position) else {
                stats.unpositioned = stats.unpositioned.saturating_add(1);
                record_boundary(
                    &mut boundaries,
                    group_start,
                    group_start.saturating_add(width),
                    onset,
                    LineBoundaryCause::Unpositioned,
                );
                line.flush(cut, &mut lines, &mut stats, &mut fragments);
                continue;
            };
            if position.fret > cut.max_fret {
                stats.beyond_max_fret = stats.beyond_max_fret.saturating_add(1);
                record_boundary(
                    &mut boundaries,
                    group_start,
                    group_start.saturating_add(width),
                    onset,
                    LineBoundaryCause::BeyondMaxFret,
                );
                line.flush(cut, &mut lines, &mut stats, &mut fragments);
                continue;
            }
            if track.tuning.pitch_at(position) != Some(note.pitch) {
                stats.pitch_mismatch = stats.pitch_mismatch.saturating_add(1);
                record_boundary(
                    &mut boundaries,
                    group_start,
                    group_start.saturating_add(width),
                    onset,
                    LineBoundaryCause::PitchMismatch,
                );
                line.flush(cut, &mut lines, &mut stats, &mut fragments);
                continue;
            }
            line.push(
                LineNote {
                    onset,
                    duration: note.duration.0,
                    pitch: note.pitch,
                    original_position: position,
                    position: orient(position),
                },
                NoteContext {
                    note_id: group_start,
                    anchor: anchor_here,
                    tapped: note.marks.contains(NoteMark::Tap),
                    legato_out: *legato_out,
                    legato_target: targets[group_start],
                },
            );
        }
        line.flush(cut, &mut lines, &mut stats, &mut fragments);
        finalize_cross_line_boundaries(
            &mut lines,
            voice_line_start,
            &notes,
            &fragments,
            &boundaries,
        );
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
    /// Stable index in the imported voice, assigned before slicing.
    note_id: usize,
    /// The hand anchor before this note's onset.
    anchor: Option<u8>,
    /// Whether the tab marks the note tapped.
    tapped: bool,
    /// The imported legato kind this note starts, if any.
    legato_out: Option<TechniqueKind>,
    /// Stable imported-voice index of its same-string target, if resolved.
    legato_target: Option<ResolvedTechniqueTarget>,
}

#[derive(Clone, Copy)]
struct LineNote {
    onset: u32,
    duration: u32,
    pitch: Pitch,
    original_position: FretboardPosition,
    position: FretboardPosition,
}

#[derive(Clone, Copy)]
struct ResolvedTechniqueTarget {
    target: TechniqueTarget,
    span: TechniqueSpanStats,
}

#[derive(Clone, Copy)]
struct PendingTechnique {
    from: usize,
    target: Option<ResolvedTechniqueTarget>,
    kind: TechniqueKind,
}

struct LineFragment {
    note_ids: Vec<usize>,
    start_tick: u32,
    kept_line_index: Option<usize>,
}

/// Accumulates one tablature line while a voice is scanned.
struct LineBuilder<'a> {
    track: usize,
    voice: u8,
    ticks_per_quarter: u32,
    tuning: &'a Tuning,
    original_tuning: &'a Tuning,
    start_tick: u32,
    anchor: Option<u8>,
    tapped: Vec<bool>,
    onsets: Vec<u32>,
    durations: Vec<u32>,
    original_positions: Vec<FretboardPosition>,
    note_ids: Vec<usize>,
    origins: Vec<PendingTechnique>,
    pitches: Vec<Pitch>,
    human: Vec<FretboardPosition>,
}

impl<'a> LineBuilder<'a> {
    const fn new(
        track: usize,
        voice: u8,
        ticks_per_quarter: u32,
        tuning: &'a Tuning,
        original_tuning: &'a Tuning,
    ) -> Self {
        Self {
            track,
            voice,
            ticks_per_quarter,
            tuning,
            original_tuning,
            start_tick: 0,
            anchor: None,
            tapped: Vec::new(),
            onsets: Vec::new(),
            durations: Vec::new(),
            original_positions: Vec::new(),
            note_ids: Vec::new(),
            origins: Vec::new(),
            pitches: Vec::new(),
            human: Vec::new(),
        }
    }

    fn is_empty(&self) -> bool {
        self.pitches.is_empty()
    }

    fn push(&mut self, note: LineNote, context: NoteContext) {
        if self.pitches.is_empty() {
            self.start_tick = note.onset;
            self.anchor = context.anchor;
        }
        self.pitches.push(note.pitch);
        self.onsets.push(note.onset);
        self.durations.push(note.duration);
        self.original_positions.push(note.original_position);
        self.human.push(note.position);
        self.tapped.push(context.tapped);
        self.note_ids.push(context.note_id);
        if let Some(kind) = context.legato_out {
            self.origins.push(PendingTechnique {
                from: context.note_id,
                target: context.legato_target,
                kind,
            });
        }
    }

    /// Ends the current line: kept when long enough, otherwise counted as
    /// dropped. An empty line is a no-op.
    fn flush(
        &mut self,
        cut: &LineCut,
        lines: &mut Vec<TabLine>,
        stats: &mut CutStats,
        fragments: &mut Vec<LineFragment>,
    ) {
        let len = self.pitches.len();
        if len == 0 {
            return;
        }
        let pitches = std::mem::take(&mut self.pitches);
        let human = std::mem::take(&mut self.human);
        let tapped = std::mem::take(&mut self.tapped);
        let onsets = std::mem::take(&mut self.onsets);
        let durations = std::mem::take(&mut self.durations);
        let original_positions = std::mem::take(&mut self.original_positions);
        let note_ids = std::mem::take(&mut self.note_ids);
        let origins = std::mem::take(&mut self.origins);
        if len < cut.min_notes {
            stats.short_lines = stats.short_lines.saturating_add(1);
            stats.short_line_notes = stats.short_line_notes.saturating_add(count(len));
            fragments.push(LineFragment {
                note_ids,
                start_tick: self.start_tick,
                kept_line_index: None,
            });
            return;
        }
        stats.kept_lines = stats.kept_lines.saturating_add(1);
        stats.kept_notes = stats.kept_notes.saturating_add(count(len));
        let kept_line_index = lines.len();
        fragments.push(LineFragment {
            note_ids: note_ids.clone(),
            start_tick: self.start_tick,
            kept_line_index: Some(kept_line_index),
        });
        let mut edges = Vec::new();
        let mut cross_line_edges = Vec::new();
        for origin in origins {
            let Some(target) = origin.target else {
                stats.dangling_legato = stats.dangling_legato.saturating_add(1);
                continue;
            };
            let from = note_ids.iter().position(|&id| id == origin.from);
            let to = note_ids.iter().position(|&id| id == target.target.note_id);
            match (from, to) {
                (Some(from), Some(to)) => edges.push(TechniqueEdge::new(from, to, origin.kind)),
                (Some(from), None) => {
                    stats.cross_line_legato = stats.cross_line_legato.saturating_add(1);
                    cross_line_edges.push(CrossLineTechniqueEdge {
                        from,
                        origin_note_id: origin.from,
                        target: target.target,
                        kind: origin.kind,
                        span: target.span,
                        boundary: CrossLineBoundary {
                            boundaries: Vec::new(),
                            line_boundaries_crossed: 0,
                            intervening_kept_fragments: 0,
                            intervening_dropped_fragments: 0,
                            target_disposition: TargetDisposition::Excluded,
                            target_line_start_tick: None,
                            target_in_next_kept_line: false,
                        },
                        target_chord: Vec::new(),
                        target_anchor_fret: None,
                    });
                }
                _ => {}
            }
        }
        lines.push(TabLine {
            track: self.track,
            voice: self.voice,
            ticks_per_quarter: self.ticks_per_quarter,
            start_tick: self.start_tick,
            tuning: self.tuning.clone(),
            original_tuning: self.original_tuning.clone(),
            pitches,
            note_ids,
            onsets,
            durations,
            original_positions,
            human,
            anchor_fret: self.anchor,
            tapped,
            edges,
            cross_line_edges,
        });
    }
}

fn imported_span(
    notes: &[(&AtomNote, Option<TechniqueKind>)],
    origin_id: usize,
    target_id: usize,
    ticks_per_quarter: u32,
) -> Option<TechniqueSpanStats> {
    let origin = notes.get(origin_id)?.0;
    let target = notes.get(target_id)?.0;
    let origin_position = origin.position?.position;
    let target_position = target.position?.position;
    let mut last_onset = None;
    let mut intervening_onsets = 0;
    let mut origin_string = 0;
    let mut other_strings = 0;
    let mut unpositioned = 0;
    for (note, _) in notes.get(origin_id + 1..target_id)? {
        let onset = note.absolute_start.0;
        if onset <= origin.absolute_start.0 || onset >= target.absolute_start.0 {
            continue;
        }
        if last_onset != Some(onset) {
            intervening_onsets += 1;
            last_onset = Some(onset);
        }
        match note.position.map(|position| position.position) {
            Some(position) if position.string == origin_position.string => origin_string += 1,
            Some(_) => other_strings += 1,
            None => unpositioned += 1,
        }
    }
    let delta_ticks = target
        .absolute_start
        .0
        .checked_sub(origin.absolute_start.0)?;
    Some(TechniqueSpanStats {
        note_distance: target_id.checked_sub(origin_id)?,
        intervening_onsets,
        intervening_note_atoms: target_id.checked_sub(origin_id)?.saturating_sub(1),
        intervening_origin_string_notes: origin_string,
        intervening_other_string_notes: other_strings,
        intervening_unpositioned_notes: unpositioned,
        delta_ticks,
        delta_quarters: ExactRatio::new(u64::from(delta_ticks), u64::from(ticks_per_quarter)),
        pitch_interval_semitones: i16::from(target.pitch.0) - i16::from(origin.pitch.0),
        fret_distance: origin_position.fret.abs_diff(target_position.fret),
        target_open: target_position.fret == 0,
    })
}

fn record_boundary(
    boundaries: &mut Vec<LineBoundary>,
    before_note_id: usize,
    excluded_note_ids_end: usize,
    onset: u32,
    cause: LineBoundaryCause,
) {
    if let Some(boundary) = boundaries
        .last_mut()
        .filter(|boundary| boundary.before_note_id == before_note_id)
    {
        boundary.excluded_note_ids_end = boundary.excluded_note_ids_end.max(excluded_note_ids_end);
        if !boundary.causes.contains(&cause) {
            boundary.causes.push(cause);
        }
    } else {
        boundaries.push(LineBoundary {
            before_note_id,
            excluded_note_ids_end,
            onset,
            causes: vec![cause],
        });
    }
}

fn finalize_cross_line_boundaries(
    lines: &mut [TabLine],
    voice_line_start: usize,
    notes: &[(&AtomNote, Option<TechniqueKind>)],
    fragments: &[LineFragment],
    boundaries: &[LineBoundary],
) {
    let note_count = notes.len();
    let mut note_fragment = vec![None; note_count];
    for (fragment_index, fragment) in fragments.iter().enumerate() {
        for &note_id in &fragment.note_ids {
            if let Some(slot) = note_fragment.get_mut(note_id) {
                *slot = Some(fragment_index);
            }
        }
    }
    for line in lines.iter_mut().skip(voice_line_start) {
        for edge in &mut line.cross_line_edges {
            let Some(origin_fragment) = note_fragment.get(edge.origin_note_id).copied().flatten()
            else {
                continue;
            };
            let target_fragment = note_fragment.get(edge.target.note_id).copied().flatten();
            let target_disposition = target_fragment.map_or(TargetDisposition::Excluded, |index| {
                if fragments[index].kept_line_index.is_some() {
                    TargetDisposition::KeptLine
                } else {
                    TargetDisposition::DroppedShortLine
                }
            });
            let target_line_start_tick = target_fragment.and_then(|index| {
                fragments[index]
                    .kept_line_index
                    .map(|_| fragments[index].start_tick)
            });
            let next_kept_fragment = fragments
                .iter()
                .enumerate()
                .skip(origin_fragment + 1)
                .find_map(|(index, fragment)| fragment.kept_line_index.map(|_| index));
            let target_in_next_kept_line = target_fragment.is_some_and(|target_index| {
                fragments[target_index].kept_line_index.is_some()
                    && next_kept_fragment == Some(target_index)
            });

            let mut intervening_kept = 0;
            let mut intervening_dropped = 0;
            for (index, fragment) in fragments.iter().enumerate().skip(origin_fragment + 1) {
                let before_target = fragment
                    .note_ids
                    .first()
                    .is_some_and(|first| *first < edge.target.note_id);
                let is_target = target_fragment == Some(index);
                if !before_target && !is_target {
                    break;
                }
                if is_target {
                    if fragment.kept_line_index.is_none() {
                        intervening_dropped += 1;
                    }
                    break;
                }
                if fragment.kept_line_index.is_some() {
                    intervening_kept += 1;
                } else {
                    intervening_dropped += 1;
                }
            }
            let crossed: Vec<LineBoundary> = boundaries
                .iter()
                .filter(|boundary| {
                    edge.origin_note_id < boundary.before_note_id
                        && boundary.before_note_id <= edge.target.note_id
                })
                .cloned()
                .collect();
            edge.boundary = CrossLineBoundary {
                line_boundaries_crossed: crossed.len(),
                boundaries: crossed,
                intervening_kept_fragments: intervening_kept,
                intervening_dropped_fragments: intervening_dropped,
                target_disposition,
                target_line_start_tick,
                target_in_next_kept_line,
            };
            let target_onset = edge.target.onset;
            let target_chord: Vec<ImportedChordAtom> = notes
                .iter()
                .enumerate()
                .filter(|(_, (note, _))| note.absolute_start.0 == target_onset)
                .map(|(note_id, (note, _))| ImportedChordAtom {
                    note_id,
                    duration: note.duration.0,
                    pitch: note.pitch,
                    original_position: note.position.map(|position| position.position),
                    tapped: note.marks.contains(NoteMark::Tap),
                })
                .collect();
            if target_chord.len() > 1 {
                edge.target_chord = target_chord;
            }
            edge.target_anchor_fret = latest_fretting_anchor_before(notes, target_onset);
        }
    }
}

fn latest_fretting_anchor_before(
    notes: &[(&AtomNote, Option<TechniqueKind>)],
    before_onset: u32,
) -> Option<u8> {
    let latest_onset = notes
        .iter()
        .filter(|(note, _)| {
            note.absolute_start.0 < before_onset
                && !note.marks.contains(NoteMark::Tap)
                && note
                    .position
                    .is_some_and(|position| position.position.fret > 0)
        })
        .map(|(note, _)| note.absolute_start.0)
        .max()?;
    notes
        .iter()
        .filter(|(note, _)| {
            note.absolute_start.0 == latest_onset && !note.marks.contains(NoteMark::Tap)
        })
        .filter_map(|(note, _)| note.position.map(|position| position.position.fret))
        .filter(|fret| *fret > 0)
        .min()
}

fn distinct_count<T: PartialEq>(values: &[T]) -> usize {
    values
        .windows(2)
        .filter(|pair| pair[0] != pair[1])
        .count()
        .saturating_add(usize::from(!values.is_empty()))
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
