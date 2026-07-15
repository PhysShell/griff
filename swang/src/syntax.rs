//! The Swang surface grammar: header pre-parser, parser, AST, and canonical
//! formatter (S16 Phase 3, `docs/swang/spec.md` §3).
//!
//! The grammar covers **only what the Phase 2 killer demo audibly earned**:
//! one `pattern` block whose pipeline is the fixed sequence
//! `ascii |> fractalize |> linearize |> map_rhythm |> generate |> export`.
//! Phase 3 adds no musical semantics — everything here parses, formats, and
//! diagnoses; expansion and generation stay where Phases 1–2 froze them.
//!
//! # The header pre-parser (spec §1.1, frozen)
//!
//! Every script begins `swang <level>` — one U+0020, a nonzero decimal level
//! of at most nine digits, LF (optionally preceded by one CR). The pre-parser
//! reads at most 64 bytes of the first line and never changes across
//! releases: a byte-order mark is [`SWG0003`], a malformed or missing header
//! is [`SWG0002`], a level newer than [`LANGUAGE_LEVEL`] is [`SWG0001`]
//! naming the supported range. Only the first line is the header; later lines
//! beginning with `swang` are ordinary content for the grammar to judge.
//!
//! # Words, not defaults (spec §3.2, §3.5 law 7)
//!
//! Every construct takes `word value` pairs. Within a construct the words may
//! arrive in any order — the canonical formatter normalizes the order — but
//! none may repeat ([`SWG0404`]) and the required ones may not be omitted
//! ([`SWG0403`]): `max_cells`, `source`, and `candidates` are required words,
//! because the parser invents no defaults over the frozen semantics.
//! `density` and `seed` are a visible pair: `density` without `seed` is
//! [`SWG0303`] — the same code the transport boundary raises — and `seed`
//! without `density` is [`SWG0403`], never an inert flag.
//!
//! # Diagnostics (spec §1.5)
//!
//! [`parse`] returns pure data: every [`Diagnostic`] carries a stable
//! registry code, a byte-offset [`Span`] into the source, and a message.
//! Rendering happens only at the frontend edge. Semantic codes keep their
//! transport numbers (`SWG0101`–`SWG0103`, `SWG0301`, `SWG0303`, `SWG0307`,
//! `SWG0308`); the `04xx` syntax class is born here.
//!
//! # Canonical form (spec §3.5 laws 2–3)
//!
//! [`format`] emits exactly one canonical text per AST: LF newlines, the
//! header, one blank line, the pattern block with four-space pipeline indent
//! and eight-space `generate` fields, canonical word order
//! (`depth max_cells density seed`; `unit tail`;
//! `source bars seed candidates strategy corpus`), no trailing whitespace,
//! one final newline. `format(parse(text))` is idempotent and
//! `parse(format(ast)) == ast`.
//!
//! [`SWG0001`]: Diagnostic
//! [`SWG0002`]: Diagnostic
//! [`SWG0003`]: Diagnostic
//! [`SWG0303`]: Diagnostic
//! [`SWG0403`]: Diagnostic
//! [`SWG0404`]: Diagnostic

use std::error::Error;
use std::fmt;
use std::iter::Peekable;
use std::str::{from_utf8, CharIndices};

use griff_pattern::{DensityBps, Traversal};

use crate::TailPolicy;

/// The language level this build parses (spec §1.1). Levels are additive-only
/// and never enter any content hash.
pub const LANGUAGE_LEVEL: u32 = 1;

/// Why an AST value refused to exist.
///
/// The `parse(format(ast)) == ast` law (spec §3.5 law 3) holds for **every
/// AST the types let you build** — a future lifter constructs programs
/// without a parser in sight — so each value the grammar could not reparse
/// is unrepresentable, and these are the doors it bounces off.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AstError {
    /// Not an ASCII `[A-Za-z_][A-Za-z0-9_]*` identifier.
    InvalidIdent {
        /// The rejected spelling.
        text: String,
    },
    /// The kernel literal fails its own registry law; the code is the
    /// `SWG____` the parser would raise for the same text.
    InvalidKernel {
        /// The registry code (`SWG0101`–`SWG0103`, `SWG0307`).
        code: &'static str,
        /// The flaw, in kernel vocabulary.
        message: String,
    },
    /// A string literal holding a quote or a line break could never lex.
    InvalidStringLiteral {
        /// The rejected content.
        text: String,
    },
    /// A unit part is zero — no note value has a zero side.
    ZeroUnitPart,
    /// Level zero, or newer than [`LANGUAGE_LEVEL`].
    UnsupportedLevel {
        /// The rejected level.
        level: u32,
    },
}

impl fmt::Display for AstError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidIdent { text } => {
                write!(f, "{text:?} is not an ASCII identifier")
            }
            Self::InvalidKernel { code, message } => {
                write!(f, "invalid kernel literal [{code}]: {message}")
            }
            Self::InvalidStringLiteral { text } => write!(
                f,
                "{text:?} cannot live in a string literal (quotes and line \
                 breaks never lex)"
            ),
            Self::ZeroUnitPart => write!(f, "a unit part is zero"),
            Self::UnsupportedLevel { level } => write!(
                f,
                "language level {level} is not supported (1..={LANGUAGE_LEVEL})"
            ),
        }
    }
}

impl Error for AstError {}

/// A pinned language level, valid by construction: nonzero and at most
/// [`LANGUAGE_LEVEL`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Level(u32);

impl Level {
    /// Validates the level against this build's supported range.
    ///
    /// # Errors
    /// [`AstError::UnsupportedLevel`] for zero or a newer level.
    pub const fn new(level: u32) -> Result<Self, AstError> {
        if level == 0 || level > LANGUAGE_LEVEL {
            return Err(AstError::UnsupportedLevel { level });
        }
        Ok(Self(level))
    }

