//! CLI transport for the `--rhythm-*` flags (spec §2).
//!
//! The pattern pipeline itself lives in
//! [`griff_swang::pattern_compile`] — the one compiler both `griff generate`
//! and the Swang evaluator drive. This module is the thin CLI edge: clap
//! value-enums for the two `--rhythm-*` choice flags, converting into the
//! griff-swang enums, plus a re-export of the compiler's public surface so
//! existing call sites keep their paths.

pub use griff_swang::pattern_compile::{
    compile_pattern, compile_pattern_flaws, parse_kernel_literal, parse_unit, PatternDiagnostic,
    PatternFlaw, PatternPlan, RhythmPatternArgs, TailChoice, TraversalChoice,
};

/// The `--rhythm-traversal` value flag; converts into
/// [`TraversalChoice`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum CliTraversal {
    /// Rows left-to-right, top-to-bottom.
    RowMajor,
    /// Boustrophedon: alternating rows reverse.
    Snake,
}

impl From<CliTraversal> for TraversalChoice {
    fn from(choice: CliTraversal) -> Self {
        match choice {
            CliTraversal::RowMajor => Self::RowMajor,
            CliTraversal::Snake => Self::Snake,
        }
    }
}

/// The `--rhythm-tail` value flag; converts into [`TailChoice`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum CliTail {
    /// An incomplete final bar is a typed error — the documented default.
    Reject,
    /// The final bar's missing slots become timed rests.
    RestPad,
}

impl From<CliTail> for TailChoice {
    fn from(choice: CliTail) -> Self {
        match choice {
            CliTail::Reject => Self::Reject,
            CliTail::RestPad => Self::RestPad,
        }
    }
}
