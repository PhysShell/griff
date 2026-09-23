//! Exact Lab-only chord-event representation regimes.
//!
//! Target chord positions are deliberately absent from [`ChordEventProblem`].
//! They enter only through [`ObservedChordVoicing`] evaluation.

use std::collections::{BTreeMap, BTreeSet};

use griff_core::event::{FretboardPosition, NoteMark, Pitch, SpanTechnique, Tuning};
use griff_core::fretboard::FingeringWeights;
use griff_core::score::{AtomEvent, Score};
use thiserror::Error;

use crate::fingering::v1_unary;

/// Stable identity of one chord onset.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ChordEventIdentity {
    /// Repository-external source identity.
    pub source: String,
    /// Normalized song identity used for blocking.
    pub song_key: String,
    /// Track index.
    pub track: usize,
    /// Imported voice id.
    pub voice: u8,
    /// Absolute onset tick.
    pub onset: u32,
}

/// One solver-visible chord atom. It contains no imported target position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChordEventAtom {
    /// Stable imported-voice note id.
    pub note_id: usize,
    /// Imported pitch.
    pub pitch: Pitch,
    /// Imported duration in ticks.
    pub duration: u32,
    /// Whether the atom is tapped.
    pub tapped: bool,
}

/// Latest preceding fretting-hand anchor under #199/#210 semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HandAnchor {
    /// Anchor fret.
    pub fret: u8,
    /// Onset that established it.
    pub onset: u32,
    /// Stable source note id when practical.
    pub source_note_id: Option<usize>,
}

/// Technique kind retained by the representation experiment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TechniqueKind {
    /// Guitar Pro legato origin (direction may be derived separately).
    Legato,
    /// Hammer-on when explicitly known.
    HammerOn,
    /// Pull-off when explicitly known.
    PullOff,
}

/// Earlier observed context targeting one stable chord atom.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct IncomingTechnique {
    /// Technique kind.
    pub kind: TechniqueKind,
    /// Stable origin id.
    pub origin_note_id: usize,
    /// Origin onset.
    pub origin_onset: u32,
    /// Origin pitch.
    pub origin_pitch: Pitch,
    /// Earlier imported position; this is input context, not the target answer.
    pub origin_position: FretboardPosition,
    /// Stable target atom id inside this chord.
    pub target_atom_id: usize,
}

/// Solver input shared by the four registered information regimes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChordEventProblem {
    identity: ChordEventIdentity,
    tuning: Tuning,
    max_fret: u8,
    atoms: Vec<ChordEventAtom>,
    preceding_hand: Option<HandAnchor>,
    incoming_techniques: Vec<IncomingTechnique>,
}

impl ChordEventProblem {
    /// Builds a validated solver input containing no imported target positions.
    ///
    /// # Errors
    ///
    /// Refuses non-chords, duplicate atom ids, missing targets, and origins
    /// that do not strictly precede the chord.
    #[allow(clippy::too_many_arguments)] // explicit registered input channels
    pub fn new(
        identity: ChordEventIdentity,
        tuning: Tuning,
        max_fret: u8,
        atoms: Vec<ChordEventAtom>,
        preceding_hand: Option<HandAnchor>,
        incoming_techniques: Vec<IncomingTechnique>,
    ) -> Result<Self, ChordEventError> {
        if atoms.len() < 2 {
            return Err(ChordEventError::NotAChord);
        }
        let mut ids = BTreeSet::new();
        for atom in &atoms {
            if !ids.insert(atom.note_id) {
                return Err(ChordEventError::DuplicateAtomId(atom.note_id));
            }
        }
        for incoming in &incoming_techniques {
            if !ids.contains(&incoming.target_atom_id) {
                return Err(ChordEventError::MissingTechniqueTarget(
                    incoming.target_atom_id,
                ));
            }
            if incoming.origin_onset >= identity.onset {
                return Err(ChordEventError::NonPrecedingTechnique {
                    origin: incoming.origin_onset,
                    chord: identity.onset,
                });
            }
        }
        Ok(Self {
            identity,
            tuning,
            max_fret,
            atoms,
            preceding_hand,
            incoming_techniques,
        })
    }

    /// Event identity.
    #[must_use]
    pub const fn identity(&self) -> &ChordEventIdentity {
        &self.identity
    }

