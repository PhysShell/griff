//! The lexemes the grammar reads.

use super::span::Span;

/// One lexeme. `text` is the word/number spelling, or the string literal's
/// content without its quotes; spans always cover the full source lexeme.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Token {
    pub(crate) kind: TokenKind,
    pub(crate) text: String,
    pub(crate) span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TokenKind {
    /// `[A-Za-z_][A-Za-z0-9_]*`
    Word,
    /// Digit-initiated: an integer, a `bps`-suffixed density, or a rational
    /// note value — the construct decides which form it accepts.
    NumberLike,
    /// A double-quoted literal, no escapes, single-line.
    Str,
    OpenBrace,
    CloseBrace,
    /// `|>`
    Pipe,
}
