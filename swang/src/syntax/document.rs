//! Level dispatch: one header, one entry point, no shared grammar.
//!
//! ```text
//! header_level -> level dispatch -> root dispatch (pattern | score)
//! ```
//!
//! Spec §5.4 requires that "each released level owns its own parser and
//! formatter entry point. A build routes to one of them and never mixes
//! them: there is no single grammar with level-conditioned branches, because
//! such a grammar has no way to prove that level 1's behaviour survived the
//! addition of level 2."
//!
//! So this module is a router and nothing else. It holds no grammar, no
//! token, no budget and no diagnostic of its own beyond the one defensive
//! arm below: everything it can say, it says by choosing which parser to
//! call. That is also why it is not on `level_two_budget_boundary.rs`'s
//! exempt list — a budget consulted here, before the branch, would be a
//! level-1 bound whatever file it lived in.

use super::ast::v1::Program;
use super::diagnostic::Diagnostic;
use super::format::v1::format as format_pattern;
use super::format::v2::format_exact;
use super::header::{header_level, LANGUAGE_LEVEL};
use super::parser;
use super::parser::v2::ExactScore;
use super::span::span_of;

/// A parsed Swang document, tagged by the root its level admits.
///
/// The two roots are two levels' (spec §5.7): `swang 1` writes `pattern`,
/// `swang 2` writes `score`, and nothing has yet earned a document that
/// holds both at once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Document {
    /// A level-1 pattern program.
    Pattern(Program),
    /// A level-2 exact score.
    Score(ExactScore),
}

/// Parses a source at whichever level its header pins.
///
/// # Errors
/// The frozen §1.1 pre-parser's diagnostics for a header this build cannot
/// read, and otherwise exactly the chosen level's own.
pub fn parse_document(source: &str) -> Result<Document, Vec<Diagnostic>> {
    let level = header_level(source).map_err(|d| vec![d])?;
    match level {
        1 => parser::v1::parse(source).map(Document::Pattern),
        2 => parser::v2::parse_exact(source).map(Document::Score),
        // Unreachable: the pre-parser already refused anything outside
        // `1..=LANGUAGE_LEVEL`. Defense in depth, so that raising the
        // constant without adding an arm is a refusal rather than a panic
        // or, worse, a silent fall-through to the wrong grammar.
        other => Err(vec![Diagnostic {
            code: "SWG0001",
            span: span_of(6, 6),
            message: format!(
                "language level {other} has no parser in this build (1..={LANGUAGE_LEVEL})"
            ),
        }]),
    }
}

/// Emits the canonical text of a document, at its own level.
///
/// Level 1's output is byte-identical to the frozen level-1 formatter's,
/// permanently (spec §5.8, Law A observable 3).
#[must_use]
pub fn format_document(document: &Document) -> String {
    match document {
        Document::Pattern(program) => format_pattern(program),
        Document::Score(score) => format_exact(score),
    }
}
