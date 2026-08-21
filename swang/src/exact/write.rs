//! Canonical level-2 text from a canonical `Score` — the whole document.
//!
//! Built in slices — transport (SWG-4A-03), musical structure (SWG-4A-04),
//! and the leaves and metadata (SWG-4A-05) — and complete since the last of
//! them. There is no longer a part of the canonical tree this writer declines
//! to spell: every inhabited `Score` either writes, or is refused for
//! violating an invariant `griff-core` itself declares.
//!
//! The census (`docs/swang/exact-score-text.md`) decided every spelling
//! here; this module only obeys it. Several of those decisions are
//! load-bearing and easy to undo by accident, so each is marked at its site:
//!
//! - `MasterBar.index` is the **stored** value, never the vector position;
//! - `master_bars` keeps **vector order** and is never sorted — and so do
//!   `tracks`, `voices`, `event_groups`, and `atoms`;
//! - `Tempo` is written from its own reduced rational, including `7/1` and
//!   `100/7`, never reconstructed from a constructor's provenance;
//! - `repeat` is omitted for `RepeatMarker::default()` and written
//!   literally for every other combination;
//! - `note` and `rest` are variant tags inside one slot, so they are never
//!   regrouped;
//! - `tuning` and `marks` are scalar list values, so their empty form is
//!   `[]` and never an absent word;
//! - `marks` is a **set**, so its order is `NoteMark::ALL` — not the order
//!   bits were set, and not alphabetical;
//! - evidence is two independent facts and is never tidied into a pair that
//!   looks more sensible.

use griff_core::event::{
    NoteMark, NoteMarks, NotePosition, Pitch, SpanTechnique, TechniqueEvidence, TechniqueSource,
    TimeSignature, Tuning, ValidationError, Velocity,
};
use griff_core::score::{
    AtomEvent, AtomNote, AtomRest, EventGroup, EventGroupKind, ImportWarning, LossReport,
    MasterBar, RepeatMarker, Score, SourceMeta, TechniqueSpan, Track, Voice,
};
use griff_core::slice::TickRange;

use super::ExactWriteError;

/// The level-2 header, byte for byte (spec §1.1).
const HEADER: &str = "swang 2\n";

/// Writes `score` as one canonical level-2 document.
///
/// # Errors
/// [`ExactWriteError::OutsideWriterDomain`] when the score violates an
/// invariant `griff-core` declares — the only refusal there is, since
/// SWG-4A-05 finished the writer. It is returned **before any byte is
/// produced**: a partial document is not a thing this writer can emit.
pub fn write_score(score: &Score) -> Result<String, ExactWriteError> {
    // The only gate left. Earlier slices ran a second, temporary check for
    // the parts of the tree they could not yet spell; SWG-4A-05 spells all of
    // them, so a refusal can now mean exactly one thing.
    check_writer_domain(score)?;

    let mut out = String::from(HEADER);
    out.push_str("\nscore {\n");
    field(
        &mut out,
        "    ",
        "ppqn",
        &score.ticks_per_quarter.to_string(),
    );
    // Vector order, never sorted: the order is the semantics (§6.2 rule 2).
    for bar in &score.master_bars {
        write_master_bar(&mut out, bar);
    }
    for track in &score.tracks {
        write_track(&mut out, track);
    }
    if let Some(source) = score.source_meta.as_ref() {
        write_source(&mut out, source);
    }
    if !score.loss.is_clean() {
        write_loss(&mut out, &score.loss);
    }
    out.push_str("}\n");
    Ok(out)
}

/// One `<indent><word> <value>` line — the shape of every scalar field.
///
/// Assembled by pushing parts rather than by `write!` or
/// `push_str(&format!(…))`. The first returns a `Result` that cannot fail
/// when the sink is a `String`, and pretending to handle it would be
/// dishonest; the second is denied by `clippy::format_push_string`.
fn field(out: &mut String, indent: &str, word: &str, value: &str) {
    out.push_str(indent);
    out.push_str(word);
    out.push(' ');
    out.push_str(value);
    out.push('\n');
}