    /// Solver-visible atoms.
    #[must_use]
    pub fn atoms(&self) -> &[ChordEventAtom] {
        &self.atoms
    }

    /// Preceding anchor.
    #[must_use]
    pub const fn preceding_hand(&self) -> Option<HandAnchor> {
        self.preceding_hand
    }

    /// Incoming technique inputs.
    #[must_use]
    pub fn incoming_techniques(&self) -> &[IncomingTechnique] {
        &self.incoming_techniques
    }

    /// Imported tuning in its original string orientation.
    #[must_use]
    pub const fn tuning(&self) -> &Tuning {
        &self.tuning
    }

    /// Registered maximum fret.
    #[must_use]
    pub const fn max_fret(&self) -> u8 {
        self.max_fret
    }

    /// Returns the same information regime with a substituted anchor.
    #[must_use]
    pub fn with_anchor(&self, anchor: Option<HandAnchor>) -> Self {
        let mut replaced = self.clone();
        replaced.preceding_hand = anchor;
        replaced
    }
}

/// Typed status of one chord onset in the general census.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ChordCensusStatus {
    /// Every atom has a usable explicit imported position.
    CompleteExplicit,
    /// At least one atom has no imported position.
    IncompletePosition,
    /// An explicit position does not sound the imported pitch.
    PitchMismatch,
    /// Two imported atoms use one physical string.
    DuplicateExplicitString,
    /// At least one imported position exceeds the registered fret range.
    BeyondMaxFret,
    /// A typed problem or observation could not be constructed.
    OtherUnsupported,
}

/// One onset in the complete chord census.
#[derive(Debug, Clone, PartialEq)]
pub struct ChordCensusEvent {
    /// Stable event identity.
    pub identity: ChordEventIdentity,
    /// Typed observation status.
    pub status: ChordCensusStatus,
    /// Number of note atoms at the onset.
    pub atom_count: usize,
    /// Solver input, present for complete explicit observations.
    pub problem: Option<ChordEventProblem>,
    /// Separate imported evaluation object.
    pub observed: Option<ObservedChordVoicing>,
}

#[derive(Clone)]
struct ImportedNote {
    note_id: usize,
    onset: u32,
    duration: u32,
    pitch: Pitch,
    position: Option<FretboardPosition>,
    tapped: bool,
    technique: Option<TechniqueKind>,
}

/// Extracts every chord onset from one imported track exactly once.
///
/// Stable ids and incoming technique targets are resolved over the whole
/// imported voice before any chord is classified.
///
/// # Errors
///
/// Refuses a missing track index.
pub fn chord_event_census(
    score: &Score,
    source: &str,
    song_key: &str,
    track_index: usize,
    max_fret: u8,
) -> Result<Vec<ChordCensusEvent>, ChordEventError> {
    let track = score
        .tracks
        .get(track_index)
        .ok_or(ChordEventError::MissingTrack(track_index))?;
    let mut output = Vec::new();
    for voice in &track.voices {
        let mut notes: Vec<ImportedNote> = voice
            .event_groups
            .iter()
            .flat_map(|group| {
                let technique =
                    imported_technique(group.technique_spans.iter().map(|span| span.technique));
                group.atoms.iter().filter_map(move |atom| match atom {
                    AtomEvent::Note(note) => Some((note, technique)),
                    AtomEvent::Rest(_) => None,
                })
            })
            .enumerate()
            .map(|(order, (note, technique))| ImportedNote {
                note_id: order,
                onset: note.absolute_start.0,
                duration: note.duration.0,
                pitch: note.pitch,
                position: note.position.map(|position| position.position),
                tapped: note.marks.contains(NoteMark::Tap),
                technique,
            })
            .collect();
        notes.sort_by_key(|note| (note.onset, note.note_id));
        for (note_id, note) in notes.iter_mut().enumerate() {
            note.note_id = note_id;
        }
        let targets = projected_targets(&notes);
        let mut anchor: Option<HandAnchor> = None;
        let mut start = 0;
        while start < notes.len() {
            let onset = notes[start].onset;
            let end = notes[start..]
                .iter()
                .position(|note| note.onset != onset)
                .map_or(notes.len(), |width| start + width);
            let group = &notes[start..end];
            if group.len() >= 2 {
                output.push(build_census_event(
                    source,
                    song_key,
                    track_index,
                    voice.id,
                    onset,
                    &track.tuning,
                    max_fret,
                    group,
                    &notes,
                    &targets,
                    anchor,
                ));
            }
            if let Some(note) = group
                .iter()
                .filter(|note| !note.tapped)
                .filter_map(|note| note.position.map(|position| (note, position)))
                .filter(|(_, position)| position.fret > 0)
                .min_by_key(|(_, position)| position.fret)
            {
                anchor = Some(HandAnchor {
                    fret: note.1.fret,
                    onset,
                    source_note_id: Some(note.0.note_id),
                });
            }
            start = end;
        }
    }
    output.sort_by(|left, right| left.identity.cmp(&right.identity));
    Ok(output)
}

