//! The level-1 recursive-descent parser (ADR-0029 §11).

use griff_pattern::{DensityBps, Traversal};

use crate::syntax::ast::v1::{
    AstError, Export, ExportFormat, Fractalize, Generate, Ident, KernelLiteral, Level, Linearize,
    MapRhythm, PatternDef, Program, Prune, StrategyName, StrategyPolicy, StringLiteral, Unit,
};
use crate::syntax::diagnostic::Diagnostic;
use crate::syntax::header::{header_level, HEADER_WINDOW};
use crate::syntax::lexer::lex;
use crate::syntax::span::{span_of, Span};
use crate::syntax::token::{Token, TokenKind};
use crate::TailPolicy;

/// Source locations of the program words an expansion frontend renders
/// diagnostics at (spec §3.5's CLI contract, §1.5's layers).
///
/// This is a side table, not part of the AST: [`Program`] equality and the
/// `parse(format(ast)) == ast` law stay span-free. String-literal spans
/// include their quotes; value spans cover the value token alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProgramSpans {
    /// The quoted `ascii` kernel literal.
    pub kernel: Span,
    /// The `unit` value (`1/16`).
    pub unit: Span,
    /// The `tail` value (`reject` / `rest_pad`).
    pub tail: Span,
    /// The quoted `source` literal.
    pub source: Span,
}

/// [`parse`], additionally returning the [`ProgramSpans`] side table.
///
/// # Errors
/// Exactly [`parse`]'s errors — the two functions are one parser.
pub fn parse_with_spans(source: &str) -> Result<(Program, ProgramSpans), Vec<Diagnostic>> {
    // `header_level` already enforced 1..=LANGUAGE_LEVEL; the map_err is
    // defense in depth, not a reachable path.
    let level = Level::new(header_level(source).map_err(|d| vec![d])?).map_err(|e| {
        vec![Diagnostic {
            code: "SWG0002",
            span: span_of(0, 0),
            message: e.to_string(),
        }]
    })?;
    let body_from = source
        .as_bytes()
        .iter()
        .take(HEADER_WINDOW)
        .position(|&b| b == b'\n')
        .map_or(source.len(), |lf| lf.saturating_add(1));
    let tokens = lex(source, body_from).map_err(|d| vec![d])?;
    let mut parser = Parser {
        tokens,
        pos: 0,
        eof: span_of(source.len(), source.len()),
    };
    let (pattern, spans) = parser.parse_pattern().map_err(|d| vec![d])?;
    Ok((Program { level, pattern }, spans))
}

/// Parses a Swang script into its [`Program`].
///
/// The header is checked first ([`header_level`]); the grammar then covers
/// exactly the earned pipeline. Diagnostics are pure data with byte-offset
/// spans; the returned vector is never empty on `Err`.
///
/// # Errors
/// Every registry code the grammar can raise: the header codes, the kernel
/// codes (`SWG0101`–`SWG0103`, `SWG0307`), the semantic parity codes
/// (`SWG0301` zero/malformed unit, `SWG0303` density without seed,
/// `SWG0308` density out of scale), and the syntax class (`SWG0401`
/// malformed syntax or out-of-range value, `SWG0402` unknown name in a
/// closed word set, `SWG0403` missing required word, `SWG0404` repeated
/// word).
pub fn parse(source: &str) -> Result<Program, Vec<Diagnostic>> {
    parse_with_spans(source).map(|(program, _)| program)
}

// ── parsing ──────────────────────────────────────────────────────────────

/// The pipeline steps, in the one order the grammar covers (spec §3.1).
const STEPS: [&str; 5] = [
    "fractalize",
    "linearize",
    "map_rhythm",
    "generate",
    "export",
];