/// One `master_bar` block.
fn write_master_bar(out: &mut String, bar: &MasterBar) {
    out.push_str("\n    master_bar {\n");

    // The *stored* index (H4). Deriving it from the enumeration position
    // would make a disagreeing import unrepresentable.
    field(out, "        ", "index", &bar.index.to_string());

    let ticks = format!("{}..{}", bar.tick_range.start.0, bar.tick_range.end.0);
    field(out, "        ", "ticks", &ticks);

    let meter = format!(
        "{}/{}",
        bar.time_signature.numerator, bar.time_signature.denominator
    );
    field(out, "        ", "meter", &meter);

    // The reduced rational itself, denominator always written. `7/1` is a
    // tempo `from_bpm_integer` builds and `60_000_000 % 7 != 0` would
    // wrongly reject; writing the value rather than a reconstructed
    // provenance keeps that trap out of the writer entirely (H1).
    let tempo = format!(
        "{}/{}",
        bar.tempo.bpm_numerator(),
        bar.tempo.bpm_denominator()
    );
    field(out, "        ", "tempo", &tempo);

    // Omitted only for the model's own documented default, which
    // `RepeatMarker` itself defines as "no repeat barline". Every other
    // combination is written literally, including the degenerate
    // `play_count 1` that `closes()` calls false.
    if bar.repeat != RepeatMarker::default() {
        // Inline, as §6.1 spells it. The reference document decides which
        // blocks are inline; the writer obeys it rather than judging which
        // ones are short enough.
        out.push_str("        repeat { start ");
        out.push_str(&bar.repeat.start.to_string());
        out.push_str(" play_count ");
        out.push_str(&bar.repeat.play_count.to_string());
        out.push_str(" }\n");
    }

    out.push_str("    }\n");
}

// ── musical structure (SWG-4A-04) ──────────────────────────────────────────

/// One `track` block.
fn write_track(out: &mut String, track: &Track) {
    out.push_str("\n    track {\n");

    // `None` is spelled by absence; `Some("")` by `""`. The two states are
    // distinguishable in the model and stay so in text (§6.2).
    if let Some(name) = track.name.as_deref() {
        field(out, "        ", "name", &quote(name));
    }

    // No range clause exists for `channel` in `griff-core`, so `200` is
    // model-valid and is written plainly (§2.3).
    field(out, "        ", "channel", &track.channel.to_string());

    // A scalar list value, not a structural repeated block: the word is
    // always present and its empty form is `[]` (§6.2). Index 0 is string 1,
    // and reversing the list retunes the instrument, so it is never sorted.
    field(out, "        ", "tuning", &pitch_list(&track.tuning));

    for voice in &track.voices {
        write_voice(out, voice);
    }

    out.push_str("    }\n");
}

/// `[p1 p2 …]`, or `[]` — the one spelling of a pitch list (§6.3).
fn pitch_list(tuning: &Tuning) -> String {
    let mut out = String::from("[");
    for (position, pitch) in tuning.open_strings().iter().enumerate() {
        if position > 0 {
            out.push(' ');
        }
        out.push_str(&pitch.0.to_string());
    }
    out.push(']');
    out
}

/// One `voice` block.
fn write_voice(out: &mut String, voice: &Voice) {
    out.push_str("\n        voice {\n");

    // Duplicates within one track are model-valid (§3), so the id is written
    // as stored and voices are never merged or renumbered.
    field(out, "            ", "id", &voice.id.to_string());

    for group in &voice.event_groups {
        write_group(out, group);
    }

    out.push_str("        }\n");
}