    /// The raw level.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// An ASCII identifier (`[A-Za-z_][A-Za-z0-9_]*`), valid by construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ident(String);

impl Ident {
    /// Validates the spelling.
    ///
    /// # Errors
    /// [`AstError::InvalidIdent`] for anything the lexer would not read as
    /// one word.
    pub fn new(text: &str) -> Result<Self, AstError> {
        let invalid = || AstError::InvalidIdent {
            text: text.to_owned(),
        };
        let mut chars = text.chars();
        let first = chars.next().ok_or_else(invalid)?;
        if !(first.is_ascii_alphabetic() || first == '_') {
            return Err(invalid());
        }
        if !chars.all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Err(invalid());
        }
        Ok(Self(text.to_owned()))
    }

    /// The identifier's text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The content of a double-quoted literal, valid by construction: no `"`,
/// no line breaks — nothing the lexer could not read back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StringLiteral(String);

impl StringLiteral {
    /// Validates the content.
    ///
    /// # Errors
    /// [`AstError::InvalidStringLiteral`] for a quote or a line break.
    pub fn new(text: &str) -> Result<Self, AstError> {
        if text.contains('"') || text.contains('\n') {
            return Err(AstError::InvalidStringLiteral {
                text: text.to_owned(),
            });
        }
        Ok(Self(text.to_owned()))
    }

    /// The literal's content, without quotes.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// An `ascii` kernel literal, valid by construction: it passes exactly the
/// registry checks the parser runs (`SWG0101`–`SWG0103`, `SWG0307`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KernelLiteral(String);

impl KernelLiteral {
    /// Validates the literal with the parser's own kernel laws.
    ///
    /// # Errors
    /// [`AstError::InvalidKernel`] carrying the registry code the parser
    /// would raise for the same text.
    pub fn new(text: &str) -> Result<Self, AstError> {
        match kernel_flaw(text) {
            Some((code, message)) => Err(AstError::InvalidKernel { code, message }),
            None => Ok(Self(text.to_owned())),
        }
    }

    /// The literal's text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A half-open byte range into the source text. Fixed-width offsets by the
/// determinism law (spec §1.2): no platform-sized integers in anything a
/// frontend may serialize.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    /// First byte of the range.
    pub start: u32,
    /// One past the last byte.
    pub end: u32,
}

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

/// A parsed Swang program.
///
/// It carries the pinned language level and the one pattern block the
/// grammar covers; a second `pattern` block is `SWG0401` — multiple patterns
/// have not earned syntax.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Program {
    /// The header's language level.
    pub level: Level,
    /// The program's single pattern definition.
    pub pattern: PatternDef,
}

/// One `pattern <name> { ... }` block.
///
/// The pipeline is a fixed sequence — every step present, in order; a
/// missing step is `SWG0403`, a step out of order is `SWG0401`. There is
/// deliberately no step list to reorder: the grammar records the one
/// pipeline shape the demo earned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatternDef {
    /// The pattern's name.
    pub name: Ident,
    /// The `ascii` kernel literal, exactly as written between the quotes
    /// (`X.X/XX./.XX`). Validated with the transport's own codes:
    /// `SWG0101` ragged, `SWG0102` foreign cell, `SWG0103` whitespace,
    /// `SWG0307` empty.
    pub kernel: KernelLiteral,
    /// `|> fractalize ...`
    pub fractalize: Fractalize,
    /// `|> linearize ...`
    pub linearize: Linearize,
    /// `|> map_rhythm ...`
    pub map_rhythm: MapRhythm,
    /// `|> generate { ... }`
    pub generate: Generate,
    /// `|> export ...`
    pub export: Export,
}

/// `fractalize depth <n> max_cells <n> [density <n>bps seed <n>]`.
///
/// The cell budget is a **required word**: the library ships no default and
/// the language invents none (spec §3.2). Density and seed are a visible
/// pair or absent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fractalize {
    /// Exact expansion depth; doubles as the structural `max_depth`.
    pub depth: u8,
    /// The structural cell budget.
    pub max_cells: u64,
    /// The seeded pruning, when the program asks for one.
    pub prune: Option<Prune>,
}

/// The visible density/seed pair: `density 9500bps seed 4`. The `bps` suffix
/// is mandatory — no bare or decimal densities (spec §3.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Prune {
    /// Density decay in basis points, already range-checked (`SWG0308`).
    pub density: DensityBps,
    /// The pruning seed — independent of the generation seed by law
    /// (spec §1.13).
    pub seed: u64,
}

/// `linearize <traversal>` — the traversal is always explicit (spec §1.9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Linearize {
    /// `row_major` or `snake`; anything else is `SWG0402`.
    pub traversal: Traversal,
}

/// `map_rhythm unit <a>/<b> tail <policy>` — both boundaries always written
/// (spec §1.11); no defaults exist to omit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapRhythm {
    /// The rational note value.
    pub unit: Unit,
    /// `reject` or `rest_pad`; anything else is `SWG0402`.
    pub tail: TailPolicy,
}

/// A rational note value (`1/16`), valid by construction.
///
/// Both parts are nonzero (`SWG0301` at parse, [`AstError::ZeroUnitPart`]
/// in code); whether the unit divides the bar is a build-time question —
/// the bar geometry lives in the seed score, not in the text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unit {
    numerator: u64,
    denominator: u64,
}

impl Unit {
    /// Validates both parts nonzero.
    ///
    /// # Errors
    /// [`AstError::ZeroUnitPart`] when either side is zero.
    pub const fn new(numerator: u64, denominator: u64) -> Result<Self, AstError> {
        if numerator == 0 || denominator == 0 {
            return Err(AstError::ZeroUnitPart);
        }
        Ok(Self {
            numerator,
            denominator,
        })
    }

    /// The note value's numerator.
    #[must_use]
    pub const fn numerator(self) -> u64 {
        self.numerator
    }

    /// The note value's denominator.
    #[must_use]
    pub const fn denominator(self) -> u64 {
        self.denominator
    }
}

