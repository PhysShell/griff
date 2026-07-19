//! `ExactSemanticDiff` — typed semantic comparison of canonical [`Score`]
//! trees (S16 Phase 4-pre B1).
//!
//! The comparator walks two concrete trees exactly as they are stored: no
//! sorting, no normalization, no fuzzy matching. Every divergence is
//! reported as a [`SemanticDifference`] carrying a **typed**
//! [`SemanticPath`] and a typed [`SemanticDifferenceKind`]; the rendered
//! string form (`Display`) is presentation only — never a selector, never a
//! patch identity, never serialized truth.
//!
//! Positional coordinates (`ordinal`) are deliberate: a diff compares two
//! specific trees, and its paths are valid only for that pair. Stable
//! addressing across edits is Phase 4C's problem, and this module does not
//! pretend to solve it. The `index` / `id` annotations on
//! [`SemanticPathSegment::MasterBar`] / [`SemanticPathSegment::Voice`] are
//! shown only when the two trees agree on the coordinate; when the
//! coordinate field itself differs, the annotation is omitted so
//! `diff(a, b)` and `diff(b, a)` carry identical paths.
//!
//! Traversal order is deterministic depth-first in field-declaration order;
//! for collections, cardinality is compared first and then the common prefix
//! of elements. Every struct is destructured exhaustively (no `..`), so a
//! new canonical field breaks this comparator's compilation instead of
//! silently leaving the contract.
//!
//! ## `NormalizedMusicalDiff` v1 (S16 Phase 4-pre B2)
//!
//! [`normalized_musical_diff`] compares through the ADR-0020
//! [`NormalizedScore`] projection, named as the versioned policy
//! [`NORMALIZED_MUSICAL_POLICY_ID`] / [`NORMALIZED_MUSICAL_POLICY_VERSION`].
//! The projection itself is unchanged; the policy pins what it means today:
//! rests, group kinds, technique-span ranges and evidence, position
//! evidence, repeat markers, and source metadata are not v1 facts, the
//! canonical order erases import order, and the projection's loss labels
//! are deliberately excluded — a musical policy compares sounding facts,
//! not import provenance. Any widening is a `policy_version = 2`, never a
//! silent change of what old reports meant.

use core::fmt;

use crate::{
    dump::{normalize, NormBar, NormNote, NormTrack, NormVoice, NormalizedScore},
    event::NotePosition,
    score::{
        AtomEvent, AtomNote, AtomRest, EventGroup, EventGroupKind, ImportWarning, LossReport,
        MasterBar, Score, SourceMeta, TechniqueSpan, Track, Voice,
    },
};

// ── typed path ────────────────────────────────────────────────────────────────

/// A compared leaf or collection field of the canonical model — a closed
/// enum, so the set of compared facts is a compile-time contract, not a
/// string convention.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(missing_docs)] // each variant names the canonical field it mirrors
pub enum SemanticField {
    TicksPerQuarter,
    MasterBars,
    Index,
    TickRange,
    TimeSignature,
    Tempo,
    Repeat,
    Tracks,
    Name,
    Channel,
    Tuning,
    Voices,
    Id,
    EventGroups,
    Kind,
    Atoms,
    TechniqueSpans,
    AbsoluteStart,
    Duration,
    Pitch,
    Velocity,
    Marks,
    Position,
    Technique,
    Evidence,
    SourceMeta,
    Format,
    LossWarnings,
    TupletNum,
    TupletDen,
    TrackIndex,
    BarIndex,
    NearestMicros,
    Message,
    Ppqn,
    Bars,
    Notes,
    TimeSig,
    StartTick,
    EndTick,
    OnsetTick,
    DurTick,
    NoteString,
    NoteFret,
    Spans,
}

impl SemanticField {
    /// The field's rendered name (presentation only).
    const fn as_str(self) -> &'static str {
        match self {
            Self::TicksPerQuarter => "ticks_per_quarter",
            Self::MasterBars => "master_bars",
            Self::Index => "index",
            Self::TickRange => "tick_range",
            Self::TimeSignature => "time_signature",
            Self::Tempo => "tempo",
            Self::Repeat => "repeat",
            Self::Tracks => "tracks",
            Self::Name => "name",
            Self::Channel => "channel",
            Self::Tuning => "tuning",
            Self::Voices => "voices",
            Self::Id => "id",
            Self::EventGroups => "event_groups",
            Self::Kind => "kind",
            Self::Atoms => "atoms",
            Self::TechniqueSpans => "technique_spans",
            Self::AbsoluteStart => "absolute_start",
            Self::Duration => "duration",
            Self::Pitch => "pitch",
            Self::Velocity => "velocity",
            Self::Marks => "marks",
            Self::Position => "position",
            Self::Technique => "technique",
            Self::Evidence => "evidence",
            Self::SourceMeta => "source_meta",
            Self::Format => "format",
            Self::LossWarnings => "loss.warnings",
            Self::TupletNum => "num",
            Self::TupletDen => "den",
            Self::TrackIndex => "track_index",
            Self::BarIndex => "bar_index",
            Self::NearestMicros => "nearest_micros",
            Self::Message => "message",
            Self::Ppqn => "ppqn",
            Self::Bars => "bars",
            Self::Notes => "notes",
            Self::TimeSig => "time_sig",
            Self::StartTick => "start_tick",
            Self::EndTick => "end_tick",
            Self::OnsetTick => "onset_tick",
            Self::DurTick => "dur_tick",
            Self::NoteString => "string",
            Self::NoteFret => "fret",
            Self::Spans => "spans",
        }
    }
}

