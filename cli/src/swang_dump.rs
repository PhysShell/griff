//! SWG-4A-10: composing the two surfaces of `griff swang dump`.
//!
//! The command has exactly one interesting decision in it, and this module
//! is where it lives: a canonical `Score` produces **two** independent
//! renderings, and neither may consume the other.
//!
//! - The **document** is the canonical level-2 exact text, straight from
//!   [`griff_swang::exact::write_score`]. It goes to stdout, whole or not at
//!   all. Nothing here sorts, tidies, annotates, or re-spells it: the census
//!   (`docs/swang/exact-score-text.md`) already decided every byte, and a
//!   second opinion at the CLI would be a second formatter.
//! - The **warnings** are a human rendering of `score.loss` for stderr: one
//!   rendered entry per `ImportWarning`, in the report's vector order.
//!
//! One *entry*, deliberately, and not one physical line. `ImportWarning`
//! includes `Other(String)`, whose message is unrestricted and may contain a
//! line break — SWG-4A-05 pinned that case in the exact text precisely
//! because the data can hold one. The exact writer escapes it, because its
//! grammar says a warning is one physical line. Nothing obliges the terminal
//! rendering to agree, and SWG-4A-10 introduces no escaping or sanitization
//! policy of its own: promising a line count here would be inventing a
//! contract to make a sentence true.
//!
//! The loss report appears on both surfaces, and that is deliberate rather
//! than redundant. `Score.loss` is a canonical fact the exact text is
//! obliged to carry (§2.8); the stderr rendering is a courtesy to whoever is
//! watching the terminal. Dropping the `loss` block because a human already
//! saw the warning would make the document depend on who was looking at it.

use griff_core::score::{ImportWarning, Score};
use griff_swang::exact::{write_score, ExactWriteError};

/// The two surfaces of one dump, both built before either is written.
///
/// Returning them together is what makes "no partial document" structural
/// rather than a discipline: a refusal produces no `DumpOutput` at all, so
/// there is no half-formed value for a caller to print.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DumpOutput {
    /// The canonical level-2 document, for stdout.
    pub document: String,
    /// One human-facing rendering per `ImportWarning`, in `LossReport`
    /// vector order, for stderr. An entry is not promised to be a single
    /// physical line: an `Other` message may carry its own line breaks, and
    /// this surface adds no escaping policy to prevent that.
    pub warnings: Vec<String>,
}

/// Renders `score` as the document and the warning entries that accompany
/// it, one entry per `ImportWarning`, in `LossReport` order.
///
/// # Errors
/// [`ExactWriteError`] when the score is outside the exact writer's domain.
/// The writer produces no bytes in that case and neither does this.
pub fn dump_score(score: &Score) -> Result<DumpOutput, ExactWriteError> {
    let document = write_score(score)?;
    let warnings = score.loss.warnings.iter().map(describe_warning).collect();
    Ok(DumpOutput { document, warnings })
}

/// One import warning, in a sentence — verbatim for `Other`, whose message
/// is unrestricted and is neither escaped nor reflowed here.
///
/// The exact text spells the same facts in its own frozen grammar; this is
/// the terminal rendering and is never the source of truth for any of them.
fn describe_warning(warning: &ImportWarning) -> String {
    match warning {
        ImportWarning::TrackNameInvalidUtf8 { track_index } => {
            format!("track {track_index} has a name that is not valid UTF-8; it was dropped")
        }
        ImportWarning::SmpteTimingUnsupported => {
            "the source used SMPTE timing, which griff does not support".to_owned()
        }
        ImportWarning::TempoApproximated {
            bar_index,
            nearest_micros,
        } => format!(
            "the tempo of bar {bar_index} has no exact microsecond form; \
             it was approximated to {nearest_micros}"
        ),
        ImportWarning::Other(message) => message.clone(),
    }
}