/// One `|> word args...` pipeline entry, args still raw.
struct PipelineEntry {
    name: String,
    name_span: Span,
    args: Vec<Token>,
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    eof: Span,
}

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn next(&mut self) -> Option<Token> {
        let token = self.tokens.get(self.pos).cloned();
        if token.is_some() {
            self.pos = self.pos.saturating_add(1);
        }
        token
    }

    fn unexpected_end(&self) -> Diagnostic {
        Diagnostic {
            code: "SWG0401",
            span: self.eof,
            message: "unexpected end of input".to_owned(),
        }
    }

    fn expect_word(&mut self, word: &str) -> Result<Token, Diagnostic> {
        let token = self.next().ok_or_else(|| self.unexpected_end())?;
        if token.kind == TokenKind::Word && token.text == word {
            Ok(token)
        } else {
            Err(Diagnostic {
                code: "SWG0401",
                span: token.span,
                message: format!("expected `{word}`, found `{}`", token.text),
            })
        }
    }

    fn expect_kind(&mut self, kind: TokenKind, what: &str) -> Result<Token, Diagnostic> {
        let token = self.next().ok_or_else(|| self.unexpected_end())?;
        if token.kind == kind {
            Ok(token)
        } else {
            Err(Diagnostic {
                code: "SWG0401",
                span: token.span,
                message: format!("expected {what}, found `{}`", token.text),
            })
        }
    }

    /// `pattern <name> { ascii "…" entries* }` and nothing after it.
    fn parse_pattern(&mut self) -> Result<(PatternDef, ProgramSpans), Diagnostic> {
        self.expect_word("pattern")?;
        let name = self.expect_kind(TokenKind::Word, "a pattern name")?;
        self.expect_kind(TokenKind::OpenBrace, "`{`")?;

        let (ascii, kernel_span) = self.parse_ascii()?;
        let entries = self.collect_entries()?;
        let close = self.expect_kind(TokenKind::CloseBrace, "`}`")?;
        if let Some(extra) = self.peek() {
            return Err(Diagnostic {
                code: "SWG0401",
                span: extra.span,
                message: "a program is one pattern block; nothing may follow it".to_owned(),
            });
        }

        let steps = order_entries(entries, close.span)?;
        // `order_entries` proved the canonical order and count.
        let [fractalize_entry, linearize_entry, map_rhythm_entry, generate_entry, export_entry] =
            steps.as_slice()
        else {
            return Err(self.unexpected_end());
        };
        let fractalize = parse_fractalize(fractalize_entry)?;
        let linearize = parse_linearize(linearize_entry)?;
        let (map_rhythm, unit_span, tail_span) = parse_map_rhythm(map_rhythm_entry)?;
        let (generate, source_span) = parse_generate(generate_entry)?;
        let export = parse_export(export_entry)?;

        let name = Ident::new(&name.text).map_err(|e| Diagnostic {
            // The lexer reads exactly the identifier charset; defensive.
            code: "SWG0401",
            span: name.span,
            message: e.to_string(),
        })?;

        Ok((
            PatternDef {
                name,
                kernel: ascii,
                fractalize,
                linearize,
                map_rhythm,
                generate,
                export,
            },
            ProgramSpans {
                kernel: kernel_span,
                unit: unit_span,
                tail: tail_span,
                source: source_span,
            },
        ))
    }

    /// `ascii "<literal>"` — the block's first element.
    fn parse_ascii(&mut self) -> Result<(KernelLiteral, Span), Diagnostic> {
        match self.peek() {
            Some(t) if t.kind == TokenKind::Word && t.text == "ascii" => {
                self.next();
            }
            Some(t) => {
                return Err(Diagnostic {
                    code: "SWG0403",
                    span: t.span,
                    message: "the pattern block begins with its `ascii` literal".to_owned(),
                })
            }
            None => return Err(self.unexpected_end()),
        }
        let literal = self.expect_kind(TokenKind::Str, "a kernel literal")?;
        let kernel = KernelLiteral::new(&literal.text).map_err(|e| match e {
            AstError::InvalidKernel { code, message } => Diagnostic {
                code,
                span: literal.span,
                message,
            },
            other => Diagnostic {
                code: "SWG0401",
                span: literal.span,
                message: other.to_string(),
            },
        })?;
        Ok((kernel, literal.span))
    }

    /// Collects raw `|> word args…` entries up to the pattern's `}`.
    fn collect_entries(&mut self) -> Result<Vec<PipelineEntry>, Diagnostic> {
        let mut entries = Vec::new();
        while matches!(self.peek(), Some(t) if t.kind == TokenKind::Pipe) {
            self.next();
            let name = self.expect_kind(TokenKind::Word, "a pipeline step")?;
            let mut args = Vec::new();
            let mut depth = 0_u32;
            loop {
                match self.peek() {
                    None => return Err(self.unexpected_end()),
                    Some(t) if depth == 0 && t.kind == TokenKind::Pipe => break,
                    Some(t) if depth == 0 && t.kind == TokenKind::CloseBrace => break,
                    Some(t) => {
                        match t.kind {
                            TokenKind::OpenBrace => depth = depth.saturating_add(1),
                            TokenKind::CloseBrace => depth = depth.saturating_sub(1),
                            _ => {}
                        }
                        args.push(self.next().ok_or_else(|| self.unexpected_end())?);
                    }
                }
            }
            entries.push(PipelineEntry {
                name: name.text,
                name_span: name.span,
                args,
            });
        }
        Ok(entries)
    }
}