/// One step of a [`SemanticPath`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SemanticPathSegment {
    /// The score root.
    Score,
    /// A master bar, by position; `index` annotates the stored
    /// `MasterBar::index` only when both trees agree on it.
    MasterBar {
        /// Zero-based position in `master_bars`.
        ordinal: usize,
        /// The agreed stored bar index; `None` when the index itself
        /// differs, so both diff directions render the same path.
        index: Option<usize>,
    },
    /// A track, by position.
    Track {
        /// Zero-based position in `tracks`.
        ordinal: usize,
    },
    /// A voice, by position; `id` annotates `Voice::id` only when both
    /// trees agree on it.
    Voice {
        /// Zero-based position in `voices`.
        ordinal: usize,
        /// The agreed voice id; `None` when the id itself differs, so both
        /// diff directions render the same path.
        id: Option<u8>,
    },
    /// An event group, by position.
    EventGroup {
        /// Zero-based position in `event_groups`.
        ordinal: usize,
    },
    /// An atom event, by position.
    Atom {
        /// Zero-based position in `atoms`.
        ordinal: usize,
    },
    /// A technique span, by position.
    TechniqueSpan {
        /// Zero-based position in `technique_spans`.
        ordinal: usize,
    },
    /// A loss-report warning, by position.
    LossWarning {
        /// Zero-based position in `loss.warnings`.
        ordinal: usize,
    },
    /// A normalized-projection bar, by position; `index` annotates the
    /// stored bar index only when both projections agree on it.
    Bar {
        /// Zero-based position in the projection's `bars`.
        ordinal: usize,
        /// The agreed stored bar index; `None` when it differs.
        index: Option<usize>,
    },
    /// A normalized-projection note, by position.
    Note {
        /// Zero-based position in the projection's `notes`.
        ordinal: usize,
    },
    /// A named field of the enclosing node.
    Field(SemanticField),
}

impl fmt::Display for SemanticPathSegment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Score => write!(f, "score"),
            Self::MasterBar {
                ordinal,
                index: Some(index),
            } => write!(f, ".master_bars[index={index},ordinal={ordinal}]"),
            Self::MasterBar {
                ordinal,
                index: None,
            } => write!(f, ".master_bars[ordinal={ordinal}]"),
            Self::Track { ordinal } => write!(f, ".tracks[{ordinal}]"),
            Self::Voice {
                ordinal,
                id: Some(id),
            } => write!(f, ".voices[id={id},ordinal={ordinal}]"),
            Self::Voice { ordinal, id: None } => write!(f, ".voices[ordinal={ordinal}]"),
            Self::EventGroup { ordinal } => write!(f, ".event_groups[{ordinal}]"),
            Self::Atom { ordinal } => write!(f, ".atoms[{ordinal}]"),
            Self::TechniqueSpan { ordinal } => write!(f, ".technique_spans[{ordinal}]"),
            Self::LossWarning { ordinal } => write!(f, ".loss.warnings[{ordinal}]"),
            Self::Bar {
                ordinal,
                index: Some(index),
            } => write!(f, ".bars[index={index},ordinal={ordinal}]"),
            Self::Bar {
                ordinal,
                index: None,
            } => write!(f, ".bars[ordinal={ordinal}]"),
            Self::Note { ordinal } => write!(f, ".notes[{ordinal}]"),
            Self::Field(field) => write!(f, ".{}", field.as_str()),
        }
    }
}

/// A typed path from the score root to a compared fact.
///
/// The typed segments are the truth; [`fmt::Display`] renders a
/// deterministic human-readable form such as
/// `score.master_bars[index=4,ordinal=4].tempo` — presentation only.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SemanticPath(Vec<SemanticPathSegment>);

