//! Canonical level-2 text from a canonical `Score` — transport and master
//! timeline (SWG-4A-03).
//!
//! The census (`docs/swang/exact-score-text.md`) decided every spelling
//! here; this module only obeys it. Four of those decisions are load-bearing
//! and easy to undo by accident, so each is marked at its site:
//!
//! - `MasterBar.index` is the **stored** value, never the vector position;
//! - `master_bars` keeps **vector order** and is never sorted;
//! - `Tempo` is written from its own reduced rational, including `7/1` and
//!   `100/7`, never reconstructed from a constructor's provenance;
//! - `repeat` is omitted for `RepeatMarker::default()` and written
//!   literally for every other combination.

use griff_core::event::{TimeSignature, ValidationError};
use griff_core::score::{MasterBar, RepeatMarker, Score};
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
        // Inline, as §6.1 spells it. A block small enough to read on one
        // line is written on one line; the layout is part of the canonical
        // text, not a formatting preference.
        out.push_str("        repeat { start ");
        out.push_str(&bar.repeat.start.to_string());
        out.push_str(" play_count ");
        out.push_str(&bar.repeat.play_count.to_string());
        out.push_str(" }\n");
    }

    out.push_str("    }\n");
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
    Ok(())
}

// ── this slice's frontier ──────────────────────────────────────────────────

/// Refuses the parts of the tree SWG-4A-03 does not write yet.
///
/// Not a claim about representability: every state named here is spellable
/// under the accepted grammar, and a later slice will spell it.
const fn check_slice_frontier(score: &Score) -> Result<(), ExactWriteError> {
    if !score.tracks.is_empty() {
        return Err(ExactWriteError::NotYetWritten {
            what: "score.track",
            task: "SWG-4A-04",
        });
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