/// Checks the entries against the canonical sequence: unknown steps and
/// duplicates are `SWG0401`, a missing step is `SWG0403` naming it, a
/// present-but-misplaced step is `SWG0401`. Returns the entries in canonical
/// order (which, by then, is the order they arrived in).
fn order_entries(
    entries: Vec<PipelineEntry>,
    close: Span,
) -> Result<Vec<PipelineEntry>, Diagnostic> {
    for entry in &entries {
        if !STEPS.contains(&entry.name.as_str()) {
            return Err(Diagnostic {
                code: "SWG0401",
                span: entry.name_span,
                message: format!("unknown pipeline step `{}`", entry.name),
            });
        }
    }
    for step in STEPS {
        if !entries.iter().any(|e| e.name == step) {
            return Err(Diagnostic {
                code: "SWG0403",
                span: close,
                message: format!("the pipeline is missing its `{step}` step"),
            });
        }
    }
    for (i, entry) in entries.iter().enumerate() {
        match STEPS.get(i) {
            Some(&expected) if entry.name == expected => {}
            Some(&expected) => {
                return Err(Diagnostic {
                    code: "SWG0401",
                    span: entry.name_span,
                    message: format!(
                        "`{}` arrives out of pipeline order; expected `{expected}`",
                        entry.name
                    ),
                })
            }
            None => {
                return Err(Diagnostic {
                    code: "SWG0401",
                    span: entry.name_span,
                    message: format!("`{}` repeats a pipeline step", entry.name),
                })
            }
        }
    }
    Ok(entries)
}

// ── word-value constructs ────────────────────────────────────────────────

/// A scanned `word value` pair.
type WordValue = (Token, Token);

/// Scans `word value` pairs: every word from `allowed`, none repeated, every
/// word carrying exactly one value token.
fn scan_pairs(
    args: &[Token],
    allowed: &[&str],
    construct: &str,
) -> Result<Vec<WordValue>, Diagnostic> {
    let mut pairs: Vec<WordValue> = Vec::new();
    let mut it = args.iter();
    while let Some(word) = it.next() {
        if word.kind != TokenKind::Word || !allowed.contains(&word.text.as_str()) {
            return Err(Diagnostic {
                code: "SWG0401",
                span: word.span,
                message: format!("`{construct}` does not take a `{}` word", word.text),
            });
        }
        if pairs.iter().any(|(w, _)| w.text == word.text) {
            return Err(Diagnostic {
                code: "SWG0404",
                span: word.span,
                message: format!("the word `{}` repeats within `{construct}`", word.text),
            });
        }
        let value = it.next().ok_or_else(|| Diagnostic {
            code: "SWG0401",
            span: word.span,
            message: format!("the word `{}` names no value", word.text),
        })?;
        pairs.push((word.clone(), value.clone()));
    }
    Ok(pairs)
}

/// A required word that never arrived: `SWG0403` at the construct's name.
fn missing_word(construct: &str, word: &str, at: Span) -> Diagnostic {
    Diagnostic {
        code: "SWG0403",
        span: at,
        message: format!("`{construct}` is missing its required word `{word}`"),
    }
}

