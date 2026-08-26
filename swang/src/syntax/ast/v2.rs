//! The level-2 exact-score AST — a transient syntax form (SWG-4A-02).
//!
//! ```text
//! level-2 text  ->  ExactScoreDocument  ->  ScoreBuilder  ->  griff_core::Score
//!                       (here)              (SWG-4A-09)
//! ```
//!
//! This module is the middle box and nothing else. It holds no parser, no
//! builder, no formatter, and no conversion in either direction: it is the
//! shape a parsed exact score has on its way to becoming a `Score`, and it
//! is expected to be short-lived in every program that constructs one.
//!
//! # Raw shapes, on purpose
//!
//! Nothing here is a canonical type. A note's pitch is a `u8`, not a
//! [`Pitch`]; a bar's meter is two `u8`s, not a `TimeSignature`. That is not
//! laziness — it is where the boundary goes. The canonical newtypes have
//! checked constructors, and reaching for them here would move every
//! refusal into a struct definition: `pitch 200` would become unbuildable,
//! and the diagnostic that should name it (4A-07's, or 4A-09's) would have
//! nowhere left to be raised from. So the document can hold `ppqn 0`,
//! `meter 0/3`, `ticks 100..10`, and `velocity 255`. None of that says such
//! a text will be accepted. It says this task did not decide.
//!
//! The converse also holds, and matters more. Everything
//! `docs/swang/exact-score-text.md` §3 lists as *inhabited* canonical state
//! — `channel 200`, duplicate `Voice.id`, `tuplet 0/0`, a zero-duration
//! rest, a string beyond the tuning — must be representable, or the exact
//! writer's own output would have no syntax form to be parsed back into.
//!
//! # Named slots, not a concrete syntax tree
//!
//! The fields are typed and named rather than a `Vec` of "whatever word came
//! next", because §6.2 already settled what is semantic:
//!
//! - the order *between* slots belongs to the formatter — so nothing here
//!   remembers that an author wrote `track` above `master_bar`;
//! - the order *within* one repeated slot belongs to the music — so
//!   `master_bars`, `tracks`, `voices`, `groups`, `atoms`, `spans`, `tuning`
//!   and `loss` are all `Vec`, never a set, and nothing sorts or dedups;
//! - `note` and `rest` are variant tags at one position of one sequence, so
//!   [`ExactAtom`] is a sum type and there is no `rests` field to interleave
//!   wrongly with;
//! - the four warning variants are likewise one sequence, not four.
//!
//! Recording the author's word order would make this a concrete syntax tree
//! and would preserve information the spec has declared meaningless. The one
//! place that reasoning is deliberately *not* applied is [`ExactNote::marks`]
//! — see its own note.
//!
//! # No second format
//!
//! There is no `Serialize`, no `Hash`, no JSON schema, no artifact. The exact
//! score text **is** this document's serialization; a second one would be a
//! compatibility promise nobody agreed to freeze.
//!
//! # Why this module allows dead code
//!
//! Nothing outside its own tests constructs one of these types yet, and that
//! is the task's shape rather than an oversight: 4A-02 builds the form,
//! 4A-06..08 fill it from text, and 4A-09 lowers it. Wiring it into a caller
//! now — an evaluator that takes one, a public constructor — is precisely
//! what the boundary tests in `swang/tests/exact_document_boundary.rs`
//! forbid, because that is how a transient form becomes a second model. So
//! the allow stays until a parser exists to remove the need for it.
//!
//! [`Pitch`]: griff_core::event::Pitch
#![allow(dead_code)]

/// One whole level-2 exact score, as the grammar spells it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExactScoreDocument {
    /// `ppqn <n>` — the tick resolution, unvalidated.
    pub(crate) ppqn: u16,
    /// `master_bar { … }` blocks, in the order the score carries them.
    pub(crate) master_bars: Vec<ExactMasterBar>,
    /// `track { … }` blocks, in order.
    pub(crate) tracks: Vec<ExactTrack>,
    /// The `source` block: absent, or present and possibly formatless. The
    /// three states are three values (§2.1).
    pub(crate) source: Option<ExactSource>,
    /// The `loss` block's warnings, in order, duplicates included. An empty
    /// vector is a clean report, which is the state that omits the block.
    pub(crate) loss: Vec<ExactWarning>,
}