impl SemanticPath {
    /// The path's segments, root first.
    #[must_use]
    pub fn segments(&self) -> &[SemanticPathSegment] {
        &self.0
    }
}

impl fmt::Display for SemanticPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for segment in &self.0 {
            segment.fmt(f)?;
        }
        Ok(())
    }
}

// ── differences and report ────────────────────────────────────────────────────

/// What kind of divergence a [`SemanticDifference`] reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SemanticDifferenceKind {
    /// The same fact carries different values.
    ValueMismatch,
    /// The same position carries different enum variants (e.g. `Single` vs
    /// `Chord`, note vs rest).
    VariantMismatch,
    /// A collection's lengths differ; elements beyond the common prefix are
    /// covered by this one difference.
    CardinalityMismatch {
        /// The expected side's element count.
        expected: usize,
        /// The actual side's element count.
        actual: usize,
    },
    /// The expected side lacks a value the actual side carries.
    MissingExpected,
    /// The actual side lacks a value the expected side carries.
    MissingActual,
}

/// One reported divergence: a typed path, a typed kind, and deterministic
/// diagnostic renderings of the two sides (when a side has a value).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticDifference {
    /// Where in the canonical tree the divergence sits.
    pub path: SemanticPath,
    /// What kind of divergence it is.
    pub kind: SemanticDifferenceKind,
    /// The expected side's value, rendered for diagnostics (not truth).
    pub expected: Option<String>,
    /// The actual side's value, rendered for diagnostics (not truth).
    pub actual: Option<String>,
}

/// Which comparison contract produced a report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SemanticDiffMode {
    /// The exact contract: the tree as stored, every fact compared.
    Exact,
    /// A named, versioned normalization policy (Phase 4-pre B2): the report
    /// is only meaningful under that policy id and version.
    Normalized {
        /// The policy's stable identifier.
        policy_id: &'static str,
        /// The policy's version; any semantic change bumps it.
        policy_version: u16,
    },
}

/// The result of a semantic comparison: the contract it ran under and its
/// divergences in deterministic depth-first order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticDiffReport {
    /// The comparison contract.
    pub mode: SemanticDiffMode,
    /// Divergences in deterministic depth-first order; empty = same.
    pub differences: Vec<SemanticDifference>,
}

impl SemanticDiffReport {
    /// An empty exact report — the two trees are exactly the same.
    #[must_use]
    pub const fn empty_exact() -> Self {
        Self {
            mode: SemanticDiffMode::Exact,
            differences: Vec::new(),
        }
    }

    /// Whether the compared trees had no divergence under the report's mode.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.differences.is_empty()
    }
}

// ── the walker ────────────────────────────────────────────────────────────────

/// Depth-first walker: a segment stack plus the differences found so far.
struct Walker {
    segments: Vec<SemanticPathSegment>,
    differences: Vec<SemanticDifference>,
}

impl Walker {
    fn path(&self) -> SemanticPath {
        SemanticPath(self.segments.clone())
    }

    /// Runs `f` with `segment` pushed onto the path.
    fn scoped(&mut self, segment: SemanticPathSegment, f: impl FnOnce(&mut Self)) {
        self.segments.push(segment);
        f(self);
        self.segments.pop();
    }

    /// Records a difference at the current path.
    fn record(
        &mut self,
        kind: SemanticDifferenceKind,
        expected: Option<String>,
        actual: Option<String>,
    ) {
        self.differences.push(SemanticDifference {
            path: self.path(),
            kind,
            expected,
            actual,
        });
    }

    /// Compares one value-typed leaf under `Field(field)`.
    fn value_leaf<T: PartialEq + fmt::Debug>(&mut self, field: SemanticField, e: &T, a: &T) {
        if e != a {
            self.scoped(SemanticPathSegment::Field(field), |w| {
                w.record(
                    SemanticDifferenceKind::ValueMismatch,
                    Some(format!("{e:?}")),
                    Some(format!("{a:?}")),
                );
            });
        }
    }

    /// Compares one enum-typed leaf under `Field(field)`.
    fn variant_leaf<T: PartialEq + fmt::Debug>(&mut self, field: SemanticField, e: &T, a: &T) {
        if e != a {
            self.scoped(SemanticPathSegment::Field(field), |w| {
                w.record(
                    SemanticDifferenceKind::VariantMismatch,
                    Some(format!("{e:?}")),
                    Some(format!("{a:?}")),
                );
            });
        }
    }