/// `generate { source ... bars ... seed ... candidates ... strategy ...
/// [corpus ...] }` — the S6 pass through the shared compiler (spec §1.12).
///
/// A program names **every semantic dependency of its run**: `source` (the
/// seed score — pitch material, range, PPQN, meter, tempo) and `candidates`
/// (variants per strategy) are required words; `corpus`, when given, is a
/// declared dependency, never an ambient one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Generate {
    /// The seed score path.
    pub source: StringLiteral,
    /// Bars to generate; the palette rotates, never stretches (spec §1.11).
    pub bars: u64,
    /// The generation seed — independent of the pruning seed by law.
    pub seed: u64,
    /// Variants per strategy in the ranked set.
    pub candidates: u64,
    /// The explicit strategy policy (spec §3.3, §3.5 law 6).
    pub strategy: StrategyPolicy,
    /// The corpus directory, when the program declares one.
    pub corpus: Option<StringLiteral>,
}

/// The strategy policy is explicit in the AST (spec §3.3): the audible
/// result is decided between the expansion and the ear, and the program says
/// which reading was asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrategyPolicy {
    /// The reranked winner across all strategies — today's behavior.
    Auto,
    /// The top-ranked candidate of one named strategy from the same,
    /// already-ranked set — selection semantics only (spec §3.5 law 5).
    Named(StrategyName),
}

/// The five S6 strategies a program may name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrategyName {
    /// Per-bar rhythm copy.
    RhythmCopy,
    /// Per-bar motif transposition.
    MotifTranspose,
    /// Per-bar constrained walk.
    ConstrainedWalk,
    /// Per-bar motif shuffle.
    ShuffleMotifs,
    /// Holds the palette's first template for the whole take.
    RepeatVariation,
}

/// `export midi "<path>"` — the output edge. The program is the output's
/// single owner: `griff swang build` takes no output flag (spec §3.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Export {
    /// The output format.
    pub format: ExportFormat,
    /// The output path.
    pub path: StringLiteral,
}

/// The output formats a program may name. One entry so far; an unknown name
/// is `SWG0402`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    /// Standard MIDI file.
    Midi,
}

/// The frozen §1.1 pre-parser: reads at most 64 bytes of the first line and
/// returns the pinned language level.
///
/// # Errors
/// `SWG0003` for a byte-order mark, `SWG0002` for a missing or malformed
/// header (wrong casing, wrong spacing, leading zeros, a sign, more than
/// nine digits, a missing EOL, or a first line longer than 64 bytes), and
/// `SWG0001` — naming the supported range — for a level newer than
/// [`LANGUAGE_LEVEL`].
pub fn header_level(source: &str) -> Result<u32, Diagnostic> {
    let bytes = source.as_bytes();
    if bytes.get(..3) == Some(b"\xef\xbb\xbf") {
        return Err(Diagnostic {
            code: "SWG0003",
            span: span_of(0, 3),
            message: "byte-order mark before the header; Swang is UTF-8 without a BOM".to_owned(),
        });
    }
    let window = bytes.len().min(HEADER_WINDOW);
    let malformed = || Diagnostic {
        code: "SWG0002",
        span: span_of(0, window),
        message: "missing or malformed header line; a script begins `swang <level>`".to_owned(),
    };
    let Some(lf) = bytes.iter().take(HEADER_WINDOW).position(|&b| b == b'\n') else {
        return Err(malformed());
    };
    let mut line = bytes.get(..lf).unwrap_or_default();
    if let Some((b'\r', rest)) = line.split_last() {
        line = rest;
    }
    let digits = line.strip_prefix(b"swang ").ok_or_else(malformed)?;
    let first = digits.first().ok_or_else(malformed)?;
    if !(b'1'..=b'9').contains(first) || digits.len() > 9 || !digits.iter().all(u8::is_ascii_digit)
    {
        return Err(malformed());
    }
    let level: u32 = from_utf8(digits)
        .ok()
        .and_then(|d| d.parse().ok())
        .ok_or_else(malformed)?;
    if level > LANGUAGE_LEVEL {
        return Err(Diagnostic {
            code: "SWG0001",
            span: span_of(6, 6_usize.saturating_add(digits.len())),
            message: format!(
                "language level {level} is newer than this build supports (1..={LANGUAGE_LEVEL})"
            ),
        });
    }
    Ok(level)
}

/// The pre-parser reads at most this many bytes of the first line (spec
/// §1.1, frozen).
const HEADER_WINDOW: usize = 64;

/// Builds a [`Span`] from byte indices, saturating into the fixed-width
/// offsets the determinism law demands.
fn span_of(start: usize, end: usize) -> Span {
    Span {
        start: u32::try_from(start).unwrap_or(u32::MAX),
        end: u32::try_from(end).unwrap_or(u32::MAX),
    }
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
    let pattern = parser.parse_pattern().map_err(|d| vec![d])?;
    Ok(Program { level, pattern })
}

/// Formats a [`Program`] into its canonical text — the unique fixed point of
/// `format ∘ parse` (spec §3.5 laws 2–3).
#[must_use]
pub fn format(program: &Program) -> String {
    let p = &program.pattern;
    let prune = p.fractalize.prune.map_or_else(String::new, |prune| {
        format!(" density {}bps seed {}", prune.density.get(), prune.seed)
    });
    let corpus = p
        .generate
        .corpus
        .as_ref()
        .map_or_else(String::new, |corpus| {
            format!("        corpus \"{}\"\n", corpus.as_str())
        });
    format!(
        "swang {level}\n\
         \n\
         pattern {name} {{\n\
         \x20   ascii \"{kernel}\"\n\
         \x20   |> fractalize depth {depth} max_cells {max_cells}{prune}\n\
         \x20   |> linearize {traversal}\n\
         \x20   |> map_rhythm unit {numerator}/{denominator} tail {tail}\n\
         \x20   |> generate {{\n\
         \x20       source \"{source}\"\n\
         \x20       bars {bars}\n\
         \x20       seed {seed}\n\
         \x20       candidates {candidates}\n\
         \x20       strategy {strategy}\n\
         {corpus}\
         \x20   }}\n\
         \x20   |> export {export} \"{path}\"\n\
         }}\n",
        level = program.level.get(),
        name = p.name.as_str(),
        kernel = p.kernel.as_str(),
        depth = p.fractalize.depth,
        max_cells = p.fractalize.max_cells,
        traversal = traversal_word(p.linearize.traversal),
        numerator = p.map_rhythm.unit.numerator(),
        denominator = p.map_rhythm.unit.denominator(),
        tail = tail_word(p.map_rhythm.tail),
        source = p.generate.source.as_str(),
        bars = p.generate.bars,
        seed = p.generate.seed,
        candidates = p.generate.candidates,
        strategy = strategy_word(p.generate.strategy),
        export = export_word(p.export.format),
        path = p.export.path.as_str(),
    )
}