fn imported_technique(
    mut techniques: impl Iterator<Item = SpanTechnique>,
) -> Option<TechniqueKind> {
    techniques.find_map(|technique| match technique {
        SpanTechnique::HammerOn => Some(TechniqueKind::HammerOn),
        SpanTechnique::PullOff => Some(TechniqueKind::PullOff),
        SpanTechnique::Legato => Some(TechniqueKind::Legato),
        _ => None,
    })
}

fn projected_targets(notes: &[ImportedNote]) -> Vec<Option<usize>> {
    let mut targets = vec![None; notes.len()];
    let mut next = [None; 256];
    let mut end = notes.len();
    while end > 0 {
        let onset = notes[end - 1].onset;
        let start = notes[..end]
            .iter()
            .rposition(|note| note.onset != onset)
            .map_or(0, |index| index + 1);
        for index in start..end {
            if notes[index].technique.is_some() {
                targets[index] = notes[index]
                    .position
                    .and_then(|position| next[usize::from(position.string)]);
            }
        }
        for (offset, note) in notes[start..end].iter().enumerate() {
            if let Some(position) = note.position {
                next[usize::from(position.string)] = Some(start + offset);
            }
        }
        end = start;
    }
    targets
}

#[allow(clippy::too_many_arguments)] // transparent census identity and context
fn build_census_event(
    source: &str,
    song_key: &str,
    track: usize,
    voice: u8,
    onset: u32,
    tuning: &Tuning,
    max_fret: u8,
    group: &[ImportedNote],
    voice_notes: &[ImportedNote],
    targets: &[Option<usize>],
    anchor: Option<HandAnchor>,
) -> ChordCensusEvent {
    let identity = ChordEventIdentity {
        source: source.to_owned(),
        song_key: song_key.to_owned(),
        track,
        voice,
        onset,
    };
    let status = classify_group(group, tuning, max_fret);
    if status != ChordCensusStatus::CompleteExplicit {
        return ChordCensusEvent {
            identity,
            status,
            atom_count: group.len(),
            problem: None,
            observed: None,
        };
    }
    let ids: BTreeSet<usize> = group.iter().map(|note| note.note_id).collect();
    let incoming_techniques = voice_notes
        .iter()
        .enumerate()
        .filter_map(|(origin_id, origin)| {
            let target_atom_id = targets.get(origin_id).copied().flatten()?;
            if !ids.contains(&target_atom_id) || origin.onset >= onset {
                return None;
            }
            Some(IncomingTechnique {
                kind: origin.technique?,
                origin_note_id: origin.note_id,
                origin_onset: origin.onset,
                origin_pitch: origin.pitch,
                origin_position: origin.position?,
                target_atom_id,
            })
        })
        .collect();
    let atoms = group
        .iter()
        .map(|note| ChordEventAtom {
            note_id: note.note_id,
            pitch: note.pitch,
            duration: note.duration,
            tapped: note.tapped,
        })
        .collect();
    let observed_positions = group
        .iter()
        .filter_map(|note| {
            note.position
                .map(|position| ObservedAtomPosition::new(note.note_id, position))
        })
        .collect();
    let problem = ChordEventProblem::new(
        identity.clone(),
        tuning.clone(),
        max_fret,
        atoms,
        anchor,
        incoming_techniques,
    );
    let observed = ObservedChordVoicing::new(observed_positions);
    match (problem, observed) {
        (Ok(problem), Ok(observed)) => ChordCensusEvent {
            identity,
            status,
            atom_count: group.len(),
            problem: Some(problem),
            observed: Some(observed),
        },
        _ => ChordCensusEvent {
            identity,
            status: ChordCensusStatus::OtherUnsupported,
            atom_count: group.len(),
            problem: None,
            observed: None,
        },
    }
}