    /// Compares an optional value under `Field(field)`: presence first, then
    /// `on_both` for two present values.
    fn option_leaf<T: fmt::Debug>(
        &mut self,
        field: SemanticField,
        e: Option<&T>,
        a: Option<&T>,
        on_both: impl FnOnce(&mut Self, &T, &T),
    ) {
        self.scoped(SemanticPathSegment::Field(field), |w| match (e, a) {
            (None, None) => {}
            (Some(ev), None) => w.record(
                SemanticDifferenceKind::MissingActual,
                Some(format!("{ev:?}")),
                None,
            ),
            (None, Some(av)) => w.record(
                SemanticDifferenceKind::MissingExpected,
                None,
                Some(format!("{av:?}")),
            ),
            (Some(ev), Some(av)) => on_both(w, ev, av),
        });
    }

    /// Compares a collection: cardinality first (under `Field(field)`), then
    /// the common prefix of elements, each under its own segment. `segment`
    /// builds the element segment from the ordinal and *both* elements, so
    /// coordinate annotations can be restricted to agreed values.
    // Two closures (segment shape + element comparison) plus the slices are
    // what keep every call site declarative; bundling them into a struct
    // would only rename the same six things.
    #[allow(clippy::too_many_arguments)]
    fn collection<T>(
        &mut self,
        field: SemanticField,
        e: &[T],
        a: &[T],
        segment: impl Fn(usize, &T, &T) -> SemanticPathSegment,
        item: impl Fn(&mut Self, &T, &T),
    ) {
        if e.len() != a.len() {
            self.scoped(SemanticPathSegment::Field(field), |w| {
                w.record(
                    SemanticDifferenceKind::CardinalityMismatch {
                        expected: e.len(),
                        actual: a.len(),
                    },
                    None,
                    None,
                );
            });
        }
        for (ordinal, (ev, av)) in e.iter().zip(a.iter()).enumerate() {
            self.scoped(segment(ordinal, ev, av), |w| item(w, ev, av));
        }
    }
}

// ── the exact comparator ──────────────────────────────────────────────────────

/// Compares two canonical scores under the exact contract.
///
/// Segment annotations (`index`, `id`) appear only where both trees agree
/// on the coordinate, so `diff(a, b)` and `diff(b, a)` carry identical
/// paths. The report's differences arrive in deterministic depth-first,
/// field-declaration order.
#[must_use]
pub fn exact_semantic_diff(expected: &Score, actual: &Score) -> SemanticDiffReport {
    if expected == actual {
        return SemanticDiffReport::empty_exact();
    }
    let mut walker = Walker {
        segments: Vec::new(),
        differences: Vec::new(),
    };
    walker.scoped(SemanticPathSegment::Score, |w| {
        diff_score(w, expected, actual);
    });
    SemanticDiffReport {
        mode: SemanticDiffMode::Exact,
        differences: walker.differences,
    }
}

fn diff_score(w: &mut Walker, expected: &Score, actual: &Score) {
    let Score {
        ticks_per_quarter: e_tpq,
        master_bars: e_bars,
        tracks: e_tracks,
        source_meta: e_meta,
        loss: e_loss,
    } = expected;
    let Score {
        ticks_per_quarter: a_tpq,
        master_bars: a_bars,
        tracks: a_tracks,
        source_meta: a_meta,
        loss: a_loss,
    } = actual;

    w.value_leaf(SemanticField::TicksPerQuarter, e_tpq, a_tpq);
    w.collection(
        SemanticField::MasterBars,
        e_bars,
        a_bars,
        |ordinal, e_bar, a_bar| SemanticPathSegment::MasterBar {
            ordinal,
            index: (e_bar.index == a_bar.index).then_some(e_bar.index),
        },
        diff_master_bar,
    );
    w.collection(
        SemanticField::Tracks,
        e_tracks,
        a_tracks,
        |ordinal, _, _| SemanticPathSegment::Track { ordinal },
        diff_track,
    );
    w.option_leaf(
        SemanticField::SourceMeta,
        e_meta.as_ref(),
        a_meta.as_ref(),
        |w, e, a| {
            let SourceMeta { format: e_format } = e;
            let SourceMeta { format: a_format } = a;
            w.option_leaf(
                SemanticField::Format,
                e_format.as_ref(),
                a_format.as_ref(),
                |inner, e_value, a_value| {
                    if e_value != a_value {
                        inner.record(
                            SemanticDifferenceKind::ValueMismatch,
                            Some(format!("{e_value:?}")),
                            Some(format!("{a_value:?}")),
                        );
                    }
                },
            );
        },
    );
    let LossReport {
        warnings: e_warnings,
    } = e_loss;
    let LossReport {
        warnings: a_warnings,
    } = a_loss;
    w.collection(
        SemanticField::LossWarnings,
        e_warnings,
        a_warnings,
        |ordinal, _, _| SemanticPathSegment::LossWarning { ordinal },
        diff_import_warning,
    );
}

