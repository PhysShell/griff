//! Why the exact writer produced no bytes.

use core::error::Error;
use core::fmt::{Display, Formatter, Result as FmtResult};

use griff_core::event::ValidationError;

/// A refusal from the exact-text writer.
///
/// One variant, and that is the point. While the writer was built in slices
/// this enum also carried `NotYetWritten`, a statement about how much of the
/// writer existed rather than about the score. SWG-4A-05 finished the writer,
/// so that variant described nothing and retired with the frontier it named:
/// a refusal now means exactly one thing, and no caller has to handle a case
/// that cannot occur.
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
}

impl Display for ExactWriteError {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        let Self::OutsideWriterDomain { at, reason } = self;
        write!(f, "{at} is outside the exact writer's domain: {reason:?}")
    }
}

impl Error for ExactWriteError {}
