//! The level-2 exact-score parser: root dispatch and the minimal score.
//!
//! Spec §5.4 gives each released level its own parser entry point, and this
//! is level 2's. It never sees a `swang 1` source and never branches on a
//! level — the branch happened one module up, in the dispatcher.
//!
//! # What this slice parses
//!
//! Exactly the minimal empty score:
//!
//! ```text
//! swang 2
//!
//! score {
//!     ppqn 960
//! }
//! ```
//!
//! `ppqn` is `score`'s only `1` word; `master_bar`, `track`, `source` and
//! `loss` are `*`/`?` words (`exact-score-text.md` §6.4b) whose omission is
//! §6.2's, not a missing required word. So the minimal score is a complete
//! level-2 program, not a stub — and the structural tree that fills those
//! slots is SWG-4A-08's.
//!
//! Words this slice does not implement are **refused**, never skipped. A
//! parser that ignores a word it does not understand is how exact text stops
//! being exact, and the refusal is a `SWG0401` that says which task owns the
//! word rather than pretending the grammar has no such thing.
//!
//! # Where the budget is spent
//!
//! The level-2 budget (SWG-INF-06, spec §5.11) is constructed here and
//! nowhere else, and it is consulted before the work it bounds: the source
//! byte cap before lexing, the token cap inside the lexer as each token is
//! retained, and structural depth as each block is entered. There is no
//! successful `swang 2` result that does not pass through all three.
//!
//! `admit_diagnostic` is deliberately not called. The diagnostic cap governs
//! what a *recovering* parser returns, and recovery is SWG-INF-05's; this
//! parser stops at the first refusal, so it can no more exhaust that axis
//! than level 1 can.

pub(crate) mod lexer;

use self::lexer::{lex_level_two, Level2Token, Level2TokenKind};
use crate::syntax::ast::v2::ExactScoreDocument;
use crate::syntax::diagnostic::Diagnostic;
use crate::syntax::header::HEADER_WINDOW;
use crate::syntax::limits::Level2Budget;
use crate::syntax::span::{span_of, Span};

/// A parsed level-2 exact score.
///
/// Opaque on purpose. SWG-4A-02 made the exact document a transient syntax
/// form and fenced it off the public surface so it could not quietly become
/// a second durable model beside `griff_core::Score`; the fence holds here.
/// This handle exists so that a level-2 parse has a public result at all —
/// which is what lets the fuzz oracle reach the real path — and it exposes
/// nothing of what it wraps. SWG-4A-09's builder is what opens it, into a
/// `Score` and nothing else.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExactScore(ExactScoreDocument);

impl ExactScore {
    /// The wrapped document, for the crate-internal formatter.
    pub(crate) const fn document(&self) -> &ExactScoreDocument {
        &self.0
    }
}

/// Parses a level-2 source. The caller has already proved the header says
/// `swang 2`; this function owns everything after it.
///
/// # Errors
/// One diagnostic. `SWG0509` for a declared budget breach, `SWG0403` for a
/// missing required word, `SWG0404` for a repeated singleton, `SWG0505` for
/// a non-canonical spelling, `SWG0506` for a canonical-model invariant, and
/// `SWG0401` for everything structural.
pub(crate) fn parse_exact(source: &str) -> Result<ExactScore, Vec<Diagnostic>> {
    let mut budget = Level2Budget::declared();
    parse_score(source, &mut budget)
        .map(ExactScore)
        .map_err(|d| vec![d])
}

/// The parse itself, spending a budget the caller owns.
///
/// The caller owning it is what makes the accounting observable: the
/// counters can be read after the parse — including after a refusal, where
/// a depth still held is proof the block was entered. The budget is not an
/// argument a caller may weaken, because `Level2ResourceLimits` has private
/// fields and `declared()` as its only production constructor.
///
/// # Errors
/// Exactly [`parse_exact`]'s, unwrapped from the vector.
pub(crate) fn parse_score(
    source: &str,
    budget: &mut Level2Budget,
) -> Result<ExactScoreDocument, Diagnostic> {
    budget.admit_source(source, span_of(0, source.len()))?;
    let tokens = lex_level_two(source, body_offset(source), budget)?;
    let mut parser = Parser {
        source,
        tokens,
        pos: 0,
        eof: span_of(source.len(), source.len()),
        budget,
    };
    let document = parser.score()?;
    parser.expect_end()?;
    Ok(document)
}

/// The first byte after the header line, matching level 1's own arithmetic.
fn body_offset(source: &str) -> usize {
    source
        .as_bytes()
        .iter()
        .take(HEADER_WINDOW)
        .position(|&b| b == b'\n')
        .map_or(source.len(), |lf| lf.saturating_add(1))
}

/// The `score` words this grammar defines but this slice does not yet parse
/// (`exact-score-text.md` §6.4b). Named so the refusal can say which task
/// owns them instead of claiming they are not words at all.
const DEFERRED_SCORE_WORDS: &[&str] = &["master_bar", "track", "source", "loss"];

struct Parser<'a> {
    source: &'a str,
    tokens: Vec<Level2Token>,
    pos: usize,
    eof: Span,
    budget: &'a mut Level2Budget,
}