/// Compares two loss warnings variant- and payload-aware: a different
/// variant is a `VariantMismatch` on `.kind`; a shared variant compares its
/// payload fields individually. The outer match is exhaustive over the
/// expected variant with no wildcard, so a new `ImportWarning` variant
/// breaks this comparator's compilation.
fn diff_import_warning(w: &mut Walker, expected: &ImportWarning, actual: &ImportWarning) {
    let variant_mismatch = |walker: &mut Walker| {
        walker.scoped(SemanticPathSegment::Field(SemanticField::Kind), |inner| {
            inner.record(
                SemanticDifferenceKind::VariantMismatch,
                Some(format!("{expected:?}")),
                Some(format!("{actual:?}")),
            );
        });
    };
    match expected {
        ImportWarning::TrackNameInvalidUtf8 {
            track_index: e_track_index,
        } => {
            if let ImportWarning::TrackNameInvalidUtf8 {
                track_index: a_track_index,
            } = actual
            {
                w.value_leaf(SemanticField::TrackIndex, e_track_index, a_track_index);
            } else {
                variant_mismatch(w);
            }
        }
        ImportWarning::SmpteTimingUnsupported => {
            if !matches!(actual, ImportWarning::SmpteTimingUnsupported) {
                variant_mismatch(w);
            }
        }
        ImportWarning::TempoApproximated {
            bar_index: e_bar_index,
            nearest_micros: e_nearest_micros,
        } => {
            if let ImportWarning::TempoApproximated {
                bar_index: a_bar_index,
                nearest_micros: a_nearest_micros,
            } = actual
            {
                w.value_leaf(SemanticField::BarIndex, e_bar_index, a_bar_index);
                w.value_leaf(
                    SemanticField::NearestMicros,
                    e_nearest_micros,
                    a_nearest_micros,
                );
            } else {
                variant_mismatch(w);
            }
        }
        ImportWarning::Other(e_message) => {
            if let ImportWarning::Other(a_message) = actual {
                w.value_leaf(SemanticField::Message, e_message, a_message);
            } else {
                variant_mismatch(w);
            }
        }
    }
}

fn diff_master_bar(w: &mut Walker, expected: &MasterBar, actual: &MasterBar) {
    let MasterBar {
        index: e_index,
        tick_range: e_range,
        time_signature: e_sig,
        tempo: e_tempo,
        repeat: e_repeat,
    } = expected;
    let MasterBar {
        index: a_index,
        tick_range: a_range,
        time_signature: a_sig,
        tempo: a_tempo,
        repeat: a_repeat,
    } = actual;
    w.value_leaf(SemanticField::Index, e_index, a_index);
    w.value_leaf(SemanticField::TickRange, e_range, a_range);
    w.value_leaf(SemanticField::TimeSignature, e_sig, a_sig);
    w.value_leaf(SemanticField::Tempo, e_tempo, a_tempo);
    w.value_leaf(SemanticField::Repeat, e_repeat, a_repeat);
}

fn diff_track(w: &mut Walker, expected: &Track, actual: &Track) {
    let Track {
        name: e_name,
        channel: e_channel,
        voices: e_voices,
        tuning: e_tuning,
    } = expected;
    let Track {
        name: a_name,
        channel: a_channel,
        voices: a_voices,
        tuning: a_tuning,
    } = actual;
    w.option_leaf(
        SemanticField::Name,
        e_name.as_ref(),
        a_name.as_ref(),
        |w, e, a| {
            if e != a {
                w.record(
                    SemanticDifferenceKind::ValueMismatch,
                    Some(format!("{e:?}")),
                    Some(format!("{a:?}")),
                );
            }
        },
    );
    w.value_leaf(SemanticField::Channel, e_channel, a_channel);
    w.collection(
        SemanticField::Voices,
        e_voices,
        a_voices,
        |ordinal, e_voice, a_voice| SemanticPathSegment::Voice {
            ordinal,
            id: (e_voice.id == a_voice.id).then_some(e_voice.id),
        },
        diff_voice,
    );
    w.value_leaf(SemanticField::Tuning, e_tuning, a_tuning);
}

fn diff_voice(w: &mut Walker, expected: &Voice, actual: &Voice) {
    let Voice {
        id: e_id,
        event_groups: e_groups,
    } = expected;
    let Voice {
        id: a_id,
        event_groups: a_groups,
    } = actual;
    w.value_leaf(SemanticField::Id, e_id, a_id);
    w.collection(
        SemanticField::EventGroups,
        e_groups,
        a_groups,
        |ordinal, _, _| SemanticPathSegment::EventGroup { ordinal },
        diff_group,
    );
}

