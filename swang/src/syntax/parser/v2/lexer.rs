//! The level-2 lexer and the token it retains.
//!
//! Level 1's [`Token`] owns a `String` per lexeme. That is not the level-2
//! storage representation, and saying so is an obligation rather than a
//! preference: spec §5.11 declares `MAX_TOKENS = 4_000_000`, and the
//! derivation behind that number budgets **at most 12 bytes per retained
//! token on `wasm32`**. A token owning a `String` costs 12 bytes for the
//! `String` alone on a 32-bit target, before its kind, its span, and the
//! heap block each lexeme would allocate.
//!
//! So a level-2 token is a kind and a [`Span`], and the lexeme is sliced
//! back out of the source on demand ([`Level2Token::text_in`]). The bound is
//! asserted at compile time rather than in a host test, because the claim is
//! about `wasm32` and a runtime assertion on `x86_64` proves `x86_64`.
//!
//! [`Token`]: crate::syntax::token::Token

use std::iter::Peekable;
use std::mem::size_of;
use std::str::CharIndices;

use crate::syntax::diagnostic::Diagnostic;
use crate::syntax::limits::Level2Budget;
use crate::syntax::span::{span_of, Span};

/// One retained level-2 lexeme: what it is, and where it is. Never what it
/// says — that is [`Level2Token::text_in`]'s job, and the reason this type
/// fits the budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Level2Token {
    /// Which lexeme class this is.
    pub(crate) kind: Level2TokenKind,
    /// The full source range of the lexeme.
    pub(crate) span: Span,
}

/// The lexeme classes the level-2 grammar reads. Strings, and with them the
/// escape policy and `SWG0508`, are SWG-4A-07's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Level2TokenKind {
    /// `[A-Za-z_][A-Za-z0-9_]*`
    Word,
    /// A run of ASCII digits. Its spelling laws belong to the construct that
    /// reads it, not to the lexer.
    Number,
    /// `{`
    OpenBrace,
    /// `}`
    CloseBrace,
}

/// The retained-token budget from spec §5.11, asserted where every target
/// this crate builds for must satisfy it — `wasm32` included.
///
/// The preregistered SWG-INF-06 falsification probe is *adding owned lexeme
/// text to the level-2 token*: a `String` field takes the type to 24 bytes
/// on `wasm32`, and this assertion is what refuses to compile.
const _RETAINED_TOKEN_FITS_THE_BUDGET: () = assert!(
    size_of::<Level2Token>() <= 12,
    "a retained level-2 token must fit spec §5.11's 12-byte wasm32 budget; \
     owning lexeme text is what breaks this"
);

impl Level2Token {
    /// The lexeme, sliced out of the source this token was lexed from.
    pub(crate) fn text_in<'a>(&self, source: &'a str) -> &'a str {
        source
            .get(self.span.start as usize..self.span.end as usize)
            .unwrap_or_default()
    }
}

/// Lexes `source` from byte `from` on, spending one token of `budget` for
/// every token retained.
///
/// Whitespace is ASCII only, as at level 1: the determinism law (spec §1.2)
/// keeps Unicode classification out of anything semantics can observe.
///
/// # Errors
/// `SWG0401` for a character the level-2 grammar does not read, and
/// `SWG0509` when the token allowance is spent — the breach is typed and
/// reaches the caller rather than being discovered by the allocator.
pub(crate) fn lex_level_two(
    source: &str,
    from: usize,
    budget: &mut Level2Budget,
) -> Result<Vec<Level2Token>, Diagnostic> {
    let tail = source.get(from..).unwrap_or_default();
    let mut tokens = Vec::new();
    let mut chars = tail.char_indices().peekable();
    while let Some((at, c)) = chars.next() {
        let start = from.saturating_add(at);
        let kind = match c {
            ' ' | '\t' | '\r' | '\n' => continue,
            '{' => Level2TokenKind::OpenBrace,
            '}' => Level2TokenKind::CloseBrace,
            'A'..='Z' | 'a'..='z' | '_' => Level2TokenKind::Word,
            '0'..='9' => Level2TokenKind::Number,
            other => {
                return Err(Diagnostic {
                    code: "SWG0401",
                    span: span_of(start, start.saturating_add(other.len_utf8())),
                    message: format!("unexpected character {other:?}"),
                })
            }
        };
        let end = match kind {
            Level2TokenKind::Word => run(tail, &mut chars, |ch| {
                ch.is_ascii_alphanumeric() || ch == '_'
            }),
            Level2TokenKind::Number => run(tail, &mut chars, |ch| ch.is_ascii_digit()),
            Level2TokenKind::OpenBrace | Level2TokenKind::CloseBrace => at.saturating_add(1),
        };
        let span = span_of(start, from.saturating_add(end));
        // Admitted before it is retained: a token that would exceed the
        // allowance is never stored, so the refusal costs one token's worth
        // of memory rather than the whole tail of the file.
        budget.admit_token(span)?;
        tokens.push(Level2Token { kind, span });
    }
    Ok(tokens)
}

/// Consumes characters while `keep` holds; returns the end byte offset
/// relative to `tail`.
fn run(tail: &str, chars: &mut Peekable<CharIndices<'_>>, keep: impl Fn(char) -> bool) -> usize {
    while let Some(&(_, c)) = chars.peek() {
        if keep(c) {
            chars.next();
        } else {
            break;
        }
    }
    chars.peek().map_or(tail.len(), |&(next, _)| next)
}