fn classify_group(group: &[ImportedNote], tuning: &Tuning, max_fret: u8) -> ChordCensusStatus {
    if group.iter().any(|note| note.position.is_none()) {
        return ChordCensusStatus::IncompletePosition;
    }
    if group.iter().any(|note| {
        note.position
            .is_some_and(|position| tuning.pitch_at(position) != Some(note.pitch))
    }) {
        return ChordCensusStatus::PitchMismatch;
    }
    let mut strings = BTreeSet::new();
    if group
        .iter()
        .filter_map(|note| note.position)
        .any(|position| !strings.insert(position.string))
    {
        return ChordCensusStatus::DuplicateExplicitString;
    }
    if group
        .iter()
        .filter_map(|note| note.position)
        .any(|position| position.fret > max_fret)
    {
        return ChordCensusStatus::BeyondMaxFret;
    }
    ChordCensusStatus::CompleteExplicit
}

/// One evaluation-only imported position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ObservedAtomPosition {
    /// Stable atom id.
    pub atom_id: usize,
    /// Imported position.
    pub position: FretboardPosition,
}

impl ObservedAtomPosition {
    /// Creates one observation.
    #[must_use]
    pub const fn new(atom_id: usize, position: FretboardPosition) -> Self {
        Self { atom_id, position }
    }
}

/// Evaluation object kept outside the solver input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedChordVoicing {
    positions: Vec<ObservedAtomPosition>,
}

impl ObservedChordVoicing {
    /// Builds an observation with unique stable atom ids.
    ///
    /// # Errors
    ///
    /// Refuses duplicate stable atom ids.
    pub fn new(positions: Vec<ObservedAtomPosition>) -> Result<Self, ChordEventError> {
        let mut ids = BTreeSet::new();
        for position in &positions {
            if !ids.insert(position.atom_id) {
                return Err(ChordEventError::DuplicateObservedAtomId(position.atom_id));
            }
        }
        Ok(Self { positions })
    }

    /// Imported positions.
    #[must_use]
    pub fn positions(&self) -> &[ObservedAtomPosition] {
        &self.positions
    }
}

/// One complete assignment in stable atom order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChordAssignment {
    /// `(stable atom id, position)` pairs.
    pub positions: Vec<ObservedAtomPosition>,
}

impl ChordAssignment {
    /// Finds the assigned position of a stable atom.
    #[must_use]
    pub fn position(&self, atom_id: usize) -> Option<FretboardPosition> {
        self.positions
            .iter()
            .find(|position| position.atom_id == atom_id)
            .map(|position| position.position)
    }
}

/// Saturating count with explicit loss-of-exactness state and log count.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AssignmentCount {
    /// Saturating count.
    pub value: u64,
    /// Whether `value` saturated.
    pub saturated: bool,
    /// Natural logarithm accumulated descriptively.
    pub ln: f64,
}

/// Conflicting technique requirements for one stable target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TechniqueConflict {
    /// Target atom.
    pub target_atom_id: usize,
    /// Distinct required strings.
    pub required_strings: Vec<u8>,
}

/// Evaluation of one exact assignment set.
#[derive(Debug, Clone, PartialEq)]
pub struct HumanSetMetrics {
    /// Whether the complete imported assignment belongs to the set.
    pub exact_membership: bool,
    /// Minimum stable-atom agreement fraction.
    pub floor: f64,
    /// Uniform expected agreement fraction.
    pub uniform: f64,
    /// Agreement of the deterministic first assignment.
    pub chosen: f64,
    /// Maximum agreement fraction.
    pub ceiling: f64,
}

/// Exact feasible-set result (R0 or R2).
#[derive(Debug, Clone, PartialEq)]
pub struct FeasibleRegime {
    /// Every admissible assignment in deterministic order.
    pub assignments: Vec<ChordAssignment>,
    /// Explicit count contract.
    pub admissible_count: AssignmentCount,
    /// Typed conflict, when the technique context is contradictory.
    pub conflict: Option<TechniqueConflict>,
    /// Human-set evaluation when supplied and complete.
    pub human: Option<HumanSetMetrics>,
    /// Frozen B0 optimum over this feasible set.
    pub b0: Option<ExactPreferredSet>,
}