/// One `group` block, its kind on the header line.
fn write_group(out: &mut String, group: &EventGroup) {
    out.push_str("\n            group ");
    out.push_str(&group_kind(group.kind));
    out.push_str(" {\n");

    // One sum-type slot, not two collections (§6.2 rule 3). Grouping every
    // note before every rest would look like a canonical normalization and
    // would rewrite the music.
    for atom in &group.atoms {
        write_atom(out, atom);
    }

    // Every atom, then every span: §6.2 rule 1 fixes the order *between*
    // slots, and rule 2 leaves the order *within* each of them alone.
    for span in &group.technique_spans {
        write_span(out, span);
    }

    // A group whose slots are both empty still writes its block: the block
    // is an element of `event_groups`, and it is the *elements* of a
    // repeated slot that vanish when there are none, not the slot's own
    // container. §6.2 states the layout; emptiness does not change it.
    out.push_str("            }\n");
}

/// The six `EventGroupKind` spellings (§6.4), `Tuplet`'s payload opened.
fn group_kind(kind: EventGroupKind) -> String {
    match kind {
        EventGroupKind::Single => "single".to_owned(),
        EventGroupKind::Chord => "chord".to_owned(),
        EventGroupKind::Arpeggio => "arpeggio".to_owned(),
        EventGroupKind::Strum => "strum".to_owned(),
        // The comparator has separate `TupletNum` and `TupletDen` fields, so
        // a text writing only "tuplet" would lose a fact the diff reports.
        // `0/0` is model-valid (§3) and spelled like any other pair.
        EventGroupKind::Tuplet { num, den } => {
            let mut out = String::from("tuplet ");
            out.push_str(&num.to_string());
            out.push('/');
            out.push_str(&den.to_string());
            out
        }
        EventGroupKind::Grace => "grace".to_owned(),
    }
}

/// One atom, inline as §6.1 spells it.
///
/// `at` plus `duration` is the whole story: a note crossing a barline stays
/// one note whose duration crosses the barline, and no tie is synthesized —
/// there is no tie in the canonical model to synthesize one from.
fn write_atom(out: &mut String, atom: &AtomEvent) {
    match *atom {
        AtomEvent::Note(note) => write_note(out, note),
        AtomEvent::Rest(rest) => write_rest(out, rest),
    }
}

/// One note.
///
/// The layout follows the note's own contents, which is the rule §6.2 states
/// for every construct: a note carrying a `position` opens across lines, one
/// without it stays on its line however many marks it has. `marks` is a
/// required word either way and `position` is omitted when `None`.
fn write_note(out: &mut String, note: AtomNote) {
    let Some(position) = note.position else {
        out.push_str("                note { at ");
        out.push_str(&note.absolute_start.0.to_string());
        out.push_str(" duration ");
        out.push_str(&note.duration.0.to_string());
        out.push_str(" pitch ");
        out.push_str(&note.pitch.0.to_string());
        out.push_str(" velocity ");
        out.push_str(&note.velocity.0.to_string());
        out.push_str(" marks ");
        out.push_str(&mark_list(note.marks));
        out.push_str(" }\n");
        return;
    };

    out.push_str("                note {\n");
    field(
        out,
        "                    ",
        "at",
        &note.absolute_start.0.to_string(),
    );
    field(
        out,
        "                    ",
        "duration",
        &note.duration.0.to_string(),
    );
    field(
        out,
        "                    ",
        "pitch",
        &note.pitch.0.to_string(),
    );
    field(
        out,
        "                    ",
        "velocity",
        &note.velocity.0.to_string(),
    );
    field(out, "                    ", "marks", &mark_list(note.marks));
    write_position(out, position);
    out.push_str("                }\n");
}

/// `[m1 m2 …]`, or `[]` — the one spelling of a mark list (§6.3).
///
/// The order is `NoteMark::ALL`, which is what `NoteMarks::iter` yields. A
/// set has no author order to preserve, so `ALL` is the only order there is;
/// sorting the words alphabetically would look tidier and be wrong.
fn mark_list(marks: NoteMarks) -> String {
    let mut out = String::from("[");
    for (position, mark) in marks.iter().enumerate() {
        if position > 0 {
            out.push(' ');
        }
        out.push_str(mark_word(mark));
    }
    out.push(']');
    out
}

