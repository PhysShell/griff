//! The parse-time diagnostic — a stable `SWG____` registry code, a source
//! span, and a message (spec §1.5).

use super::span::Span;

/// A parse-time diagnostic: a stable `SWG____` registry code, a source span,
/// and a message — pure data, rendered only at the frontend edge (spec §1.5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// The stable registry code.
    pub code: &'static str,
    /// Where in the source the user's fix lives.
    pub span: Span,
    /// What went wrong, in the construct's own vocabulary.
    pub message: String,
}
