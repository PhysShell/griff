//! The level-1 surface AST (spec §3, frozen).

use std::error::Error;
use std::fmt;

use griff_pattern::{DensityBps, Traversal};

use crate::syntax::header::LANGUAGE_LEVEL;
use crate::TailPolicy;

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

/// The one language level this AST spells (spec §5.4: each released level
/// owns its own parser, formatter and tree).
const LEVEL_ONE: u32 = 1;

impl Level {
    /// Validates the level against the one level this AST spells.
    ///
    /// Level 1's AST carries level 1 and nothing else. Validating against
    /// `LANGUAGE_LEVEL` instead would mean that raising the constant — which
    /// SWG-4A-06 did — silently admits a `Program` whose header says
    /// `swang 2` above a `pattern` root: text the formatter emits happily
    /// and both entry points refuse, so `parse(format(ast)) == ast` fails
    /// for an AST a caller can build from public fields. The mixed value is
    /// unconstructible instead of merely unreachable.
    ///
    /// # Errors
    /// [`AstError::UnsupportedLevel`] for any level but 1.
    pub const fn new(level: u32) -> Result<Self, AstError> {
        if level != LEVEL_ONE {
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