fn diff_group(w: &mut Walker, expected: &EventGroup, actual: &EventGroup) {
    let EventGroup {
        kind: e_kind,
        atoms: e_atoms,
        technique_spans: e_spans,
    } = expected;
    let EventGroup {
        kind: a_kind,
        atoms: a_atoms,
        technique_spans: a_spans,
    } = actual;
    diff_event_group_kind(w, *e_kind, *a_kind);
    w.collection(
        SemanticField::Atoms,
        e_atoms,
        a_atoms,
        |ordinal, _, _| SemanticPathSegment::Atom { ordinal },
        diff_atom,
    );
    w.collection(
        SemanticField::TechniqueSpans,
        e_spans,
        a_spans,
        |ordinal, _, _| SemanticPathSegment::TechniqueSpan { ordinal },
        diff_span,
    );
}

/// Compares two group kinds payload-aware: different variants are a
/// `VariantMismatch` on `.kind`; two tuplets compare `num` / `den` as
/// individual `ValueMismatch`es on `.kind.num` / `.kind.den`. The outer
/// match is exhaustive over the expected variant with no wildcard, so a new
/// `EventGroupKind` variant breaks this comparator's compilation.
fn diff_event_group_kind(w: &mut Walker, expected: EventGroupKind, actual: EventGroupKind) {
    let variant_mismatch = |walker: &mut Walker| {
        walker.scoped(SemanticPathSegment::Field(SemanticField::Kind), |inner| {
            inner.record(
                SemanticDifferenceKind::VariantMismatch,
                Some(format!("{expected:?}")),
                Some(format!("{actual:?}")),
            );
        });
    };
    match expected {
        EventGroupKind::Single => {
            if !matches!(actual, EventGroupKind::Single) {
                variant_mismatch(w);
            }
        }
        EventGroupKind::Chord => {
            if !matches!(actual, EventGroupKind::Chord) {
                variant_mismatch(w);
            }
        }
        EventGroupKind::Arpeggio => {
            if !matches!(actual, EventGroupKind::Arpeggio) {
                variant_mismatch(w);
            }
        }
        EventGroupKind::Strum => {
            if !matches!(actual, EventGroupKind::Strum) {
                variant_mismatch(w);
            }
        }
        EventGroupKind::Tuplet {
            num: e_num,
            den: e_den,
        } => {
            if let EventGroupKind::Tuplet {
                num: a_num,
                den: a_den,
            } = actual
            {
                w.scoped(SemanticPathSegment::Field(SemanticField::Kind), |inner| {
                    inner.value_leaf(SemanticField::TupletNum, &e_num, &a_num);
                    inner.value_leaf(SemanticField::TupletDen, &e_den, &a_den);
                });
            } else {
                variant_mismatch(w);
            }
        }
        EventGroupKind::Grace => {
            if !matches!(actual, EventGroupKind::Grace) {
                variant_mismatch(w);
            }
        }
    }
}

fn diff_atom(w: &mut Walker, expected: &AtomEvent, actual: &AtomEvent) {
    match (expected, actual) {
        (AtomEvent::Note(e), AtomEvent::Note(a)) => diff_note(w, e, a),
        (AtomEvent::Rest(e), AtomEvent::Rest(a)) => diff_rest(w, *e, *a),
        (AtomEvent::Note(_), AtomEvent::Rest(_)) => {
            w.scoped(SemanticPathSegment::Field(SemanticField::Kind), |w| {
                w.record(
                    SemanticDifferenceKind::VariantMismatch,
                    Some("Note".to_owned()),
                    Some("Rest".to_owned()),
                );
            });
        }
        (AtomEvent::Rest(_), AtomEvent::Note(_)) => {
            w.scoped(SemanticPathSegment::Field(SemanticField::Kind), |w| {
                w.record(
                    SemanticDifferenceKind::VariantMismatch,
                    Some("Rest".to_owned()),
                    Some("Note".to_owned()),
                );
            });
        }
    }
}