/// The seven mark spellings (§6.4).
const fn mark_word(mark: NoteMark) -> &'static str {
    match mark {
        NoteMark::Accent => "accent",
        NoteMark::Ghost => "ghost",
        NoteMark::Staccato => "staccato",
        NoteMark::DeadNote => "dead_note",
        NoteMark::HarmonicNatural => "harmonic_natural",
        NoteMark::HarmonicPinch => "harmonic_pinch",
        NoteMark::Tap => "tap",
    }
}

/// One `position` block, with its own evidence.
///
/// No check that `string` fits the track's tuning: §3 lists a string beyond
/// the tuning as inhabited, and the writer is not a fretboard validator.
fn write_position(out: &mut String, position: NotePosition) {
    out.push_str("                    position {\n");
    field(
        out,
        "                        ",
        "string",
        &position.position.string.to_string(),
    );
    field(
        out,
        "                        ",
        "fret",
        &position.position.fret.to_string(),
    );
    out.push_str("                        ");
    out.push_str(&evidence_block(position.evidence));
    out.push('\n');
    out.push_str("                    }\n");
}

/// `evidence { source <src> confidence <bps> }` — inline, as §6.1 spells it.
///
/// The two facts are independent. `TechniqueEvidence`'s fields are public and
/// `explicit()` / `inferred()` are conveniences, so `Explicit` at 0 bps and
/// `InferredFromMidi` at 10 000 are both model-valid (§2.6) and both are
/// written as they stand. Repairing the pair here would delete what an
/// importer recorded.
fn evidence_block(evidence: TechniqueEvidence) -> String {
    let mut out = String::from("evidence { source ");
    out.push_str(match evidence.source {
        TechniqueSource::Explicit => "explicit",
        TechniqueSource::InferredFromMidi => "inferred_from_midi",
    });
    out.push_str(" confidence ");
    out.push_str(&evidence.confidence.get().to_string());
    out.push_str(" }");
    out
}

/// One `span` block.
fn write_span(out: &mut String, span: &TechniqueSpan) {
    out.push_str("                span ");
    out.push_str(span_word(span.technique));
    out.push_str(" {\n");

    let ticks = format!("{}..{}", span.tick_range.start.0, span.tick_range.end.0);
    field(out, "                    ", "ticks", &ticks);

    out.push_str("                    ");
    out.push_str(&evidence_block(span.evidence));
    out.push('\n');
    out.push_str("                }\n");
}

/// The eight `SpanTechnique` spellings (§6.4), matched exhaustively.
const fn span_word(technique: SpanTechnique) -> &'static str {
    match technique {
        SpanTechnique::Slide => "slide",
        SpanTechnique::Bend => "bend",
        SpanTechnique::Legato => "legato",
        SpanTechnique::PalmMute => "palm_mute",
        SpanTechnique::HammerOn => "hammer_on",
        SpanTechnique::PullOff => "pull_off",
        SpanTechnique::Vibrato => "vibrato",
        SpanTechnique::LetRing => "let_ring",
    }
}

fn write_rest(out: &mut String, rest: AtomRest) {
    // A rest is never an absence (§2.5): dropping a zero-duration rest would
    // delete an event the exact diff can see.
    out.push_str("                rest { at ");
    out.push_str(&rest.absolute_start.0.to_string());
    out.push_str(" duration ");
    out.push_str(&rest.duration.0.to_string());
    out.push_str(" }\n");
}

// ── document metadata (SWG-4A-05) ─────────────────────────────────────────

