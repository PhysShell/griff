//! The hand-written lexer (ADR-0029 §11).

use std::iter::Peekable;
use std::str::CharIndices;

use super::diagnostic::Diagnostic;
use super::span::span_of;
use super::token::{Token, TokenKind};

/// Lexes `source` from byte `from` on. Whitespace is ASCII only — the
/// determinism law (spec §1.2) keeps Unicode classification out of anything
/// semantics can observe, so a non-ASCII space is `SWG0401`, not a
/// separator.
pub(crate) fn lex(source: &str, from: usize) -> Result<Vec<Token>, Diagnostic> {
    let tail = source.get(from..).unwrap_or_default();
    let mut tokens = Vec::new();
    let mut chars = tail.char_indices().peekable();
    while let Some((at, c)) = chars.next() {
        let start = from.saturating_add(at);
        match c {
            ' ' | '\t' | '\r' | '\n' => {}
            '{' | '}' => tokens.push(Token {
                kind: if c == '{' {
                    TokenKind::OpenBrace
                } else {
                    TokenKind::CloseBrace
                },
                text: c.to_string(),
                span: span_of(start, start.saturating_add(1)),
            }),
            '|' => match chars.next() {
                Some((_, '>')) => tokens.push(Token {
                    kind: TokenKind::Pipe,
                    text: "|>".to_owned(),
                    span: span_of(start, start.saturating_add(2)),
                }),
                _ => {
                    return Err(Diagnostic {
                        code: "SWG0401",
                        span: span_of(start, start.saturating_add(1)),
                        message: "expected `|>`".to_owned(),
                    })
                }
            },
            '"' => tokens.push(lex_string(tail, from, at, &mut chars)?),
            'A'..='Z' | 'a'..='z' | '_' => {
                let end = lex_while(tail, &mut chars, |ch| {
                    ch.is_ascii_alphanumeric() || ch == '_'
                });
                tokens.push(token_from(tail, from, at, end, TokenKind::Word));
            }
            '0'..='9' => {
                let end = lex_while(tail, &mut chars, |ch| {
                    ch.is_ascii_alphanumeric() || ch == '_' || ch == '/'
                });
                tokens.push(token_from(tail, from, at, end, TokenKind::NumberLike));
            }
            other => {
                return Err(Diagnostic {
                    code: "SWG0401",
                    span: span_of(start, start.saturating_add(other.len_utf8())),
                    message: format!("unexpected character {other:?}"),
                })
            }
        }
    }
    Ok(tokens)
}

/// Consumes characters while `keep` holds; returns the end byte offset
/// (relative to `tail`).
fn lex_while(
    tail: &str,
    chars: &mut Peekable<CharIndices<'_>>,
    keep: impl Fn(char) -> bool,
) -> usize {
    while let Some(&(_, c)) = chars.peek() {
        if keep(c) {
            chars.next();
        } else {
            break;
        }
    }
    chars.peek().map_or(tail.len(), |&(next, _)| next)
}

/// Builds a word/number token from `tail[at..end]`.
fn token_from(tail: &str, from: usize, at: usize, end: usize, kind: TokenKind) -> Token {
    Token {
        kind,
        text: tail.get(at..end).unwrap_or_default().to_owned(),
        span: span_of(from.saturating_add(at), from.saturating_add(end)),
    }
}

/// Lexes a double-quoted string literal starting at `at` (the opening
/// quote). No escapes; a newline or the end of input before the closing
/// quote is `SWG0401`.
fn lex_string(
    tail: &str,
    from: usize,
    at: usize,
    chars: &mut Peekable<CharIndices<'_>>,
) -> Result<Token, Diagnostic> {
    for (i, c) in chars.by_ref() {
        match c {
            '"' => {
                let content_start = at.saturating_add(1);
                return Ok(Token {
                    kind: TokenKind::Str,
                    text: tail.get(content_start..i).unwrap_or_default().to_owned(),
                    span: span_of(
                        from.saturating_add(at),
                        from.saturating_add(i).saturating_add(1),
                    ),
                });
            }
            '\n' => break,
            _ => {}
        }
    }
    Err(Diagnostic {
        code: "SWG0401",
        span: span_of(from.saturating_add(at), from.saturating_add(tail.len())),
        message: "unterminated string literal".to_owned(),
    })
}
