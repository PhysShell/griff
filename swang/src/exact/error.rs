//! Why the exact writer produced no bytes.

use core::error::Error;
use core::fmt::{Display, Formatter, Result as FmtResult};

use griff_core::event::ValidationError;

/// A refusal from the exact-text writer.
///
/// The two variants say different things and must not be collapsed. One is
/// a judgement about the `Score`; the other is a statement about how much
/// of the writer exists yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExactWriteError {
    /// The `Score` violates an invariant `griff-core` itself declares, so
    /// **no** exact writer — this one or the finished one — may emit it
    /// (`docs/swang/exact-score-text.md` §3).
    ///
    /// The carried `reason` is the model's own [`ValidationError`], not a
    /// message of the writer's invention: that is what makes "this
    /// predicate invents nothing" checkable rather than merely claimed.
    OutsideWriterDomain {
        /// Where in the score the violation sits, for a human reading the
        /// refusal. Not a source span — there is no source yet.
        at: &'static str,
        /// The model's own verdict.
        reason: ValidationError,
    },
    /// The `Score` is perfectly writable in principle; this increment of
    /// the writer does not reach that part of the tree yet.
    ///
    /// A slice boundary, never a claim that an inhabited state is
    /// unspellable — the census forbids the latter, and reporting a
    /// build-stage limit as a domain judgement would smuggle it in.
    NotYetWritten {
        /// The part of the tree that is still unimplemented.
        what: &'static str,
        /// The task that will write it.
        task: &'static str,
    },
}

impl Display for ExactWriteError {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match self {
            Self::OutsideWriterDomain { at, reason } => {
                write!(f, "{at} is outside the exact writer's domain: {reason:?}")
            }
            Self::NotYetWritten { what, task } => {
                write!(f, "{what} is not written yet; {task} adds it")
            }
        }
    }
}

impl Error for ExactWriteError {}
