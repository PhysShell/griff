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

use griff_pattern::{DensityBps, Traversal};

use crate::TailPolicy;

/// The language level this build parses (spec §1.1). Levels are additive-only
/// and never enter any content hash.
pub const LANGUAGE_LEVEL: u32 = 1;

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

/// A parsed Swang program: the pinned language level and the one pattern
/// block the grammar covers. A second `pattern` block is `SWG0401` — multiple
/// patterns have not earned syntax.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Program {
    /// The header's language level.
    pub level: u32,
    /// The program's single pattern definition.
    pub pattern: PatternDef,
}

/// One `pattern <name> { ... }` block. The pipeline is a fixed sequence —
/// every step present, in order; a missing step is `SWG0403`, a step out of
/// order is `SWG0401`. There is deliberately no step list to reorder: the
/// grammar records the one pipeline shape the demo earned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatternDef {
    /// The pattern's name: ASCII `[A-Za-z_][A-Za-z0-9_]*`.
    pub name: String,
    /// The `ascii` kernel literal, exactly as written between the quotes
    /// (`X.X/XX./.XX`). Validated at parse time with the transport's own
    /// codes: `SWG0101` ragged, `SWG0102` foreign cell, `SWG0103`
    /// whitespace, `SWG0307` empty.
    pub kernel: String,
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

/// A rational note value (`1/16`). Both parts are nonzero by parse
/// (`SWG0301`, the transport's own code); whether the unit divides the bar
/// is a build-time question — the bar geometry lives in the seed score, not
/// in the text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unit {
    /// The note value's numerator.
    pub numerator: u64,
    /// The note value's denominator.
    pub denominator: u64,
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
    pub source: String,
    /// Bars to generate; the palette rotates, never stretches (spec §1.11).
    pub bars: u64,
    /// The generation seed — independent of the pruning seed by law.
    pub seed: u64,
    /// Variants per strategy in the ranked set.
    pub candidates: u64,
    /// The explicit strategy policy (spec §3.3, §3.5 law 6).
    pub strategy: StrategyPolicy,
    /// The corpus directory, when the program declares one.
    pub corpus: Option<String>,
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
    pub path: String,
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
    let _ = source;
    unimplemented!("S16 Phase 3: the frozen header pre-parser")
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
    let _ = source;
    unimplemented!("S16 Phase 3: the parser")
}

/// Formats a [`Program`] into its canonical text — the unique fixed point of
/// `format ∘ parse` (spec §3.5 laws 2–3).
#[must_use]
pub fn format(program: &Program) -> String {
    let _ = program;
    unimplemented!("S16 Phase 3: the canonical formatter")
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::missing_assert_message,
    clippy::arithmetic_side_effects,
    clippy::str_to_string
)]
mod tests {
    use griff_pattern::{DensityBps, Traversal};

    use super::{
        format, header_level, parse, Diagnostic, Export, ExportFormat, Fractalize, Generate,
        Linearize, MapRhythm, PatternDef, Program, Prune, StrategyName, StrategyPolicy, Unit,
        LANGUAGE_LEVEL,
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
            level: 1,
            pattern: PatternDef {
                name: "dgd_fractal".to_string(),
                kernel: "X.X/XX./.XX".to_string(),
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
                    unit: Unit {
                        numerator: 1,
                        denominator: 16,
                    },
                    tail: TailPolicy::RestPad,
                },
                generate: Generate {
                    source: "corpus/Dance Gavin Dance - The Robot With Human Hair Part 2.gp5"
                        .to_string(),
                    bars: 8,
                    seed: 42,
                    candidates: 2,
                    strategy: StrategyPolicy::Named(StrategyName::RepeatVariation),
                    corpus: Some("corpus".to_string()),
                },
                export: Export {
                    format: ExportFormat::Midi,
                    path: "dgd_fractal_dense.mid".to_string(),
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
            "swang  1\n",   // two spaces
            " swang 1\n",   // leading whitespace
            "\nswang 1\n",  // leading blank line
            "Swang 1\n",    // wrong case
            "swang 01\n",   // leading zero
            "swang -1\n",   // sign
            "swang 1 \n",   // trailing space
            "swang 1",      // missing EOL
            "swang 1\r",    // CR without LF
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
        let source =
            program_with("|> fractalize depth 1 max_cells 4096").replace("    |> linearize snake\n", "");
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
        let d = first_error(&program_with(
            "|> fractalize depth 1 max_cells 4096 seed 4",
        ));
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
        assert!(span.contains("X.X/XX"), "the span covers the literal: {span}");
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
        let source =
            program_with("|> fractalize depth 1 max_cells 4096").replace("unit 1/16", "unit banana");
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
        let source = program_with("|> fractalize depth 1 max_cells 4096")
            .replace(
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
        let source =
            program_with("|> fractalize depth 1 max_cells 4096").replace("bars 8", "bars 08");
        assert_eq!(first_error(&source).code, "SWG0401");
    }

    // ── the canonical formatter (spec §3.5 laws 2–3) ────────────────────────

    #[test]
    fn the_reference_text_is_the_fixed_point_of_format_parse() {
        let program = parse(REFERENCE).expect("parses");
        assert_eq!(format(&program), REFERENCE, "canonical text formats to itself");
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
}