impl Parser<'_> {
    fn next(&mut self) -> Option<Level2Token> {
        let token = self.tokens.get(self.pos).copied();
        if token.is_some() {
            self.pos = self.pos.saturating_add(1);
        }
        token
    }

    fn peek(&self) -> Option<Level2Token> {
        self.tokens.get(self.pos).copied()
    }

    fn text(&self, token: Level2Token) -> &str {
        token.text_in(self.source)
    }

    fn unexpected_end(&self) -> Diagnostic {
        Diagnostic {
            code: "SWG0401",
            span: self.eof,
            message: "unexpected end of input".to_owned(),
        }
    }

    /// A structural refusal at a token: `SWG0401`, the class level 1 already
    /// uses for an unexpected token or a violated shape.
    const fn structural(token: Level2Token, message: String) -> Diagnostic {
        Diagnostic {
            code: "SWG0401",
            span: token.span,
            message,
        }
    }

    fn expect_kind(
        &mut self,
        kind: Level2TokenKind,
        what: &str,
    ) -> Result<Level2Token, Diagnostic> {
        let token = self.next().ok_or_else(|| self.unexpected_end())?;
        if token.kind == kind {
            Ok(token)
        } else {
            let found = self.text(token).to_owned();
            Err(Self::structural(
                token,
                format!("expected {what}, found `{found}`"),
            ))
        }
    }

    /// `score { … }` — the one level-2 root (spec §5.7).
    fn score(&mut self) -> Result<ExactScoreDocument, Diagnostic> {
        let root = self.expect_kind(Level2TokenKind::Word, "a root construct")?;
        if self.text(root) != "score" {
            let found = self.text(root).to_owned();
            return Err(Self::structural(
                root,
                format!("expected the `score` root, found `{found}`; level 2 admits no other root"),
            ));
        }
        let open = self.expect_kind(Level2TokenKind::OpenBrace, "`{`")?;
        self.budget.enter_block(open.span)?;
        let ppqn = self.score_body(root.span)?;
        self.budget.leave_block();
        Ok(ExactScoreDocument {
            ppqn,
            master_bars: Vec::new(),
            tracks: Vec::new(),
            source: None,
            loss: Vec::new(),
        })
    }

    /// The `score` body up to its closing brace, returning the one value
    /// this slice reads.
    fn score_body(&mut self, root: Span) -> Result<u16, Diagnostic> {
        let mut ppqn: Option<u16> = None;
        loop {
            let token = self.next().ok_or_else(|| self.unexpected_end())?;
            match token.kind {
                Level2TokenKind::CloseBrace => break,
                Level2TokenKind::Word => {
                    let word = self.text(token).to_owned();
                    if word != "ppqn" {
                        return Err(Self::unknown_word(token, &word));
                    }
                    if ppqn.is_some() {
                        return Err(Diagnostic {
                            code: "SWG0404",
                            span: token.span,
                            message: "`score` takes one `ppqn` word".to_owned(),
                        });
                    }
                    ppqn = Some(self.ppqn_value()?);
                }
                Level2TokenKind::Number | Level2TokenKind::OpenBrace => {
                    let found = self.text(token).to_owned();
                    return Err(Self::structural(
                        token,
                        format!("expected a word, found `{found}`"),
                    ));
                }
            }
        }
        ppqn.ok_or_else(|| Diagnostic {
            code: "SWG0403",
            span: root,
            message: "`score` requires a `ppqn` word".to_owned(),
        })
    }

    /// A word `score` does not take here, said honestly: a word the grammar
    /// defines but this slice has not implemented is not the same failure as
    /// a word nobody has ever defined, and the message says which it is.
    fn unknown_word(token: Level2Token, word: &str) -> Diagnostic {
        let message = if DEFERRED_SCORE_WORDS.contains(&word) {
            format!(
                "`score` takes a `{word}` word, but this build does not parse \
                 it yet (SWG-4A-08); it is refused rather than ignored"
            )
        } else {
            format!("`score` does not take a `{word}` word")
        };
        Diagnostic {
            code: "SWG0401",
            span: token.span,
            message,
        }
    }

    /// `ppqn <n>` — the one scalar this slice reads.
    ///
    /// The scalar layer proper is SWG-4A-07's. The two registry refusals
    /// here are the ones reachable from this single word, and they are
    /// implemented rather than deferred because the alternative is to accept
    /// `ppqn 0960` and `ppqn 0` — text §6.6 declares invalid — into the
    /// accepted set of a level that has not frozen.
    fn ppqn_value(&mut self) -> Result<u16, Diagnostic> {
        let token = self.expect_kind(Level2TokenKind::Number, "a `ppqn` value")?;
        let text = self.text(token);
        if text.len() > 1 && text.starts_with('0') {
            return Err(Diagnostic {
                code: "SWG0505",
                span: token.span,
                message: format!("`{text}` has a leading zero; the canonical spelling has none"),
            });
        }
        let value: u16 = text.parse().map_err(|_| Diagnostic {
            code: "SWG0401",
            span: token.span,
            message: format!("`{text}` does not fit the `ppqn` field"),
        })?;
        if value == 0 {
            return Err(Diagnostic {
                code: "SWG0506",
                span: token.span,
                message: "`ppqn` is the tick resolution and cannot be zero".to_owned(),
            });
        }
        Ok(value)
    }

    /// One document holds one root: anything after the root's closing brace
    /// is a second document trying to arrive inside the first.
    fn expect_end(&self) -> Result<(), Diagnostic> {
        self.peek().map_or(Ok(()), |token| {
            let found = self.text(token).to_owned();
            Err(Self::structural(
                token,
                format!("expected the end of the document, found `{found}`"),
            ))
        })
    }
}
