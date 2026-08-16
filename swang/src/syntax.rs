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

mod ast;
mod diagnostic;
mod format;
mod header;
mod lexer;
mod parser;
mod span;
mod token;

pub use ast::v1::{
    AstError, Export, ExportFormat, Fractalize, Generate, Ident, KernelLiteral, Level, Linearize,
    MapRhythm, PatternDef, Program, Prune, StrategyName, StrategyPolicy, StringLiteral, Unit,
};
pub use diagnostic::Diagnostic;
pub use format::v1::format;
pub use header::{header_level, LANGUAGE_LEVEL};
pub use parser::v1::{parse, parse_with_spans, ProgramSpans};
pub use span::Span;
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
mod tests;
