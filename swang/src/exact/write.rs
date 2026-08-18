//! Canonical level-2 text from a canonical `Score` — transport, master
//! timeline (SWG-4A-03), and musical structure (SWG-4A-04).
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
//!   `[]` and never an absent word.

use griff_core::event::{Pitch, TimeSignature, Tuning, ValidationError, Velocity};
use griff_core::score::{
    AtomEvent, AtomNote, AtomRest, EventGroup, EventGroupKind, MasterBar, RepeatMarker, Score,
    Track, Voice,
};
use griff_core::slice::TickRange;

use super::ExactWriteError;

/// The level-2 header, byte for byte (spec §1.1).
const HEADER: &str = "swang 2\n";

/// Writes `score` as one canonical level-2 document.
///
/// # Errors
/// [`ExactWriteError::OutsideWriterDomain`] when the score violates an
/// invariant `griff-core` declares, and [`ExactWriteError::NotYetWritten`]
/// when it reaches a part of the tree this slice does not cover. Both are
/// returned **before any byte is produced**: a partial document is not a
/// thing this writer can emit.
pub fn write_score(score: &Score) -> Result<String, ExactWriteError> {
    // Domain first, frontier second, and the order is deliberate. A score
    // that is both invalid and beyond this slice gets the permanent verdict,
    // not the temporary one: "the finished writer will refuse this too" is
    // strictly more informative than "not implemented yet", and reversing
    // the checks would hide the stronger statement behind the weaker.
    check_writer_domain(score)?;
    check_slice_frontier(score)?;

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
    // distinguishable in the model and stay so in text (§6.2). Only names
    // needing no escape reach here — the frontier refused the rest.
    if let Some(name) = track.name.as_deref() {
        let mut quoted = String::with_capacity(name.len().saturating_add(2));
        quoted.push('"');
        quoted.push_str(name);
        quoted.push('"');
        field(out, "        ", "name", &quoted);
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

    // A group whose atoms vector is empty still writes its block: the block
    // is an element of `event_groups`, and it is the *elements* of a
    // repeated slot that vanish when there are none, not the slot's own
    // container. §6.1 spells `group` across lines, and emptiness does not
    // change a block's layout — see the residual noted in the task report.
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

fn write_note(out: &mut String, note: AtomNote) {
    out.push_str("                note { at ");
    out.push_str(&note.absolute_start.0.to_string());
    out.push_str(" duration ");
    out.push_str(&note.duration.0.to_string());
    out.push_str(" pitch ");
    out.push_str(&note.pitch.0.to_string());
    out.push_str(" velocity ");
    out.push_str(&note.velocity.0.to_string());
    // A required word whose empty form is `[]` (§6.2), exactly like
    // `tuning`. Only empty sets reach here; the mark vocabulary is 4A-05's,
    // and `position` — omitted when `None` — is refused when present.
    out.push_str(" marks [] }\n");
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
/// **Reach.** The walk covers exactly what this slice can emit. All six of
/// §3's clauses are present — `ppqn`, pitch, velocity, meter numerator,
/// meter denominator, tick range — each applied wherever the writer would
/// otherwise put the value in a document. Technique spans carry a
/// `TickRange` and are *not* walked, because the frontier refuses any score
/// holding one: the writer cannot emit text its own builder would reject,
/// which is the property §3 exists to guarantee. When 4A-05 starts writing
/// spans, their ranges join this walk.
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

// ── this slice's frontier ──────────────────────────────────────────────────

/// Refuses the parts of the tree SWG-4A-04 does not write yet.
///
/// Not a claim about representability: every state named here is spellable
/// under the accepted grammar, and SWG-4A-05 will spell it.
fn check_slice_frontier(score: &Score) -> Result<(), ExactWriteError> {
    for track in &score.tracks {
        // Only names written through unescaped are in this slice. Drawing
        // the boundary at one field rather than dragging half of §6.5's
        // encoding policy back here keeps the escape rules in one place.
        if track.name.as_deref().is_some_and(needs_escape) {
            return Err(ExactWriteError::NotYetWritten {
                what: "track.name needing an escape",
                task: "SWG-4A-05",
            });
        }
        for voice in &track.voices {
            for group in &voice.event_groups {
                if !group.technique_spans.is_empty() {
                    return Err(ExactWriteError::NotYetWritten {
                        what: "group.span",
                        task: "SWG-4A-05",
                    });
                }
                for atom in &group.atoms {
                    if let AtomEvent::Note(note) = *atom {
                        check_note_frontier(note)?;
                    }
                }
            }
        }
    }
    if score.source_meta.is_some() {
        return Err(ExactWriteError::NotYetWritten {
            what: "score.source",
            task: "SWG-4A-05",
        });
    }
    if !score.loss.is_clean() {
        return Err(ExactWriteError::NotYetWritten {
            what: "score.loss",
            task: "SWG-4A-05",
        });
    }
    Ok(())
}

const fn check_note_frontier(note: AtomNote) -> Result<(), ExactWriteError> {
    // `marks []` is written because §6.2 makes it a required word. The mark
    // vocabulary itself belongs to the slice that owns §6.4's closed sets.
    if !note.marks.is_empty() {
        return Err(ExactWriteError::NotYetWritten {
            what: "note.marks",
            task: "SWG-4A-05",
        });
    }
    if note.position.is_some() {
        return Err(ExactWriteError::NotYetWritten {
            what: "note.position",
            task: "SWG-4A-05",
        });
    }
    Ok(())
}

/// Whether writing `value` through unchanged would misspell it (§6.5).
///
/// The escaped set is the frozen range list of §6.5 and nothing else — not
/// `char::is_control`, not `char::is_whitespace`, and no other Unicode-table
/// predicate. §1.2 keeps table-dependent classification out of observable
/// semantics, so a value's spelling must not change when the build relinks a
/// newer Unicode table.
fn needs_escape(value: &str) -> bool {
    value
        .chars()
        .any(|c| matches!(c, '"' | '\\' | '\u{0}'..='\u{1f}' | '\u{7f}'..='\u{9f}'))
}