/// Exact preferred subset under one primitive or lexicographic score.
#[derive(Debug, Clone, PartialEq)]
pub struct ExactPreferredSet {
    /// Primary optimum value.
    pub optimum: i64,
    /// Optional secondary optimum value.
    pub secondary: Option<i64>,
    /// Assignments attaining the optimum.
    pub assignments: Vec<ChordAssignment>,
    /// Explicit optimum-set count.
    pub optimum_count: AssignmentCount,
    /// Human-set metrics over the preferred set.
    pub human: Option<HumanSetMetrics>,
}

/// Registered R0/R1/R2/R3 analysis.
#[derive(Debug, Clone, PartialEq)]
pub struct ChordRegimeAnalysis {
    /// Chord-only feasible set.
    pub r0: FeasibleRegime,
    /// Anchor-minimum subset of R0.
    pub r1: Option<ExactPreferredSet>,
    /// Technique-conditioned feasible set.
    pub r2: FeasibleRegime,
    /// Anchor-minimum subset of R2.
    pub r3: Option<ExactPreferredSet>,
}

/// One complete legal-string condition for an incoming target atom.
#[derive(Debug, Clone, PartialEq)]
pub struct TechniqueStringControl {
    /// Stable target atom.
    pub target_atom_id: usize,
    /// Conditioned physical string.
    pub string: u8,
    /// Exact admissible count.
    pub admissible_count: AssignmentCount,
    /// Frozen B0 preferred set.
    pub b0: Option<ExactPreferredSet>,
    /// Anchor preferred set when an anchor exists.
    pub anchor: Option<ExactPreferredSet>,
}

/// Typed malformed input and evaluation refusals.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ChordEventError {
    /// Fewer than two atoms.
    #[error("event is not a chord")]
    NotAChord,
    /// Duplicate problem atom.
    #[error("duplicate stable chord atom id {0}")]
    DuplicateAtomId(usize),
    /// Duplicate observed atom.
    #[error("duplicate observed chord atom id {0}")]
    DuplicateObservedAtomId(usize),
    /// Technique target is absent.
    #[error("incoming technique targets missing atom {0}")]
    MissingTechniqueTarget(usize),
    /// Technique origin does not precede the chord.
    #[error("incoming technique origin {origin} does not precede chord {chord}")]
    NonPrecedingTechnique {
        /// Origin onset.
        origin: u32,
        /// Chord onset.
        chord: u32,
    },
    /// Requested track does not exist.
    #[error("score has no track {0}")]
    MissingTrack(usize),
}

/// Computes all four exact regimes.
///
/// # Errors
///
/// Reserved for typed representation refusals; a validated problem currently
/// evaluates without additional failure modes.
pub fn analyze_regimes(
    problem: &ChordEventProblem,
    observed: Option<&ObservedChordVoicing>,
) -> Result<ChordRegimeAnalysis, ChordEventError> {
    let r0_assignments = enumerate(problem, &BTreeMap::new());
    let requirements = technique_requirements(problem);
    let (r2_assignments, conflict) = match requirements {
        Ok(requirements) => (enumerate(problem, &requirements), None),
        Err(conflict) => (Vec::new(), Some(conflict)),
    };
    let r0 = feasible_regime(&r0_assignments, None, observed);
    let r1 = problem.preceding_hand.and_then(|anchor| {
        preferred(&r0_assignments, observed, |assignment| {
            anchor_cost(assignment, anchor.fret)
        })
    });
    let r2 = feasible_regime(&r2_assignments, conflict, observed);
    let r3 = problem.preceding_hand.and_then(|anchor| {
        preferred(&r2_assignments, observed, |assignment| {
            anchor_cost(assignment, anchor.fret)
        })
    });
    Ok(ChordRegimeAnalysis { r0, r1, r2, r3 })
}