/// One `source` block, inline (§6.2).
///
/// Reached only when `source_meta` is `Some`. `None` is spelled by absence,
/// and `Some(SourceMeta { format: None })` is the empty body `source { }` —
/// three states, three documents, because the exact walker compares
/// `SourceMeta` and `Format` as separate fields (§2.1).
fn write_source(out: &mut String, source: &SourceMeta) {
    out.push_str("\n    source {");
    if let Some(format) = source.format.as_deref() {
        out.push_str(" format ");
        out.push_str(&quote(format));
    }
    out.push_str(" }\n");
}

/// One `loss` block — multiline, one warning per line (§6.2).
///
/// Reached only when the report is non-clean; a clean one is omitted whole.
/// Warning order is vector order and duplicates are duplicates: `LossReport`
/// appends and concatenates, and the exact walker compares positionally.
fn write_loss(out: &mut String, loss: &LossReport) {
    out.push_str("\n    loss {\n");
    for warning in &loss.warnings {
        write_warning(out, warning);
    }
    out.push_str("    }\n");
}

/// One warning, one physical line.
///
/// True whatever an `Other` message holds: a U+000A in the value is written
/// `\n` by §6.5's frozen policy, never as a real break.
fn write_warning(out: &mut String, warning: &ImportWarning) {
    out.push_str("        ");
    match warning {
        ImportWarning::TrackNameInvalidUtf8 { track_index } => {
            out.push_str("track_name_invalid_utf8 { track_index ");
            out.push_str(&track_index.to_string());
            out.push_str(" }");
        }
        ImportWarning::SmpteTimingUnsupported => {
            out.push_str("smpte_timing_unsupported");
        }
        ImportWarning::TempoApproximated {
            bar_index,
            nearest_micros,
        } => {
            out.push_str("tempo_approximated { bar_index ");
            out.push_str(&bar_index.to_string());
            out.push_str(" nearest_micros ");
            out.push_str(&nearest_micros.to_string());
            out.push_str(" }");
        }
        ImportWarning::Other(message) => {
            out.push_str("other { message ");
            out.push_str(&quote(message));
            out.push_str(" }");
        }
    }
    out.push('\n');
}

// ── the writer domain (census §3) ──────────────────────────────────────────

/// Re-checks the invariants `griff-core` already declares.
///
/// Every clause delegates to the model's own checked constructor where one
/// exists, so the predicate cannot drift away from the model's rules — and
/// the error it carries is the model's [`ValidationError`], not a second
/// small constitution written here.
///
/// The constructors are advisory rather than enforced (H5: the fields are
/// public), which is exactly why the writer must run them again on values
/// that may never have passed through them.
///
/// **Reach.** Complete since SWG-4A-05. All six of §3's clauses are present —
/// `ppqn`, pitch, velocity, meter numerator, meter denominator, tick range —
/// each applied everywhere the writer puts such a value in a document, which
/// is now everywhere the canonical tree holds one. The tick-range clause has
/// two homes: a master bar's range and every technique span's.
///
/// Six clauses and no seventh. `Tempo` positivity and `ConfidenceBps` range
/// need none: those types keep their fields private, so the constructor is
/// the only way in and the invariant is enforced rather than advisory. Adding
/// a check for either would be the writer inventing validation §3 forbids.
fn check_writer_domain(score: &Score) -> Result<(), ExactWriteError> {
    if score.ticks_per_quarter == 0 {
        return Err(ExactWriteError::OutsideWriterDomain {
            at: "score.ppqn",
            reason: ValidationError::InvalidTicksPerQuarter,
        });
    }
    for bar in &score.master_bars {
        TickRange::new(bar.tick_range.start, bar.tick_range.end).map_err(|reason| {
            ExactWriteError::OutsideWriterDomain {
                at: "master_bar.ticks",
                reason,
            }
        })?;
        TimeSignature::new(bar.time_signature.numerator, bar.time_signature.denominator).map_err(
            |reason| ExactWriteError::OutsideWriterDomain {
                at: "master_bar.meter",
                reason,
            },
        )?;
    }
    for track in &score.tracks {
        check_track_domain(track)?;
    }
    Ok(())
}

