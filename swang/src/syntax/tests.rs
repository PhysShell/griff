//! The level-1 grammar tests.

use griff_pattern::{DensityBps, Traversal};

use super::{
    format, header_level, parse, parse_with_source_map, AstError, AstId, Diagnostic, Export,
    ExportFormat, FieldKind, FieldRef, Fractalize, Generate, Ident, KernelLiteral, Level,
    Linearize, MapRhythm, PatternDef, Program, Prune, StrategyName, StrategyPolicy, StringLiteral,
    Unit, LANGUAGE_LEVEL,
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
    let source =
        program_with("|> fractalize depth 1 max_cells 4096").replace("        candidates 2\n", "");
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
    let source =
        program_with("|> fractalize depth 1 max_cells 4096").replace("tail rest_pad", "tail chop");
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
    let source =
        program_with("|> fractalize depth 1 max_cells 4096").replace("export midi", "export wav");
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

// ── the source-location side table (the expand frontend's locations) ────

#[test]
fn the_span_table_slices_the_source_to_the_owning_words() {
    // Characterization, carried across SWG-INF-04's migration from
    // `ProgramSpans` to `SourceMap` unchanged: these are the four locations
    // §3.5 released, and the wider map does not move them.
    let parsed = parse_with_source_map(REFERENCE).expect("the reference parses");
    assert_eq!(
        parsed.value,
        reference_ast(),
        "one parser, two entry points"
    );

    let slice = |span: super::Span| &REFERENCE[span.start as usize..span.end as usize];
    let at = |node: AstId, field: FieldKind| {
        slice(
            parsed
                .source_map
                .field_span(FieldRef::new(node, field))
                .expect("a released location"),
        )
    };
    assert_eq!(
        at(AstId::Pattern(0), FieldKind::Kernel),
        "\"X.X/XX./.XX\"",
        "quotes included"
    );
    assert_eq!(
        at(AstId::MapRhythm(0), FieldKind::Unit),
        "1/16",
        "the value token alone"
    );
    assert_eq!(at(AstId::MapRhythm(0), FieldKind::Tail), "rest_pad");
    assert_eq!(
        at(AstId::Generate(0), FieldKind::Source),
        "\"corpus/Dance Gavin Dance - The Robot With Human Hair Part 2.gp5\""
    );
}

#[test]
fn parse_and_parse_with_source_map_are_one_parser() {
    // Same acceptance and same diagnostics on the same flawed source.
    let flawed = program_with("|> fractalize depth 1 max_cells 4096 density 9500bps");
    assert_eq!(
        parse_with_source_map(&flawed).expect_err("seedless density")[0].code,
        first_error(&flawed).code
    );
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

/// SWG-INF-06: the level-2 resource budget's contract, stated before it
/// exists.
///
/// Spec §5.11 puts every declared input bound at **level 2 only**: level 1's
/// acceptance set is frozen, so a parser-wide bound that rejected a source
/// level 1 accepts would be an observable change to a frozen level. Nothing
/// here may ever be consulted on a level-1 path, and the type is named for
/// that.
///
/// The budget is a live counter, not a post-hoc audit. Checking
/// `tokens.len()` after lexing four million tokens is not a resource gate;
/// it is an obituary written after the allocation. So every axis is admitted
/// *before* the thing it counts is built.
mod level_two_budget {
    use crate::syntax::limits::{
        Level2Budget, Level2ResourceLimits, MAX_DIAGNOSTICS, MAX_NESTING_DEPTH, MAX_SOURCE_BYTES,
        MAX_TOKENS,
    };
    use crate::syntax::Span;

    /// Any location; these tests are about counting, not about pointing.
    const AT: Span = Span { start: 0, end: 1 };

    /// A scaled-down budget, so every boundary is testable at its exact
    /// off-by-one without allocating the declared caps.
    const fn small() -> Level2ResourceLimits {
        Level2ResourceLimits {
            source_bytes: 8,
            tokens: 3,
            nesting_depth: 2,
            diagnostics: 2,
        }
    }

    #[test]
    fn the_declared_limits_are_the_four_numbers_the_spec_names() {
        assert_eq!(MAX_SOURCE_BYTES, 16_777_216, "exactly 16 MiB");
        assert_eq!(MAX_TOKENS, 4_000_000);
        assert_eq!(MAX_NESTING_DEPTH, 64);
        assert_eq!(MAX_DIAGNOSTICS, 256);
        let declared = Level2ResourceLimits::declared();
        assert_eq!(declared.source_bytes, MAX_SOURCE_BYTES);
        assert_eq!(declared.tokens, MAX_TOKENS);
        assert_eq!(declared.nesting_depth, MAX_NESTING_DEPTH);
        assert_eq!(declared.diagnostics, MAX_DIAGNOSTICS);
    }

    #[test]
    fn a_source_of_exactly_the_limit_is_admitted() {
        let budget = Level2Budget::new(small());
        budget
            .admit_source("12345678", AT)
            .expect("eight bytes is exactly the limit");
    }

    #[test]
    fn one_byte_over_the_source_limit_is_refused() {
        let budget = Level2Budget::new(small());
        let refusal = budget
            .admit_source("123456789", AT)
            .expect_err("nine bytes exceeds a limit of eight");
        assert_eq!(refusal.code, "SWG0509");
    }

    #[test]
    fn the_source_limit_counts_utf8_bytes_not_characters() {
        // Three two-byte characters are six bytes, not three.
        let budget = Level2Budget::new(Level2ResourceLimits {
            source_bytes: 5,
            ..small()
        });
        budget
            .admit_source("ééé", AT)
            .expect_err("six bytes exceeds a limit of five");
    }

    #[test]
    fn the_declared_source_limit_is_the_one_actually_consulted() {
        // The scaled budget proves the arithmetic; this proves the real
        // number is wired to it rather than merely declared beside it.
        let budget = Level2Budget::new(Level2ResourceLimits::declared());
        let at_limit = "a".repeat(usize::try_from(MAX_SOURCE_BYTES).expect("16 MiB fits usize"));
        budget
            .admit_source(&at_limit, AT)
            .expect("exactly the declared limit");
        let over = format!("{at_limit}a");
        budget
            .admit_source(&over, AT)
            .expect_err("one byte over the declared limit");
    }

    #[test]
    fn the_token_budget_admits_exactly_its_limit_then_refuses() {
        let mut budget = Level2Budget::new(small());
        for _ in 0..3 {
            budget.admit_token(AT).expect("within the token budget");
        }
        assert_eq!(budget.tokens(), 3);
        let refusal = budget.admit_token(AT).expect_err("the fourth token");
        assert_eq!(refusal.code, "SWG0509");
    }

    #[test]
    fn a_refused_token_is_not_counted() {
        // The lexer asks before it stores. A budget that recorded the token
        // it just refused would drift past its own cap.
        let mut budget = Level2Budget::new(small());
        for _ in 0..3 {
            budget.admit_token(AT).expect("within the token budget");
        }
        budget.admit_token(AT).expect_err("the fourth token");
        budget.admit_token(AT).expect_err("the fifth token");
        assert_eq!(budget.tokens(), 3, "a refusal stores nothing");
    }

    #[test]
    fn the_root_block_is_depth_one() {
        let mut budget = Level2Budget::new(small());
        budget.enter_block(AT).expect("the score root");
        assert_eq!(budget.depth(), 1);
    }

    #[test]
    fn nesting_counts_simultaneously_open_blocks_not_total_blocks() {
        // Two sibling blocks are depth 1 twice, never depth 2. A counter
        // that never decremented would refuse a perfectly flat document.
        let mut budget = Level2Budget::new(small());
        for _ in 0..10 {
            budget.enter_block(AT).expect("a sibling block");
            budget.leave_block();
        }
        assert_eq!(budget.depth(), 0);
    }

    #[test]
    fn the_block_that_would_exceed_the_depth_is_the_one_refused() {
        let mut budget = Level2Budget::new(small());
        budget.enter_block(AT).expect("depth 1");
        budget.enter_block(AT).expect("depth 2");
        let refusal = budget.enter_block(AT).expect_err("depth 3 exceeds two");
        assert_eq!(refusal.code, "SWG0509");
        assert_eq!(budget.depth(), 2, "a refused block was never entered");
    }

    #[test]
    fn the_diagnostic_budget_reserves_its_last_slot_for_the_breach() {
        // §5.11's diagnostic cap is on what one parse attempt *returns*, and
        // the terminal resource diagnostic counts toward it. So a cap of two
        // buys one ordinary diagnostic and the SWG0509 that ends the run —
        // never two ordinary ones and a third that quietly exceeds the cap.
        let mut budget = Level2Budget::new(small());
        budget.admit_diagnostic(AT).expect("the first diagnostic");
        let terminal = budget
            .admit_diagnostic(AT)
            .expect_err("the second would leave no room for the breach");
        assert_eq!(terminal.code, "SWG0509");
        assert_eq!(budget.diagnostics(), 2, "the terminal one is counted");
    }

    #[test]
    fn every_breach_names_its_axis_the_declared_limit_and_what_was_seen() {
        let budget = Level2Budget::new(small());
        let refusal = budget.admit_source("123456789", AT).expect_err("over");
        assert!(refusal.message.contains("source bytes"), "{refusal:?}");
        assert!(refusal.message.contains('8'), "the declared limit");
        assert!(refusal.message.contains('9'), "what was observed");
    }

    #[test]
    fn a_breach_points_where_the_caller_said() {
        let mut budget = Level2Budget::new(small());
        let at = Span { start: 40, end: 44 };
        for _ in 0..3 {
            budget.admit_token(AT).expect("within the budget");
        }
        let refusal = budget.admit_token(at).expect_err("the fourth token");
        assert_eq!(refusal.span, at, "the crossing token, not the whole file");
    }

    #[test]
    fn all_four_axes_share_one_code_because_they_share_one_meaning() {
        let mut budget = Level2Budget::new(Level2ResourceLimits {
            source_bytes: 1,
            tokens: 0,
            nesting_depth: 0,
            diagnostics: 1,
        });
        let codes = [
            budget.admit_token(AT).expect_err("tokens").code,
            budget.enter_block(AT).expect_err("depth").code,
            budget.admit_diagnostic(AT).expect_err("diagnostics").code,
        ];
        for code in codes {
            assert_eq!(code, "SWG0509");
        }
    }
}