/// The canonical spelling of a traversal.
const fn traversal_word(traversal: Traversal) -> &'static str {
    match traversal {
        Traversal::RowMajor => "row_major",
        Traversal::Snake => "snake",
    }
}

/// The canonical spelling of a tail policy.
const fn tail_word(tail: TailPolicy) -> &'static str {
    match tail {
        TailPolicy::Reject => "reject",
        TailPolicy::RestPad => "rest_pad",
    }
}

/// The canonical spelling of a strategy policy.
const fn strategy_word(strategy: StrategyPolicy) -> &'static str {
    match strategy {
        StrategyPolicy::Auto => "auto",
        StrategyPolicy::Named(StrategyName::RhythmCopy) => "rhythm_copy",
        StrategyPolicy::Named(StrategyName::MotifTranspose) => "motif_transpose",
        StrategyPolicy::Named(StrategyName::ConstrainedWalk) => "constrained_walk",
        StrategyPolicy::Named(StrategyName::ShuffleMotifs) => "shuffle_motifs",
        StrategyPolicy::Named(StrategyName::RepeatVariation) => "repeat_variation",
    }
}

/// The canonical spelling of an export format.
const fn export_word(format: ExportFormat) -> &'static str {
    match format {
        ExportFormat::Midi => "midi",
    }
}

// ── lexing ───────────────────────────────────────────────────────────────