fn parse_fractalize(entry: &PipelineEntry) -> Result<Fractalize, Diagnostic> {
    let pairs = scan_pairs(
        &entry.args,
        &["depth", "max_cells", "density", "seed"],
        "fractalize",
    )?;
    let mut depth = None;
    let mut max_cells = None;
    let mut density = None;
    let mut seed = None;
    for (word, value) in &pairs {
        match word.text.as_str() {
            "depth" => depth = Some(int_value::<u8>(value, "depth")?),
            "max_cells" => max_cells = Some(int_value::<u64>(value, "max_cells")?),
            "density" => density = Some((word.span, density_value(value)?)),
            _ => seed = Some((word.span, int_value::<u64>(value, "seed")?)),
        }
    }
    let depth = depth.ok_or_else(|| missing_word("fractalize", "depth", entry.name_span))?;
    let max_cells =
        max_cells.ok_or_else(|| missing_word("fractalize", "max_cells", entry.name_span))?;
    let prune = match (density, seed) {
        (Some((_, density)), Some((_, seed))) => Some(Prune { density, seed }),
        (Some((at, _)), None) => {
            return Err(Diagnostic {
                code: "SWG0303",
                span: at,
                message: "density decay was given without a rhythm seed; pruning must be \
                          explicitly seeded"
                    .to_owned(),
            })
        }
        (None, Some((at, _))) => {
            return Err(Diagnostic {
                code: "SWG0403",
                span: at,
                message: "`seed` names a pruning this fractalize does not declare; `density` \
                          and `seed` are a visible pair"
                    .to_owned(),
            })
        }
        (None, None) => None,
    };
    Ok(Fractalize {
        depth,
        max_cells,
        prune,
    })
}

fn parse_linearize(entry: &PipelineEntry) -> Result<Linearize, Diagnostic> {
    match entry.args.as_slice() {
        [] => Err(missing_word("linearize", "traversal", entry.name_span)),
        [token] => Ok(Linearize {
            traversal: closed_set(
                token,
                &[
                    ("row_major", Traversal::RowMajor),
                    ("snake", Traversal::Snake),
                ],
                "traversal",
            )?,
        }),
        [_, extra, ..] => Err(Diagnostic {
            code: "SWG0401",
            span: extra.span,
            message: "`linearize` takes one traversal and nothing else".to_owned(),
        }),
    }
}

fn parse_map_rhythm(entry: &PipelineEntry) -> Result<(MapRhythm, Span, Span), Diagnostic> {
    let pairs = scan_pairs(&entry.args, &["unit", "tail"], "map_rhythm")?;
    let mut unit = None;
    let mut tail = None;
    for (word, value) in &pairs {
        if word.text == "unit" {
            unit = Some((unit_value(value)?, value.span));
        } else {
            tail = Some((
                closed_set(
                    value,
                    &[
                        ("reject", TailPolicy::Reject),
                        ("rest_pad", TailPolicy::RestPad),
                    ],
                    "tail policy",
                )?,
                value.span,
            ));
        }
    }
    let (unit, unit_span) =
        unit.ok_or_else(|| missing_word("map_rhythm", "unit", entry.name_span))?;
    let (tail, tail_span) =
        tail.ok_or_else(|| missing_word("map_rhythm", "tail", entry.name_span))?;
    Ok((MapRhythm { unit, tail }, unit_span, tail_span))
}