/// Enumerates the complete legal target-string domain for one stable atom.
///
/// Other incoming requirements remain active; requirements for the selected
/// target are replaced by the conditioned legal string.
///
/// # Errors
///
/// Refuses a target id absent from the problem.
pub fn technique_string_controls(
    problem: &ChordEventProblem,
    target_atom_id: usize,
    observed: Option<&ObservedChordVoicing>,
) -> Result<Vec<TechniqueStringControl>, ChordEventError> {
    let target = problem
        .atoms
        .iter()
        .find(|atom| atom.note_id == target_atom_id)
        .ok_or(ChordEventError::MissingTechniqueTarget(target_atom_id))?;
    let mut strings: Vec<u8> = problem
        .tuning
        .candidates(target.pitch, problem.max_fret)
        .into_iter()
        .map(|position| position.string)
        .collect();
    strings.sort_unstable();
    strings.dedup();
    let mut base_requirements = BTreeMap::new();
    for relation in &problem.incoming_techniques {
        if relation.target_atom_id != target_atom_id {
            base_requirements.insert(relation.target_atom_id, relation.origin_position.string);
        }
    }
    Ok(strings
        .into_iter()
        .map(|string| {
            let mut requirements = base_requirements.clone();
            requirements.insert(target_atom_id, string);
            let assignments = enumerate(problem, &requirements);
            TechniqueStringControl {
                target_atom_id,
                string,
                admissible_count: count(assignments.len()),
                b0: preferred(&assignments, observed, b0_cost),
                anchor: problem.preceding_hand.and_then(|anchor| {
                    preferred(&assignments, observed, |assignment| {
                        anchor_cost(assignment, anchor.fret)
                    })
                }),
            }
        })
        .collect())
}

fn enumerate(
    problem: &ChordEventProblem,
    requirements: &BTreeMap<usize, u8>,
) -> Vec<ChordAssignment> {
    let candidates: Vec<Vec<FretboardPosition>> = problem
        .atoms
        .iter()
        .map(|atom| {
            problem
                .tuning
                .candidates(atom.pitch, problem.max_fret)
                .into_iter()
                .filter(|candidate| {
                    requirements
                        .get(&atom.note_id)
                        .is_none_or(|required| candidate.string == *required)
                })
                .collect()
        })
        .collect();
    if candidates.iter().any(Vec::is_empty) {
        return Vec::new();
    }
    let mut output = Vec::new();
    let mut path = Vec::with_capacity(problem.atoms.len());
    visit(
        problem,
        &candidates,
        0,
        &mut [false; 256],
        &mut path,
        &mut output,
    );
    output
}

#[allow(clippy::too_many_arguments)] // transparent exact-search state
fn visit(
    problem: &ChordEventProblem,
    candidates: &[Vec<FretboardPosition>],
    index: usize,
    used: &mut [bool; 256],
    path: &mut Vec<ObservedAtomPosition>,
    output: &mut Vec<ChordAssignment>,
) {
    if index == candidates.len() {
        output.push(ChordAssignment {
            positions: path.clone(),
        });
        return;
    }
    for &candidate in &candidates[index] {
        let slot = usize::from(candidate.string);
        if used[slot] {
            continue;
        }
        used[slot] = true;
        path.push(ObservedAtomPosition::new(
            problem.atoms[index].note_id,
            candidate,
        ));
        visit(problem, candidates, index + 1, used, path, output);
        path.pop();
        used[slot] = false;
    }
}

fn technique_requirements(
    problem: &ChordEventProblem,
) -> Result<BTreeMap<usize, u8>, TechniqueConflict> {
    let mut requirements = BTreeMap::new();
    for relation in &problem.incoming_techniques {
        let required = relation.origin_position.string;
        if let Some(previous) = requirements.insert(relation.target_atom_id, required) {
            if previous != required {
                let mut required_strings = vec![previous, required];
                required_strings.sort_unstable();
                required_strings.dedup();
                return Err(TechniqueConflict {
                    target_atom_id: relation.target_atom_id,
                    required_strings,
                });
            }
        }
    }
    Ok(requirements)
}

fn feasible_regime(
    assignments: &[ChordAssignment],
    conflict: Option<TechniqueConflict>,
    observed: Option<&ObservedChordVoicing>,
) -> FeasibleRegime {
    FeasibleRegime {
        assignments: assignments.to_vec(),
        admissible_count: count(assignments.len()),
        conflict,
        human: observed.map(|human| human_metrics(assignments, human)),
        b0: preferred(assignments, observed, b0_cost),
    }
}