/// One lexeme. `text` is the word/number spelling, or the string literal's
/// content without its quotes; spans always cover the full source lexeme.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Token {
    kind: TokenKind,
    text: String,
    span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TokenKind {
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

/// Lexes `source` from byte `from` on. Whitespace is ASCII only — the
/// determinism law (spec §1.2) keeps Unicode classification out of anything
/// semantics can observe, so a non-ASCII space is `SWG0401`, not a
/// separator.
fn lex(source: &str, from: usize) -> Result<Vec<Token>, Diagnostic> {
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
    fn parse_pattern(&mut self) -> Result<PatternDef, Diagnostic> {
        self.expect_word("pattern")?;
        let name = self.expect_kind(TokenKind::Word, "a pattern name")?;
        self.expect_kind(TokenKind::OpenBrace, "`{`")?;

        let ascii = self.parse_ascii()?;
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
        let map_rhythm = parse_map_rhythm(map_rhythm_entry)?;
        let generate = parse_generate(generate_entry)?;
        let export = parse_export(export_entry)?;

        let name = Ident::new(&name.text).map_err(|e| Diagnostic {
            // The lexer reads exactly the identifier charset; defensive.
            code: "SWG0401",
            span: name.span,
            message: e.to_string(),
        })?;

        Ok(PatternDef {
            name,
            kernel: ascii,
            fractalize,
            linearize,
            map_rhythm,
            generate,
            export,
        })
    }

    /// `ascii "<literal>"` — the block's first element.
    fn parse_ascii(&mut self) -> Result<KernelLiteral, Diagnostic> {
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
        KernelLiteral::new(&literal.text).map_err(|e| match e {
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
        })
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

fn parse_map_rhythm(entry: &PipelineEntry) -> Result<MapRhythm, Diagnostic> {
    let pairs = scan_pairs(&entry.args, &["unit", "tail"], "map_rhythm")?;
    let mut unit = None;
    let mut tail = None;
    for (word, value) in &pairs {
        if word.text == "unit" {
            unit = Some(unit_value(value)?);
        } else {
            tail = Some(closed_set(
                value,
                &[
                    ("reject", TailPolicy::Reject),
                    ("rest_pad", TailPolicy::RestPad),
                ],
                "tail policy",
            )?);
        }
    }
    Ok(MapRhythm {
        unit: unit.ok_or_else(|| missing_word("map_rhythm", "unit", entry.name_span))?,
        tail: tail.ok_or_else(|| missing_word("map_rhythm", "tail", entry.name_span))?,
    })
}

fn parse_generate(entry: &PipelineEntry) -> Result<Generate, Diagnostic> {
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
            "source" => source = Some(string_value(value, "source")?),
            "bars" => bars = Some(int_value::<u64>(value, "bars")?),
            "seed" => seed = Some(int_value::<u64>(value, "seed")?),
            "candidates" => candidates = Some(int_value::<u64>(value, "candidates")?),
            "strategy" => strategy = Some(strategy_value(value)?),
            _ => corpus = Some(string_value(value, "corpus")?),
        }
    }
    Ok(Generate {
        source: source.ok_or_else(|| missing_word("generate", "source", entry.name_span))?,
        bars: bars.ok_or_else(|| missing_word("generate", "bars", entry.name_span))?,
        seed: seed.ok_or_else(|| missing_word("generate", "seed", entry.name_span))?,
        candidates: candidates
            .ok_or_else(|| missing_word("generate", "candidates", entry.name_span))?,
        strategy: strategy.ok_or_else(|| missing_word("generate", "strategy", entry.name_span))?,
        corpus,
    })
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

// ── the kernel literal ───────────────────────────────────────────────────

/// The transport's own kernel checks, in the transport's own order:
/// whitespace (`SWG0103`), empty rows (`SWG0307`), shape (`SWG0101`), cells
/// (`SWG0102`). One validation path serves the parser and
/// [`KernelLiteral::new`] alike.
fn kernel_flaw(literal: &str) -> Option<(&'static str, String)> {
    if literal
        .chars()
        .any(|c| matches!(c, ' ' | '\t' | '\r' | '\n'))
    {
        return Some((
            "SWG0103",
            "whitespace inside the kernel literal; rows are separated by `/` alone".to_owned(),
        ));
    }
    let rows: Vec<&str> = literal.split('/').collect();
    if rows.iter().any(|row| row.is_empty()) {
        return Some(("SWG0307", "empty kernel literal or empty row".to_owned()));
    }
    let expected = rows.first().map_or(0, |row| row.chars().count());
    for (index, row) in rows.iter().enumerate() {
        let got = row.chars().count();
        if got != expected {
            return Some((
                "SWG0101",
                format!("ragged kernel: row {index} has {got} cells, expected {expected}"),
            ));
        }
    }
    for (index, row) in rows.iter().enumerate() {
        if let Some((col, cell)) = row.chars().enumerate().find(|&(_, c)| c != 'X' && c != '.') {
            return Some((
                "SWG0102",
                format!("invalid kernel cell {cell:?} at row {index}, col {col}: only `X` and `.`"),
            ));
        }
    }
    None
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::string_slice,
    clippy::missing_assert_message,
    clippy::arithmetic_side_effects,
    clippy::str_to_string
)]
mod tests {
    use griff_pattern::{DensityBps, Traversal};

    use super::{
        format, header_level, parse, AstError, Diagnostic, Export, ExportFormat, Fractalize,
        Generate, Ident, KernelLiteral, Level, Linearize, MapRhythm, PatternDef, Program, Prune,
        StrategyName, StrategyPolicy, StringLiteral, Unit, LANGUAGE_LEVEL,
    };
    use crate::TailPolicy;

    /// The spec §3.1 reference program, byte-for-byte. This text is
    /// canonical: `format(parse(REFERENCE)) == REFERENCE`.
    const REFERENCE: &str = r#"swang 1

pattern dgd_fractal {
    ascii "X.X/XX./.XX"
    |> fractalize depth 1 max_cells 4096 density 9500bps seed 4
    |> linearize snake
    |> map_rhythm unit 1/16 tail rest_pad
    |> generate {
        source "corpus/Dance Gavin Dance - The Robot With Human Hair Part 2.gp5"
        bars 8
        seed 42
        candidates 2
        strategy repeat_variation
        corpus "corpus"
    }
    |> export midi "dgd_fractal_dense.mid"
}
"#;

    /// The reference program's AST, constructed literally.
    fn reference_ast() -> Program {
        Program {
            level: Level::new(1).expect("this build's level"),
            pattern: PatternDef {
                name: Ident::new("dgd_fractal").expect("a name"),
                kernel: KernelLiteral::new("X.X/XX./.XX").expect("the spec kernel"),
                fractalize: Fractalize {
                    depth: 1,
                    max_cells: 4096,
                    prune: Some(Prune {
                        density: DensityBps::new(9500).expect("9500 is in scale"),
                        seed: 4,
                    }),
                },
                linearize: Linearize {
                    traversal: Traversal::Snake,
                },
                map_rhythm: MapRhythm {
                    unit: Unit::new(1, 16).expect("a sixteenth"),
                    tail: TailPolicy::RestPad,
                },
                generate: Generate {
                    source: StringLiteral::new(
                        "corpus/Dance Gavin Dance - The Robot With Human Hair Part 2.gp5",
                    )
                    .expect("a path"),
                    bars: 8,
                    seed: 42,
                    candidates: 2,
                    strategy: StrategyPolicy::Named(StrategyName::RepeatVariation),
                    corpus: Some(StringLiteral::new("corpus").expect("a path")),
                },
                export: Export {
                    format: ExportFormat::Midi,
                    path: StringLiteral::new("dgd_fractal_dense.mid").expect("a path"),
                },
            },
        }
    }

    /// A minimal valid program around one replaceable pipeline line.
    fn program_with(fractalize_line: &str) -> String {
        format!(
            r#"swang 1

pattern p {{
    ascii "X.X/XX./.XX"
    {fractalize_line}
    |> linearize snake
    |> map_rhythm unit 1/16 tail rest_pad
    |> generate {{
        source "seed.gp5"
        bars 8
        seed 42
        candidates 2
        strategy auto
    }}
    |> export midi "out.mid"
}}
"#
        )
    }

    /// The same program around a replaceable kernel literal.
    fn program_with_kernel(kernel: &str) -> String {
        program_with("|> fractalize depth 1 max_cells 4096").replace("X.X/XX./.XX", kernel)
    }

    fn first_error(source: &str) -> Diagnostic {
        parse(source).expect_err("this source must not parse")[0].clone()
    }

    // ── the frozen header pre-parser (spec §1.1) ─────────────────────────────

    #[test]
    fn the_frozen_header_form_pins_the_level() {
        assert_eq!(header_level("swang 1\nrest").expect("frozen form"), 1);
        assert_eq!(LANGUAGE_LEVEL, 1, "this build parses level 1");
    }

    #[test]
    fn a_crlf_header_is_accepted() {
        assert_eq!(header_level("swang 1\r\nrest").expect("CR before LF"), 1);
    }

    #[test]
    fn a_byte_order_mark_is_swg0003_never_skipped() {
        let d = header_level("\u{feff}swang 1\n").expect_err("BOM");
        assert_eq!(d.code, "SWG0003");
        assert_eq!((d.span.start, d.span.end), (0, 3), "the UTF-8 BOM bytes");
    }

    #[test]
    fn malformed_headers_are_swg0002() {
        for source in [
            "",
            "\n",
            "pattern p {}\n",
            "swang1\n",
            "swang  1\n",         // two spaces
            " swang 1\n",         // leading whitespace
            "\nswang 1\n",        // leading blank line
            "Swang 1\n",          // wrong case
            "swang 01\n",         // leading zero
            "swang -1\n",         // sign
            "swang 1 \n",         // trailing space
            "swang 1",            // missing EOL
            "swang 1\r",          // CR without LF
            "swang 1234567890\n", // ten digits
        ] {
            let d = header_level(source).expect_err(source);
            assert_eq!(d.code, "SWG0002", "{source:?}");
        }
    }

    #[test]
    fn the_pre_parser_reads_at_most_64_bytes() {
        // No EOL within the first 64 bytes: rejected without scanning on.
        let long_first_line = format!("swang 1 {}\n", "x".repeat(100));
        let d = header_level(&long_first_line).expect_err("first line too long");
        assert_eq!(d.code, "SWG0002");
    }

    #[test]
    fn a_newer_level_is_swg0001_naming_the_supported_range() {
        let d = header_level("swang 2\n").expect_err("newer than this build");
        assert_eq!(d.code, "SWG0001");
        assert!(
            d.message.contains('1'),
            "the message names the supported range: {}",
            d.message
        );
    }

    #[test]
    fn a_later_swang_line_is_content_not_header() {
        // Only the first line is the header; a later `swang 1` is ordinary
        // content for the grammar to judge — here, a structural violation,
        // never SWG0002.
        let source = "swang 1\n\nswang 1\n";
        let d = first_error(source);
        assert_eq!(d.code, "SWG0401");
    }

    // ── the reference program (spec §3.1) ────────────────────────────────────

    #[test]
    fn the_reference_program_parses_and_the_strategy_is_explicit_in_the_ast() {
        // Law 6: the strategy policy is present in the AST explicitly.
        let program = parse(REFERENCE).expect("the reference program parses");
        assert_eq!(program, reference_ast());
        assert_eq!(
            program.pattern.generate.strategy,
            StrategyPolicy::Named(StrategyName::RepeatVariation)
        );
    }

    #[test]
    fn strategy_auto_is_a_distinct_policy() {
        let source = REFERENCE.replace("strategy repeat_variation", "strategy auto");
        let program = parse(&source).expect("auto parses");
        assert_eq!(program.pattern.generate.strategy, StrategyPolicy::Auto);
    }

    #[test]
    fn corpus_is_the_one_optional_word() {
        let source = REFERENCE.replace("        corpus \"corpus\"\n", "");
        let program = parse(&source).expect("corpus is optional");
        assert_eq!(program.pattern.generate.corpus, None);
    }

    // ── required words (spec §3.2, §3.5 law 7) ──────────────────────────────

    #[test]
    fn fractalize_without_max_cells_is_swg0403() {
        let d = first_error(&program_with("|> fractalize depth 1"));
        assert_eq!(d.code, "SWG0403");
        assert!(d.message.contains("max_cells"), "{}", d.message);
    }

    #[test]
    fn generate_without_source_is_swg0403() {
        let source = program_with("|> fractalize depth 1 max_cells 4096")
            .replace("        source \"seed.gp5\"\n", "");
        let d = first_error(&source);
        assert_eq!(d.code, "SWG0403");
        assert!(d.message.contains("source"), "{}", d.message);
    }

    #[test]
    fn generate_without_candidates_is_swg0403() {
        let source = program_with("|> fractalize depth 1 max_cells 4096")
            .replace("        candidates 2\n", "");
        let d = first_error(&source);
        assert_eq!(d.code, "SWG0403");
        assert!(d.message.contains("candidates"), "{}", d.message);
    }

    #[test]
    fn map_rhythm_without_unit_or_tail_is_swg0403() {
        let no_tail = program_with("|> fractalize depth 1 max_cells 4096")
            .replace("unit 1/16 tail rest_pad", "unit 1/16");
        assert_eq!(first_error(&no_tail).code, "SWG0403");

        let no_unit = program_with("|> fractalize depth 1 max_cells 4096")
            .replace("unit 1/16 tail rest_pad", "tail rest_pad");
        assert_eq!(first_error(&no_unit).code, "SWG0403");
    }

    #[test]
    fn a_missing_pipeline_step_is_swg0403() {
        let source = program_with("|> fractalize depth 1 max_cells 4096")
            .replace("    |> linearize snake\n", "");
        let d = first_error(&source);
        assert_eq!(d.code, "SWG0403");
        assert!(d.message.contains("linearize"), "{}", d.message);
    }

    // ── the visible pair (spec §1.13, §3.2) ─────────────────────────────────

    #[test]
    fn density_without_seed_is_swg0303_the_transport_code() {
        // Law 4: the semantic code and its number survive the grammar.
        let d = first_error(&program_with(
            "|> fractalize depth 1 max_cells 4096 density 9500bps",
        ));
        assert_eq!(d.code, "SWG0303");
    }

    #[test]
    fn seed_without_density_is_swg0403_never_an_inert_word() {
        // The transport tolerated an inert --rhythm-seed; the grammar
        // deliberately rejects the form as non-canonical (law 1's scope).
        let d = first_error(&program_with("|> fractalize depth 1 max_cells 4096 seed 4"));
        assert_eq!(d.code, "SWG0403");
        assert!(d.message.contains("density"), "{}", d.message);
    }

    // ── kernel literal parity (spec §1.6, transport codes) ──────────────────

    #[test]
    fn a_ragged_kernel_is_swg0101_at_the_literal() {
        let source = program_with_kernel("X.X/XX");
        let d = first_error(&source);
        assert_eq!(d.code, "SWG0101");
        let span = &source[d.span.start as usize..d.span.end as usize];
        assert!(
            span.contains("X.X/XX"),
            "the span covers the literal: {span}"
        );
    }

    #[test]
    fn a_foreign_cell_is_swg0102() {
        assert_eq!(first_error(&program_with_kernel("X.O")).code, "SWG0102");
    }

    #[test]
    fn whitespace_inside_the_literal_is_swg0103() {
        assert_eq!(first_error(&program_with_kernel("X. X")).code, "SWG0103");
    }

    #[test]
    fn an_empty_kernel_literal_is_swg0307() {
        assert_eq!(first_error(&program_with_kernel("")).code, "SWG0307");
        assert_eq!(first_error(&program_with_kernel("X//X")).code, "SWG0307");
    }

    // ── semantic parity codes in the grammar ────────────────────────────────

    #[test]
    fn density_out_of_scale_is_swg0308() {
        let d = first_error(&program_with(
            "|> fractalize depth 1 max_cells 4096 density 20000bps seed 4",
        ));
        assert_eq!(d.code, "SWG0308");
    }

    #[test]
    fn a_bare_density_without_the_bps_suffix_is_swg0401() {
        let d = first_error(&program_with(
            "|> fractalize depth 1 max_cells 4096 density 9500 seed 4",
        ));
        assert_eq!(d.code, "SWG0401");
    }

    #[test]
    fn a_zero_unit_part_is_swg0301() {
        for unit in ["0/16", "1/0"] {
            let source = program_with("|> fractalize depth 1 max_cells 4096")
                .replace("unit 1/16", &format!("unit {unit}"));
            assert_eq!(first_error(&source).code, "SWG0301", "{unit}");
        }
    }

    #[test]
    fn a_malformed_unit_is_swg0301() {
        let source = program_with("|> fractalize depth 1 max_cells 4096")
            .replace("unit 1/16", "unit banana");
        assert_eq!(first_error(&source).code, "SWG0301");
    }

    // ── closed word sets (SWG0402) ──────────────────────────────────────────

    #[test]
    fn an_unknown_traversal_is_swg0402() {
        let source = program_with("|> fractalize depth 1 max_cells 4096")
            .replace("linearize snake", "linearize spiral");
        assert_eq!(first_error(&source).code, "SWG0402");
    }

    #[test]
    fn an_unknown_tail_policy_is_swg0402() {
        let source = program_with("|> fractalize depth 1 max_cells 4096")
            .replace("tail rest_pad", "tail chop");
        assert_eq!(first_error(&source).code, "SWG0402");
    }

    #[test]
    fn an_unknown_strategy_is_swg0402() {
        let source = program_with("|> fractalize depth 1 max_cells 4096")
            .replace("strategy auto", "strategy banana");
        assert_eq!(first_error(&source).code, "SWG0402");
    }

    #[test]
    fn every_named_strategy_parses() {
        for (word, name) in [
            ("rhythm_copy", StrategyName::RhythmCopy),
            ("motif_transpose", StrategyName::MotifTranspose),
            ("constrained_walk", StrategyName::ConstrainedWalk),
            ("shuffle_motifs", StrategyName::ShuffleMotifs),
            ("repeat_variation", StrategyName::RepeatVariation),
        ] {
            let source = program_with("|> fractalize depth 1 max_cells 4096")
                .replace("strategy auto", &format!("strategy {word}"));
            let program = parse(&source).expect(word);
            assert_eq!(
                program.pattern.generate.strategy,
                StrategyPolicy::Named(name)
            );
        }
    }

    #[test]
    fn an_unknown_export_format_is_swg0402() {
        let source = program_with("|> fractalize depth 1 max_cells 4096")
            .replace("export midi", "export wav");
        assert_eq!(first_error(&source).code, "SWG0402");
    }

    // ── structure (SWG0401, SWG0404) ────────────────────────────────────────

    #[test]
    fn a_repeated_word_is_swg0404() {
        let d = first_error(&program_with(
            "|> fractalize depth 1 depth 2 max_cells 4096",
        ));
        assert_eq!(d.code, "SWG0404");
    }

    #[test]
    fn a_second_pattern_block_is_swg0401() {
        let one = program_with("|> fractalize depth 1 max_cells 4096");
        let body = one
            .strip_prefix("swang 1\n\n")
            .expect("the fixture starts with the header");
        let two = format!("swang 1\n\n{body}\n{body}");
        assert_eq!(first_error(&two).code, "SWG0401");
    }

    #[test]
    fn a_step_out_of_pipeline_order_is_swg0401() {
        let source = program_with("|> fractalize depth 1 max_cells 4096").replace(
            "    |> linearize snake\n    |> map_rhythm unit 1/16 tail rest_pad\n",
            "    |> map_rhythm unit 1/16 tail rest_pad\n    |> linearize snake\n",
        );
        assert_eq!(first_error(&source).code, "SWG0401");
    }

    #[test]
    fn an_out_of_range_value_is_swg0401() {
        // depth is a u8 by the frozen budget contract.
        let d = first_error(&program_with("|> fractalize depth 300 max_cells 4096"));
        assert_eq!(d.code, "SWG0401");
    }

    #[test]
    fn a_leading_zero_is_swg0401_everywhere_not_only_in_the_header() {
        // *Everywhere* means everywhere: plain integers, the bps-suffixed
        // density, and both unit parts — a spelling law, not a value law
        // (`SWG0301` stays the unit's semantic code). The transport
        // tolerated 01/16 because u64 parsing normalized it; the grammar
        // rejects the spelling and claims no parity for it (#118 review).
        let base = program_with("|> fractalize depth 1 max_cells 4096");
        let cases = [
            base.replace("bars 8", "bars 08"),
            program_with("|> fractalize depth 01 max_cells 4096"),
            program_with("|> fractalize depth 1 max_cells 04096"),
            program_with("|> fractalize depth 1 max_cells 4096 density 09500bps seed 4"),
            program_with("|> fractalize depth 1 max_cells 4096 density 9500bps seed 04"),
            base.replace("unit 1/16", "unit 01/16"),
            base.replace("unit 1/16", "unit 1/016"),
        ];
        for source in &cases {
            assert_eq!(first_error(source).code, "SWG0401", "{source}");
        }
        // A lone zero is not a leading zero: 0bps is a valid density.
        parse(&program_with(
            "|> fractalize depth 1 max_cells 4096 density 0bps seed 4",
        ))
        .expect("a zero density prunes everything but spells canonically");
    }

    // ── the canonical formatter (spec §3.5 laws 2–3) ────────────────────────

    #[test]
    fn the_reference_text_is_the_fixed_point_of_format_parse() {
        let program = parse(REFERENCE).expect("parses");
        assert_eq!(
            format(&program),
            REFERENCE,
            "canonical text formats to itself"
        );
    }

    #[test]
    fn format_normalizes_word_order_whitespace_and_layout() {
        // Same program, scrambled: word order, indentation, blank lines,
        // and a single-line generate block. One canonical text comes out.
        let messy = "swang 1\n\n\npattern   dgd_fractal {\n  ascii \"X.X/XX./.XX\"\n      |> fractalize max_cells 4096 seed 4 density 9500bps depth 1\n  |> linearize snake\n    |> map_rhythm tail rest_pad unit 1/16\n  |> generate { bars 8 source \"corpus/Dance Gavin Dance - The Robot With Human Hair Part 2.gp5\" strategy repeat_variation seed 42 corpus \"corpus\" candidates 2 }\n  |> export midi \"dgd_fractal_dense.mid\"\n}\n";
        let program = parse(messy).expect("scrambled word order still parses");
        assert_eq!(program, reference_ast());
        let formatted = format(&program);
        assert_eq!(formatted, REFERENCE, "one canonical text per program");

        // Idempotence: fmt(fmt(s)) == fmt(s).
        let reparsed = parse(&formatted).expect("canonical text parses");
        assert_eq!(format(&reparsed), formatted);
    }

    // ── the AST is valid by construction (#118 review) ──────────────────────

    #[test]
    fn the_ast_refuses_values_the_grammar_could_not_reparse() {
        // Accept the canonical forms...
        assert_eq!(
            Ident::new("dgd_fractal").expect("valid").as_str(),
            "dgd_fractal"
        );
        assert_eq!(
            Ident::new("_").expect("an underscore is a name").as_str(),
            "_"
        );
        assert_eq!(
            KernelLiteral::new("X.X/XX./.XX").expect("valid").as_str(),
            "X.X/XX./.XX"
        );
        assert_eq!(
            StringLiteral::new("with spaces/and slashes.gp5")
                .expect("valid")
                .as_str(),
            "with spaces/and slashes.gp5"
        );
        assert_eq!(
            StringLiteral::new("").expect("empty is lexable").as_str(),
            ""
        );
        let unit = Unit::new(3, 7).expect("odd but nonzero");
        assert_eq!((unit.numerator(), unit.denominator()), (3, 7));
        assert_eq!(Level::new(1).expect("this build's level").get(), 1);

        // ...and bounce everything format() could emit but parse() would
        // refuse or reread differently.
        Ident::new("not a name").expect_err("spaces never lex as one word");
        Ident::new("").expect_err("an empty name");
        Ident::new("1abc").expect_err("a digit starts a number, not a name");
        Ident::new("имя").expect_err("ASCII only — the determinism law");
        assert_eq!(
            KernelLiteral::new("X.X/XX").expect_err("ragged"),
            AstError::InvalidKernel {
                code: "SWG0101",
                message: "ragged kernel: row 1 has 2 cells, expected 3".to_string(),
            },
            "the constructor speaks the parser's own registry"
        );
        KernelLiteral::new("X.O").expect_err("foreign cell");
        KernelLiteral::new("").expect_err("empty literal");
        StringLiteral::new("a\"b.gp5").expect_err("a quote would cut the literal short");
        StringLiteral::new("a\nb").expect_err("a newline never lexes");
        Unit::new(0, 16).expect_err("a zero numerator");
        Unit::new(1, 0).expect_err("a zero denominator");
        assert_eq!(
            Level::new(0).expect_err("levels are nonzero"),
            AstError::UnsupportedLevel { level: 0 }
        );
        Level::new(LANGUAGE_LEVEL + 1).expect_err("newer than this build supports");
    }

    #[test]
    fn parse_format_roundtrips_the_ast() {
        // Law 3, on an AST that exercises the optional branches the
        // reference does not: no pruning, auto strategy, no corpus.
        let mut program = reference_ast();
        program.pattern.fractalize.prune = None;
        program.pattern.generate.strategy = StrategyPolicy::Auto;
        program.pattern.generate.corpus = None;
        let roundtripped = parse(&format(&program)).expect("formatted text parses");
        assert_eq!(roundtripped, program);
    }

    #[test]
    fn parse_format_roundtrips_any_constructible_program() {
        // The law's whole point (#118 review): it holds for every AST the
        // types let exist — this one was never near a parser, and it is
        // deliberately awkward everywhere the types allow awkward.
        let program = Program {
            level: Level::new(1).expect("level"),
            pattern: PatternDef {
                name: Ident::new("_").expect("an underscore is a name"),
                kernel: KernelLiteral::new("X").expect("one cell is a kernel"),
                fractalize: Fractalize {
                    depth: 0,
                    max_cells: 1,
                    prune: None,
                },
                linearize: Linearize {
                    traversal: Traversal::RowMajor,
                },
                map_rhythm: MapRhythm {
                    unit: Unit::new(3, 7).expect("odd but nonzero"),
                    tail: TailPolicy::Reject,
                },
                generate: Generate {
                    source: StringLiteral::new("").expect("empty is lexable"),
                    bars: 0,
                    seed: u64::MAX,
                    candidates: 0,
                    strategy: StrategyPolicy::Auto,
                    corpus: None,
                },
                export: Export {
                    format: ExportFormat::Midi,
                    path: StringLiteral::new("out with spaces.mid").expect("a path"),
                },
            },
        };
        let roundtripped = parse(&format(&program)).expect("formatted text parses");
        assert_eq!(roundtripped, program);
    }
}