/// One `master_bar` block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExactMasterBar {
    /// The **stored** index, not a position (H4), and `u64` wide because
    /// SWG-CORE-01 closed H3 there.
    pub(crate) index: u64,
    /// `ticks <start>..<end>`.
    pub(crate) ticks: ExactTickRange,
    /// `meter <numerator>/<denominator>`.
    pub(crate) meter: ExactMeter,
    /// `tempo <numerator>/<denominator>`, the reduced rational as written.
    pub(crate) tempo: ExactTempo,
    /// The `repeat` block, when the text spells one. `None` is a bar with no
    /// repeat block at all, which is not the same document as one carrying a
    /// repeat whose fields happen to hold the model's default.
    pub(crate) repeat: Option<ExactRepeat>,
}

/// `<start>..<end>` — no ordering claim; `100..10` is representable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ExactTickRange {
    /// First tick.
    pub(crate) start: u32,
    /// End tick, exclusive.
    pub(crate) end: u32,
}

/// `<numerator>/<denominator>` — no power-of-two or nonzero claim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ExactMeter {
    /// Beats per bar.
    pub(crate) numerator: u8,
    /// Beat unit.
    pub(crate) denominator: u8,
}

/// `<numerator>/<denominator>` beats per minute, as the reduced rational.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ExactTempo {
    /// BPM numerator.
    pub(crate) numerator: u32,
    /// BPM denominator; `1` for an integer BPM.
    pub(crate) denominator: u32,
}

/// `repeat { start <bool> play_count <n> }`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ExactRepeat {
    /// This bar opens a repeated section.
    pub(crate) start: bool,
    /// Times the section closing here is played.
    pub(crate) play_count: u8,
}

/// One `track` block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExactTrack {
    /// `name "…"`, absent when the block carries no name word. An absent
    /// name and an empty one are two documents (§2.1).
    pub(crate) name: Option<String>,
    /// `channel <n>` — unvalidated; `200` is inhabited canonical state.
    pub(crate) channel: u8,
    /// `tuning [p …]` — raw pitch numbers in written order. Empty is `[]`,
    /// a required word with no elements, not an absent tuning.
    pub(crate) tuning: Vec<u8>,
    /// `voice { … }` blocks, in order. Duplicate ids are not merged.
    pub(crate) voices: Vec<ExactVoice>,
}

/// One `voice` block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExactVoice {
    /// `id <n>`.
    pub(crate) id: u8,
    /// `group … { … }` blocks, in order.
    pub(crate) groups: Vec<ExactGroup>,
}

/// One `group` block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExactGroup {
    /// The word after `group`, with `tuplet`'s payload opened.
    pub(crate) kind: ExactGroupKind,
    /// `note` and `rest` in one sequence — one slot, two variant tags
    /// (§6.2 rule 3).
    pub(crate) atoms: Vec<ExactAtom>,
    /// `span … { … }` blocks, in order. A separate slot from `atoms`, and
    /// the formatter's rule that spans follow atoms is the formatter's.
    pub(crate) spans: Vec<ExactSpan>,
}

/// The six `group` words (§6.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExactGroupKind {
    /// `group single`.
    Single,
    /// `group chord`.
    Chord,
    /// `group arpeggio`.
    Arpeggio,
    /// `group strum`.
    Strum,
    /// `group tuplet <num>/<den>` — `0/0` is representable.
    Tuplet {
        /// Note count within the tuplet.
        num: u8,
        /// Grid subdivision it fits into.
        den: u8,
    },
    /// `group grace`.
    Grace,
}

/// One element of a group's atom slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ExactAtom {
    /// `note { … }`.
    Note(ExactNote),
    /// `rest { … }` — never an absence (§2.5).
    Rest(ExactRest),
}