fn preferred<F>(
    assignments: &[ChordAssignment],
    observed: Option<&ObservedChordVoicing>,
    score: F,
) -> Option<ExactPreferredSet>
where
    F: Fn(&ChordAssignment) -> i64,
{
    let optimum = assignments.iter().map(&score).min()?;
    let selected: Vec<ChordAssignment> = assignments
        .iter()
        .filter(|assignment| score(assignment) == optimum)
        .cloned()
        .collect();
    Some(ExactPreferredSet {
        optimum,
        secondary: None,
        optimum_count: count(selected.len()),
        human: observed.map(|human| human_metrics(&selected, human)),
        assignments: selected,
    })
}

fn b0_cost(assignment: &ChordAssignment) -> i64 {
    let weights = FingeringWeights::v1();
    assignment
        .positions
        .iter()
        .map(|position| v1_unary(position.position.fret, &weights))
        .fold(0_i64, i64::saturating_add)
}

fn anchor_cost(assignment: &ChordAssignment, anchor: u8) -> i64 {
    assignment
        .positions
        .iter()
        .filter(|position| position.position.fret > 0)
        .map(|position| i64::from(position.position.fret.abs_diff(anchor)))
        .fold(0_i64, i64::saturating_add)
}

#[allow(clippy::cast_precision_loss)] // descriptive ratios; exact counts are retained
fn human_metrics(
    assignments: &[ChordAssignment],
    observed: &ObservedChordVoicing,
) -> HumanSetMetrics {
    if assignments.is_empty() || observed.positions.is_empty() {
        return HumanSetMetrics {
            exact_membership: false,
            floor: 0.0,
            uniform: 0.0,
            chosen: 0.0,
            ceiling: 0.0,
        };
    }
    let scores: Vec<usize> = assignments
        .iter()
        .map(|assignment| agreement_count(assignment, observed))
        .collect();
    let denominator = observed.positions.len() as f64;
    let total: usize = scores.iter().sum();
    HumanSetMetrics {
        exact_membership: scores.contains(&observed.positions.len()),
        floor: scores.iter().copied().min().unwrap_or(0) as f64 / denominator,
        uniform: total as f64 / (denominator * assignments.len() as f64),
        chosen: scores.first().copied().unwrap_or(0) as f64 / denominator,
        ceiling: scores.iter().copied().max().unwrap_or(0) as f64 / denominator,
    }
}

fn agreement_count(assignment: &ChordAssignment, observed: &ObservedChordVoicing) -> usize {
    observed
        .positions
        .iter()
        .filter(|expected| assignment.position(expected.atom_id) == Some(expected.position))
        .count()
}

#[allow(clippy::cast_precision_loss)] // descriptive log; exact count is retained
fn count(value: usize) -> AssignmentCount {
    AssignmentCount {
        value: u64::try_from(value).unwrap_or(u64::MAX),
        saturated: u64::try_from(value).is_err(),
        ln: if value == 0 {
            f64::NEG_INFINITY
        } else {
            (value as f64).ln()
        },
    }
}

/// Deterministically rotates anchors by one stable event inside each song.
/// Songs with fewer than two events have no control row.
#[must_use]
pub fn rotate_anchors_within_song(
    rows: &[(ChordEventIdentity, HandAnchor)],
) -> Vec<(ChordEventIdentity, HandAnchor)> {
    type AnchorRow<'a> = &'a (ChordEventIdentity, HandAnchor);
    let mut grouped: BTreeMap<&str, Vec<AnchorRow<'_>>> = BTreeMap::new();
    for row in rows {
        grouped.entry(&row.0.song_key).or_default().push(row);
    }
    let mut output = Vec::new();
    for mut song_rows in grouped.into_values() {
        song_rows.sort_by(|left, right| left.0.cmp(&right.0));
        if song_rows.len() < 2 {
            continue;
        }
        for index in 0..song_rows.len() {
            let event = song_rows[index].0.clone();
            let replacement = song_rows[(index + 1) % song_rows.len()].1;
            output.push((event, replacement));
        }
    }
    output.sort_by(|left, right| left.0.cmp(&right.0));
    output
}