fn diff_note(w: &mut Walker, expected: &AtomNote, actual: &AtomNote) {
    let AtomNote {
        absolute_start: e_start,
        duration: e_duration,
        pitch: e_pitch,
        velocity: e_velocity,
        marks: e_marks,
        position: e_position,
    } = expected;
    let AtomNote {
        absolute_start: a_start,
        duration: a_duration,
        pitch: a_pitch,
        velocity: a_velocity,
        marks: a_marks,
        position: a_position,
    } = actual;
    w.value_leaf(SemanticField::AbsoluteStart, e_start, a_start);
    w.value_leaf(SemanticField::Duration, e_duration, a_duration);
    w.value_leaf(SemanticField::Pitch, e_pitch, a_pitch);
    w.value_leaf(SemanticField::Velocity, e_velocity, a_velocity);
    w.value_leaf(SemanticField::Marks, e_marks, a_marks);
    w.option_leaf(
        SemanticField::Position,
        e_position.as_ref(),
        a_position.as_ref(),
        |w, e, a| {
            let NotePosition {
                position: e_fretboard,
                evidence: e_evidence,
            } = e;
            let NotePosition {
                position: a_fretboard,
                evidence: a_evidence,
            } = a;
            if e_fretboard != a_fretboard {
                w.record(
                    SemanticDifferenceKind::ValueMismatch,
                    Some(format!("{e_fretboard:?}")),
                    Some(format!("{a_fretboard:?}")),
                );
            }
            w.value_leaf(SemanticField::Evidence, e_evidence, a_evidence);
        },
    );
}

fn diff_rest(w: &mut Walker, expected: AtomRest, actual: AtomRest) {
    let AtomRest {
        absolute_start: e_start,
        duration: e_duration,
    } = expected;
    let AtomRest {
        absolute_start: a_start,
        duration: a_duration,
    } = actual;
    w.value_leaf(SemanticField::AbsoluteStart, &e_start, &a_start);
    w.value_leaf(SemanticField::Duration, &e_duration, &a_duration);
}

fn diff_span(w: &mut Walker, expected: &TechniqueSpan, actual: &TechniqueSpan) {
    let TechniqueSpan {
        technique: e_technique,
        tick_range: e_range,
        evidence: e_evidence,
    } = expected;
    let TechniqueSpan {
        technique: a_technique,
        tick_range: a_range,
        evidence: a_evidence,
    } = actual;
    w.variant_leaf(SemanticField::Technique, e_technique, a_technique);
    w.value_leaf(SemanticField::TickRange, e_range, a_range);
    w.value_leaf(SemanticField::Evidence, e_evidence, a_evidence);
}

// ── the normalized-musical comparator (policy v1) ─────────────────────────────

/// The v1 normalized-musical policy's stable identifier (ADR-0020).
pub const NORMALIZED_MUSICAL_POLICY_ID: &str = "adr-0020-normalized-musical";

/// The v1 normalized-musical policy's version. Any semantic widening of the
/// policy bumps this; it never silently changes what old reports meant.
pub const NORMALIZED_MUSICAL_POLICY_VERSION: u16 = 1;

/// Compares two canonical scores under the v1 normalized-musical policy:
/// `Score` → ADR-0020 [`NormalizedScore`] → typed diff of the projections.
///
/// See the module docs for what v1 deliberately does not compare. Paths
/// speak the projection's own shape (`bars` / `notes`).
#[must_use]
pub fn normalized_musical_diff(expected: &Score, actual: &Score) -> SemanticDiffReport {
    let mode = SemanticDiffMode::Normalized {
        policy_id: NORMALIZED_MUSICAL_POLICY_ID,
        policy_version: NORMALIZED_MUSICAL_POLICY_VERSION,
    };
    let e = normalize(expected);
    let a = normalize(actual);
    let mut walker = Walker {
        segments: Vec::new(),
        differences: Vec::new(),
    };
    walker.scoped(SemanticPathSegment::Score, |w| {
        diff_normalized_score(w, &e, &a);
    });
    SemanticDiffReport {
        mode,
        differences: walker.differences,
    }
}

// `loss: _` is deliberate over `..`: the field is named so a projection-shape
// change still breaks this comparator's compilation, while the value is
// explicitly excluded from the v1 policy.
#[allow(clippy::unneeded_field_pattern)]
fn diff_normalized_score(w: &mut Walker, expected: &NormalizedScore, actual: &NormalizedScore) {
    let NormalizedScore {
        ppqn: e_ppqn,
        // Deliberately excluded from v1: loss labels are import provenance,
        // not sounding music. Bound (not `..`) so a projection-shape change
        // still breaks this comparator's compilation.
        loss: _,
        tracks: e_tracks,
    } = expected;
    let NormalizedScore {
        ppqn: a_ppqn,
        loss: _,
        tracks: a_tracks,
    } = actual;
    w.value_leaf(SemanticField::Ppqn, e_ppqn, a_ppqn);
    w.collection(
        SemanticField::Tracks,
        e_tracks,
        a_tracks,
        |ordinal, _, _| SemanticPathSegment::Track { ordinal },
        diff_norm_track,
    );
}