fn parse_generate(entry: &PipelineEntry) -> Result<(Generate, Span), Diagnostic> {
    let block = match entry.args.as_slice() {
        [open, inner @ .., close]
            if open.kind == TokenKind::OpenBrace && close.kind == TokenKind::CloseBrace =>
        {
            inner
        }
        _ => {
            return Err(Diagnostic {
                code: "SWG0401",
                span: entry.name_span,
                message: "`generate` takes a `{ … }` block".to_owned(),
            })
        }
    };
    let pairs = scan_pairs(
        block,
        &["source", "bars", "seed", "candidates", "strategy", "corpus"],
        "generate",
    )?;
    let mut source = None;
    let mut bars = None;
    let mut seed = None;
    let mut candidates = None;
    let mut strategy = None;
    let mut corpus = None;
    for (word, value) in &pairs {
        match word.text.as_str() {
            "source" => source = Some((string_value(value, "source")?, value.span)),
            "bars" => bars = Some(int_value::<u64>(value, "bars")?),
            "seed" => seed = Some(int_value::<u64>(value, "seed")?),
            "candidates" => candidates = Some(int_value::<u64>(value, "candidates")?),
            "strategy" => strategy = Some(strategy_value(value)?),
            _ => corpus = Some(string_value(value, "corpus")?),
        }
    }
    let (source, source_span) =
        source.ok_or_else(|| missing_word("generate", "source", entry.name_span))?;
    Ok((
        Generate {
            source,
            bars: bars.ok_or_else(|| missing_word("generate", "bars", entry.name_span))?,
            seed: seed.ok_or_else(|| missing_word("generate", "seed", entry.name_span))?,
            candidates: candidates
                .ok_or_else(|| missing_word("generate", "candidates", entry.name_span))?,
            strategy: strategy
                .ok_or_else(|| missing_word("generate", "strategy", entry.name_span))?,
            corpus,
        },
        source_span,
    ))
}

fn parse_export(entry: &PipelineEntry) -> Result<Export, Diagnostic> {
    match entry.args.as_slice() {
        [format_token, path] => Ok(Export {
            format: closed_set(
                format_token,
                &[("midi", ExportFormat::Midi)],
                "export format",
            )?,
            path: string_value(path, "export path")?,
        }),
        _ => Err(Diagnostic {
            code: "SWG0401",
            span: entry.name_span,
            message: "`export` takes a format and a path".to_owned(),
        }),
    }
}

// ── values ───────────────────────────────────────────────────────────────

/// A name from a closed word set; anything else is `SWG0402` listing the
/// set.
fn closed_set<T: Copy>(token: &Token, set: &[(&str, T)], what: &str) -> Result<T, Diagnostic> {
    if token.kind == TokenKind::Word {
        if let Some(&(_, value)) = set.iter().find(|(word, _)| *word == token.text) {
            return Ok(value);
        }
    }
    let words: Vec<&str> = set.iter().map(|&(word, _)| word).collect();
    Err(Diagnostic {
        code: "SWG0402",
        span: token.span,
        message: format!(
            "unknown {what} `{}`; the set is {}",
            token.text,
            words.join(" | ")
        ),
    })
}

fn strategy_value(token: &Token) -> Result<StrategyPolicy, Diagnostic> {
    closed_set(
        token,
        &[
            ("auto", StrategyPolicy::Auto),
            (
                "rhythm_copy",
                StrategyPolicy::Named(StrategyName::RhythmCopy),
            ),
            (
                "motif_transpose",
                StrategyPolicy::Named(StrategyName::MotifTranspose),
            ),
            (
                "constrained_walk",
                StrategyPolicy::Named(StrategyName::ConstrainedWalk),
            ),
            (
                "shuffle_motifs",
                StrategyPolicy::Named(StrategyName::ShuffleMotifs),
            ),
            (
                "repeat_variation",
                StrategyPolicy::Named(StrategyName::RepeatVariation),
            ),
        ],
        "strategy",
    )
}

fn string_value(token: &Token, what: &str) -> Result<StringLiteral, Diagnostic> {
    if token.kind == TokenKind::Str {
        // The lexer cannot produce a quote or a newline inside a literal;
        // the map_err is defensive.
        StringLiteral::new(&token.text).map_err(|e| Diagnostic {
            code: "SWG0401",
            span: token.span,
            message: e.to_string(),
        })
    } else {
        Err(Diagnostic {
            code: "SWG0401",
            span: token.span,
            message: format!("{what} takes a quoted string"),
        })
    }
}

/// The one spelling law for every number in the grammar: a leading zero is
/// never canonical (`SWG0401`), no matter which construct holds the digits —
/// the header set the tone (spec §3.2).
fn leading_zero(digits: &str) -> bool {
    digits.len() > 1 && digits.starts_with('0')
}