/// `note { at … duration … pitch … velocity … marks [ … ] position? }`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExactNote {
    /// `at <tick>`.
    pub(crate) at: u32,
    /// `duration <ticks>` — zero is representable.
    pub(crate) duration: u32,
    /// `pitch <n>` — unvalidated; the 7-bit check is 4A-09's.
    pub(crate) pitch: u8,
    /// `velocity <n>` — likewise.
    pub(crate) velocity: u8,
    /// `marks [ … ]`, **as written**.
    ///
    /// The canonical model's `NoteMarks` is a set whose order is
    /// `NoteMark::ALL`, so this is the one place the "order within a slot is
    /// semantic" rule does not apply — and a sequence is still the right
    /// shape. A set here would silently accept `[tap accent]` and hand back
    /// `[accent tap]`, and silently swallow a repeat: normalization
    /// performed by a struct definition, with no diagnostic and no author.
    /// Keeping what the text spelled leaves the refusal to whoever should
    /// make it.
    pub(crate) marks: Vec<ExactNoteMark>,
    /// The `position` block, when the note carries one.
    pub(crate) position: Option<ExactPosition>,
}

/// `rest { at … duration … }`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ExactRest {
    /// `at <tick>`.
    pub(crate) at: u32,
    /// `duration <ticks>` — zero is representable.
    pub(crate) duration: u32,
}

/// The seven `marks` words (§6.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExactNoteMark {
    /// `accent`.
    Accent,
    /// `ghost`.
    Ghost,
    /// `staccato`.
    Staccato,
    /// `dead_note`.
    DeadNote,
    /// `harmonic_natural`.
    HarmonicNatural,
    /// `harmonic_pinch`.
    HarmonicPinch,
    /// `tap`.
    Tap,
}

/// `position { string … fret … evidence { … } }`.
///
/// No claim that `string` fits the track's tuning: §3 lists that as
/// inhabited, and this is not a fretboard validator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ExactPosition {
    /// `string <n>`, 1-indexed in the canonical model.
    pub(crate) string: u8,
    /// `fret <n>`.
    pub(crate) fret: u8,
    /// The position's own evidence.
    pub(crate) evidence: ExactEvidence,
}

/// `span <technique> { ticks … evidence { … } }`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ExactSpan {
    /// The word after `span`.
    pub(crate) technique: ExactSpanTechnique,
    /// `ticks <start>..<end>`, with no claim about the enclosing group.
    pub(crate) ticks: ExactTickRange,
    /// The span's evidence.
    pub(crate) evidence: ExactEvidence,
}

/// The eight `span` words (§6.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExactSpanTechnique {
    /// `slide`.
    Slide,
    /// `bend`.
    Bend,
    /// `legato`.
    Legato,
    /// `palm_mute`.
    PalmMute,
    /// `hammer_on`.
    HammerOn,
    /// `pull_off`.
    PullOff,
    /// `vibrato`.
    Vibrato,
    /// `let_ring`.
    LetRing,
}

/// `evidence { source <src> confidence <bps> }`.
///
/// The two fields are independent facts and are never tidied into a pair
/// that looks more sensible (§2.6): explicit at 0 and inferred at 10 000 are
/// both representable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ExactEvidence {
    /// `source explicit` or `source inferred_from_midi`.
    pub(crate) source: ExactEvidenceSource,
    /// `confidence <bps>` — a raw basis-point number. `ConfidenceBps` keeps
    /// its field private and clamps; that check belongs to lowering, so the
    /// syntax form holds what was written.
    pub(crate) confidence: u16,
}

/// The two `source` words inside an `evidence` block (§6.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExactEvidenceSource {
    /// `explicit`.
    Explicit,
    /// `inferred_from_midi`.
    InferredFromMidi,
}

/// The `source` block's contents.
///
/// The block's own presence is the enclosing `Option`; this is what is
/// inside it, so `source { }` and `source { format "" }` stay distinct.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExactSource {
    /// `format "…"`, absent when the block is empty.
    pub(crate) format: Option<String>,
}

/// One element of the `loss` block (§2.8, §6.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ExactWarning {
    /// `track_name_invalid_utf8 { track_index <n> }`.
    TrackNameInvalidUtf8 {
        /// Zero-based track index; `u64` since SWG-CORE-01.
        track_index: u64,
    },
    /// `smpte_timing_unsupported` — a bare word, no block.
    SmpteTimingUnsupported,
    /// `tempo_approximated { bar_index <n> nearest_micros <n> }`.
    TempoApproximated {
        /// Zero-based master-bar index; `u64` since SWG-CORE-01.
        bar_index: u64,
        /// Microseconds per quarter actually written.
        nearest_micros: u32,
    },
    /// `other { message "…" }`, whose message is unrestricted.
    Other(String),
}