fn diff_norm_track(w: &mut Walker, expected: &NormTrack, actual: &NormTrack) {
    let NormTrack {
        name: e_name,
        channel: e_channel,
        tuning: e_tuning,
        bars: e_bars,
    } = expected;
    let NormTrack {
        name: a_name,
        channel: a_channel,
        tuning: a_tuning,
        bars: a_bars,
    } = actual;
    w.option_leaf(
        SemanticField::Name,
        e_name.as_ref(),
        a_name.as_ref(),
        |inner, e_value, a_value| {
            if e_value != a_value {
                inner.record(
                    SemanticDifferenceKind::ValueMismatch,
                    Some(format!("{e_value:?}")),
                    Some(format!("{a_value:?}")),
                );
            }
        },
    );
    w.value_leaf(SemanticField::Channel, e_channel, a_channel);
    w.value_leaf(SemanticField::Tuning, e_tuning, a_tuning);
    w.collection(
        SemanticField::Bars,
        e_bars,
        a_bars,
        |ordinal, e_bar, a_bar| SemanticPathSegment::Bar {
            ordinal,
            index: (e_bar.index == a_bar.index).then_some(e_bar.index),
        },
        diff_norm_bar,
    );
}

fn diff_norm_bar(w: &mut Walker, expected: &NormBar, actual: &NormBar) {
    let NormBar {
        index: e_index,
        time_sig: e_time_sig,
        tempo: e_tempo,
        start_tick: e_start_tick,
        end_tick: e_end_tick,
        voices: e_voices,
    } = expected;
    let NormBar {
        index: a_index,
        time_sig: a_time_sig,
        tempo: a_tempo,
        start_tick: a_start_tick,
        end_tick: a_end_tick,
        voices: a_voices,
    } = actual;
    w.value_leaf(SemanticField::Index, e_index, a_index);
    w.value_leaf(SemanticField::TimeSig, e_time_sig, a_time_sig);
    w.value_leaf(SemanticField::Tempo, e_tempo, a_tempo);
    w.value_leaf(SemanticField::StartTick, e_start_tick, a_start_tick);
    w.value_leaf(SemanticField::EndTick, e_end_tick, a_end_tick);
    w.collection(
        SemanticField::Voices,
        e_voices,
        a_voices,
        |ordinal, e_voice, a_voice| SemanticPathSegment::Voice {
            ordinal,
            id: (e_voice.id == a_voice.id).then_some(e_voice.id),
        },
        diff_norm_voice,
    );
}

fn diff_norm_voice(w: &mut Walker, expected: &NormVoice, actual: &NormVoice) {
    let NormVoice {
        id: e_id,
        notes: e_notes,
    } = expected;
    let NormVoice {
        id: a_id,
        notes: a_notes,
    } = actual;
    w.value_leaf(SemanticField::Id, e_id, a_id);
    w.collection(
        SemanticField::Notes,
        e_notes,
        a_notes,
        |ordinal, _, _| SemanticPathSegment::Note { ordinal },
        diff_norm_note,
    );
}

fn diff_norm_note(w: &mut Walker, expected: &NormNote, actual: &NormNote) {
    let NormNote {
        onset_tick: e_onset_tick,
        dur_tick: e_dur_tick,
        pitch: e_pitch,
        velocity: e_velocity,
        string: e_string,
        fret: e_fret,
        marks: e_marks,
        spans: e_spans,
    } = expected;
    let NormNote {
        onset_tick: a_onset_tick,
        dur_tick: a_dur_tick,
        pitch: a_pitch,
        velocity: a_velocity,
        string: a_string,
        fret: a_fret,
        marks: a_marks,
        spans: a_spans,
    } = actual;
    w.value_leaf(SemanticField::OnsetTick, e_onset_tick, a_onset_tick);
    w.value_leaf(SemanticField::DurTick, e_dur_tick, a_dur_tick);
    w.value_leaf(SemanticField::Pitch, e_pitch, a_pitch);
    w.value_leaf(SemanticField::Velocity, e_velocity, a_velocity);
    w.option_leaf(
        SemanticField::NoteString,
        e_string.as_ref(),
        a_string.as_ref(),
        |inner, e_value, a_value| {
            if e_value != a_value {
                inner.record(
                    SemanticDifferenceKind::ValueMismatch,
                    Some(format!("{e_value:?}")),
                    Some(format!("{a_value:?}")),
                );
            }
        },
    );
    w.option_leaf(
        SemanticField::NoteFret,
        e_fret.as_ref(),
        a_fret.as_ref(),
        |inner, e_value, a_value| {
            if e_value != a_value {
                inner.record(
                    SemanticDifferenceKind::ValueMismatch,
                    Some(format!("{e_value:?}")),
                    Some(format!("{a_value:?}")),
                );
            }
        },
    );
    w.value_leaf(SemanticField::Marks, e_marks, a_marks);
    w.value_leaf(SemanticField::Spans, e_spans, a_spans);
}