/// A plain decimal integer: digits only, no leading zeros, no separators.
fn dec_u128(token: &Token, what: &str) -> Result<u128, Diagnostic> {
    let malformed = |message: String| Diagnostic {
        code: "SWG0401",
        span: token.span,
        message,
    };
    if token.kind != TokenKind::NumberLike || !token.text.bytes().all(|b| b.is_ascii_digit()) {
        return Err(malformed(format!(
            "{what} takes a plain decimal integer, found `{}`",
            token.text
        )));
    }
    if leading_zero(&token.text) {
        return Err(malformed(format!("{what} does not take leading zeros")));
    }
    token
        .text
        .parse()
        .map_err(|_| malformed(format!("{what} value `{}` is out of range", token.text)))
}

/// A ranged integer value; out of range is `SWG0401` at the token.
fn int_value<T: TryFrom<u128>>(token: &Token, what: &str) -> Result<T, Diagnostic> {
    T::try_from(dec_u128(token, what)?).map_err(|_| Diagnostic {
        code: "SWG0401",
        span: token.span,
        message: format!("{what} value `{}` is out of range", token.text),
    })
}

/// `<n>bps`, basis points `0..=10000`. A bare or decimal density is
/// `SWG0401`; an out-of-scale one is `SWG0308` (the transport's code).
fn density_value(token: &Token) -> Result<DensityBps, Diagnostic> {
    let digits = if token.kind == TokenKind::NumberLike {
        token.text.strip_suffix("bps")
    } else {
        None
    };
    let Some(digits) = digits.filter(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit()))
    else {
        return Err(Diagnostic {
            code: "SWG0401",
            span: token.span,
            message: format!(
                "density takes basis points with the `bps` suffix, like `9500bps`; found `{}`",
                token.text
            ),
        });
    };
    if leading_zero(digits) {
        return Err(Diagnostic {
            code: "SWG0401",
            span: token.span,
            message: format!("density does not take leading zeros: `{}`", token.text),
        });
    }
    let out_of_scale = || Diagnostic {
        code: "SWG0308",
        span: token.span,
        message: format!("density {digits} bps is outside 0..=10000"),
    };
    let bps: u128 = digits.parse().map_err(|_| out_of_scale())?;
    let narrow = u16::try_from(bps).map_err(|_| out_of_scale())?;
    DensityBps::new(narrow).map_err(|_| out_of_scale())
}

/// A rational note value `a/b`, both parts nonzero decimal integers. Every
/// malformation is `SWG0301` — the unit's own transport code; whether the
/// unit divides the bar stays a build-time question.
fn unit_value(token: &Token) -> Result<Unit, Diagnostic> {
    let malformed = |message: String| Diagnostic {
        code: "SWG0301",
        span: token.span,
        message,
    };
    let parts = if token.kind == TokenKind::NumberLike {
        token.text.split_once('/')
    } else {
        None
    };
    let Some((numerator, denominator)) = parts.filter(|(a, b)| {
        !a.is_empty()
            && !b.is_empty()
            && a.bytes().all(|c| c.is_ascii_digit())
            && b.bytes().all(|c| c.is_ascii_digit())
    }) else {
        return Err(malformed(format!(
            "malformed unit `{}`: expected a note value like 1/16",
            token.text
        )));
    };
    if leading_zero(numerator) || leading_zero(denominator) {
        // The spelling law, not the unit's semantic one: SWG0401, while
        // SWG0301 keeps naming zero parts and malformed shapes (spec §3.2).
        return Err(Diagnostic {
            code: "SWG0401",
            span: token.span,
            message: format!("unit {} does not take leading zeros", token.text),
        });
    }
    let numerator: u64 = numerator
        .parse()
        .map_err(|_| malformed(format!("unit {} is out of range", token.text)))?;
    let denominator: u64 = denominator
        .parse()
        .map_err(|_| malformed(format!("unit {} is out of range", token.text)))?;
    Unit::new(numerator, denominator)
        .map_err(|_| malformed(format!("unit {} has a zero part", token.text)))
}