fn check_track_domain(track: &Track) -> Result<(), ExactWriteError> {
    // A `Tuning` is a `Vec<Pitch>` and `Tuning::new` re-checks nothing, so
    // an open string reaches the writer by the same route H5 describes for
    // a note's pitch. §3's clause is "every `Pitch`", unqualified.
    for pitch in track.tuning.open_strings() {
        Pitch::new(pitch.0).map_err(|reason| ExactWriteError::OutsideWriterDomain {
            at: "track.tuning",
            reason,
        })?;
    }
    for voice in &track.voices {
        for group in &voice.event_groups {
            for atom in &group.atoms {
                if let AtomEvent::Note(note) = *atom {
                    check_note_domain(note)?;
                }
            }
            // The sixth clause's other home. SWG-4A-04 left this out because
            // its frontier refused spans outright, so the writer could not
            // emit one and the clause had nothing to guard; writing them is
            // exactly what makes it reachable.
            for span in &group.technique_spans {
                TickRange::new(span.tick_range.start, span.tick_range.end).map_err(|reason| {
                    ExactWriteError::OutsideWriterDomain {
                        at: "span.ticks",
                        reason,
                    }
                })?;
            }
        }
    }
    Ok(())
}

fn check_note_domain(note: AtomNote) -> Result<(), ExactWriteError> {
    Pitch::new(note.pitch.0).map_err(|reason| ExactWriteError::OutsideWriterDomain {
        at: "note.pitch",
        reason,
    })?;
    Velocity::new(note.velocity.0).map_err(|reason| ExactWriteError::OutsideWriterDomain {
        at: "note.velocity",
        reason,
    })?;
    Ok(())
}

// ── strings (census §6.5) ──────────────────────────────────────────────────

/// The one canonical spelling of `value` as a quoted string.
///
/// Shared by `track.name`, `source.format`, and `loss.other.message` — the
/// only three strings in the canonical tree. One encoder rather than three,
/// because three would eventually disagree.
///
/// The escaped set is §6.5's **frozen range list** and nothing else. Not
/// `char::is_control`, not `char::is_whitespace`, not `escape_default`, not
/// a JSON encoder: §1.2 keeps table-dependent classification out of
/// observable semantics, and a formatter whose fixed point moved when the
/// build relinked a newer Unicode table would not have one. U+00A0 is the
/// character that tells the two apart — `is_whitespace` says yes, §6.5 says
/// write it through.
fn quote(value: &str) -> String {
    let mut out = String::with_capacity(value.len().saturating_add(2));
    out.push('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{0}'..='\u{8}' | '\u{b}'..='\u{c}' | '\u{e}'..='\u{1f}' | '\u{7f}'..='\u{9f}' => {
                out.push_str("\\u{");
                out.push_str(&lower_hex(u32::from(c)));
                out.push('}');
            }
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

/// `value` in lowercase hexadecimal with no leading zeros (§6.5).
///
/// Hand-rolled rather than `format!("{value:x}")`, because pushing a
/// `format!` into a `String` is denied by `clippy::format_push_string` and
/// the alternative — importing `fmt::Write` to `write!` into a sink that
/// cannot fail — would hand back a `Result` there is no honest way to handle.
/// Masks and shifts, so nothing here can divide by zero either.
fn lower_hex(value: u32) -> String {
    if value == 0 {
        return "0".to_owned();
    }
    let mut reversed = String::new();
    let mut rest = value;
    while rest > 0 {
        // `from_digit` yields lowercase for radix 16 and cannot fail for a
        // nibble, which is always below the radix.
        reversed.push(char::from_digit(rest & 0xf, 16).unwrap_or('0'));
        rest >>= 4;
    }
    reversed.chars().rev().collect()
}
