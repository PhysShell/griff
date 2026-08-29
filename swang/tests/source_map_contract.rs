//! SWG-INF-04: the source map's contract, stated before it exists.
//!
//! `ProgramSpans` located four words because four were all the expansion
//! frontend needed. That is a special case wearing a struct: every other
//! value in a program is unlocatable, and each new one would have meant
//! another field. This suite states the general replacement:
//!
//! ```text
//! source -> parse_with_source_map -> Parsed<Program> { value, source_map }
//! ```
//!
//! Three things it is emphatically not. It is not part of the AST — a
//! `Program` built in memory still formats and reparses with no source text
//! anywhere. It is not a licence to improve frozen level-1 diagnostics,
//! which keep the spans §3.5 already released. And its `AstId`s are not
//! persistent identities: they are parse-local handles, stable across
//! whitespace and legal word reordering, and Phase 4C still owns the
//! question of identity across edits.

// Reason: integration-test code. `unwrap`/`expect`/`panic` abort loudly with
// a clear message, which is exactly what a test harness wants.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::missing_assert_message,
    clippy::indexing_slicing,
    clippy::string_slice
)]

use griff_swang::syntax::{
    format, parse, parse_with_source_map, AstId, Export, FieldKind, FieldRef, Fractalize, Generate,
    Ident, KernelLiteral, Level, Linearize, MapRhythm, PatternDef, Program, Prune, Span,
    StrategyName, StrategyPolicy, StringLiteral, Unit,
};

/// The spec §3.1 reference program — every optional word present, so the
/// census sees the widest level-1 shape there is.
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

/// The same program with neither pruning nor a corpus — the narrowest shape.
const MINIMAL: &str = r#"swang 1

pattern p {
    ascii "X.X/XX./.XX"
    |> fractalize depth 1 max_cells 4096
    |> linearize snake
    |> map_rhythm unit 1/16 tail rest_pad
    |> generate {
        source "seed.gp5"
        bars 8
        seed 42
        candidates 2
        strategy auto
    }
    |> export midi "out.mid"
}
"#;

fn slice(source: &str, span: Span) -> &str {
    &source[span.start as usize..span.end as usize]
}

// ── A. the full reference census ────────────────────────────────────────────

/// Every level-1 AST field, classified: it either carries a **field span**
/// of its own, or is a composite located by its child's **node span**.
///
/// The destructuring is exhaustive and uses no `..` on purpose. Adding a
/// field to any level-1 AST struct stops this compiling, which forces the
/// choice to be made deliberately rather than defaulted into "unlocatable".
fn classify(program: &Program) -> (Vec<FieldRef>, Vec<AstId>) {
    let mut fields = Vec::new();
    let mut nodes = vec![AstId::Program(0)];

    let Program { level, pattern } = program;
    let _: &Level = level;
    fields.push(FieldRef::new(AstId::Program(0), FieldKind::Level));
    // `pattern` is composite: located by the Pattern node span.

    let PatternDef {
        name,
        kernel,
        fractalize,
        linearize,
        map_rhythm,
        generate,
        export,
    } = pattern;
    let _: &Ident = name;
    let _: &KernelLiteral = kernel;
    nodes.push(AstId::Pattern(0));
    fields.push(FieldRef::new(AstId::Pattern(0), FieldKind::Name));
    fields.push(FieldRef::new(AstId::Pattern(0), FieldKind::Kernel));

    let Fractalize {
        depth,
        max_cells,
        prune,
    } = fractalize;
    let _: &u8 = depth;
    let _: &u64 = max_cells;
    nodes.push(AstId::Fractalize(0));
    fields.push(FieldRef::new(AstId::Fractalize(0), FieldKind::Depth));
    fields.push(FieldRef::new(AstId::Fractalize(0), FieldKind::MaxCells));
    if let Some(Prune {
        density: _,
        seed: _,
    }) = prune
    {
        fields.push(FieldRef::new(AstId::Fractalize(0), FieldKind::Density));
        fields.push(FieldRef::new(AstId::Fractalize(0), FieldKind::Seed));
    }

    let Linearize { traversal } = linearize;
    let _ = traversal;
    nodes.push(AstId::Linearize(0));
    fields.push(FieldRef::new(AstId::Linearize(0), FieldKind::Traversal));

    let MapRhythm { unit, tail } = map_rhythm;
    let _: &Unit = unit;
    let _ = tail;
    nodes.push(AstId::MapRhythm(0));
    fields.push(FieldRef::new(AstId::MapRhythm(0), FieldKind::Unit));
    fields.push(FieldRef::new(AstId::MapRhythm(0), FieldKind::Tail));

    let Generate {
        source,
        bars,
        seed,
        candidates,
        strategy,
        corpus,
    } = generate;
    let _: &StringLiteral = source;
    let _: &u64 = bars;
    let _: &u64 = seed;
    let _: &u64 = candidates;
    let _: &StrategyPolicy = strategy;
    nodes.push(AstId::Generate(0));
    fields.push(FieldRef::new(AstId::Generate(0), FieldKind::Source));
    fields.push(FieldRef::new(AstId::Generate(0), FieldKind::Bars));
    // The generation seed shares its `FieldKind` with the pruning seed; the
    // owning `AstId` is what tells them apart.
    fields.push(FieldRef::new(AstId::Generate(0), FieldKind::Seed));
    fields.push(FieldRef::new(AstId::Generate(0), FieldKind::Candidates));
    fields.push(FieldRef::new(AstId::Generate(0), FieldKind::Strategy));
    if corpus.is_some() {
        fields.push(FieldRef::new(AstId::Generate(0), FieldKind::Corpus));
    }

    let Export { format: fmt, path } = export;
    let _ = fmt;
    let _: &StringLiteral = path;
    nodes.push(AstId::Export(0));
    fields.push(FieldRef::new(AstId::Export(0), FieldKind::Format));
    fields.push(FieldRef::new(AstId::Export(0), FieldKind::Path));

    (fields, nodes)
}

#[test]
fn the_reference_program_maps_seven_nodes_and_eighteen_fields() {
    let parsed = parse_with_source_map(REFERENCE).expect("the reference parses");
    let (expected_fields, expected_nodes) = classify(&parsed.value);

    assert_eq!(expected_nodes.len(), 7, "the level-1 node kinds");
    assert_eq!(expected_fields.len(), 18, "the level-1 value fields");

    let mapped_nodes: Vec<AstId> = parsed.source_map.nodes().map(|(id, _)| id).collect();
    let mapped_fields: Vec<FieldRef> = parsed.source_map.fields().map(|(r, _)| r).collect();
    assert_eq!(
        mapped_nodes.len(),
        7,
        "every node is located: {mapped_nodes:?}"
    );
    assert_eq!(
        mapped_fields.len(),
        18,
        "every value field is located: {mapped_fields:?}"
    );

    for id in expected_nodes {
        assert!(
            parsed.source_map.node_span(id).is_some(),
            "no node span for {id:?}"
        );
    }
    for reference in expected_fields {
        assert!(
            parsed.source_map.field_span(reference).is_some(),
            "no field span for {reference:?}"
        );
    }
}

#[test]
fn every_reference_field_slices_back_to_its_author_value() {
    let parsed = parse_with_source_map(REFERENCE).expect("the reference parses");
    let at = |node: AstId, field: FieldKind| {
        let span = parsed
            .source_map
            .field_span(FieldRef::new(node, field))
            .unwrap_or_else(|| panic!("no span for {node:?}/{field:?}"));
        slice(REFERENCE, span)
    };

    assert_eq!(at(AstId::Program(0), FieldKind::Level), "1", "digits only");
    assert_eq!(at(AstId::Pattern(0), FieldKind::Name), "dgd_fractal");
    assert_eq!(
        at(AstId::Pattern(0), FieldKind::Kernel),
        "\"X.X/XX./.XX\"",
        "quotes included"
    );
    assert_eq!(at(AstId::Fractalize(0), FieldKind::Depth), "1");
    assert_eq!(at(AstId::Fractalize(0), FieldKind::MaxCells), "4096");
    assert_eq!(
        at(AstId::Fractalize(0), FieldKind::Density),
        "9500bps",
        "the complete value token, suffix included"
    );
    assert_eq!(
        at(AstId::Fractalize(0), FieldKind::Seed),
        "4",
        "the pruning seed"
    );
    assert_eq!(at(AstId::Linearize(0), FieldKind::Traversal), "snake");
    assert_eq!(
        at(AstId::MapRhythm(0), FieldKind::Unit),
        "1/16",
        "the whole rational token"
    );
    assert_eq!(at(AstId::MapRhythm(0), FieldKind::Tail), "rest_pad");
    assert_eq!(
        at(AstId::Generate(0), FieldKind::Source),
        "\"corpus/Dance Gavin Dance - The Robot With Human Hair Part 2.gp5\""
    );
    assert_eq!(at(AstId::Generate(0), FieldKind::Bars), "8");
    assert_eq!(
        at(AstId::Generate(0), FieldKind::Seed),
        "42",
        "the generation seed, told apart by its owning node"
    );
    assert_eq!(at(AstId::Generate(0), FieldKind::Candidates), "2");
    assert_eq!(
        at(AstId::Generate(0), FieldKind::Strategy),
        "repeat_variation"
    );
    assert_eq!(at(AstId::Generate(0), FieldKind::Corpus), "\"corpus\"");
    assert_eq!(at(AstId::Export(0), FieldKind::Format), "midi");
    assert_eq!(
        at(AstId::Export(0), FieldKind::Path),
        "\"dgd_fractal_dense.mid\""
    );
}

#[test]
fn a_node_span_contains_every_field_span_it_owns() {
    // The weaker structural claim, but the one that catches a node span
    // built from the wrong construct entirely.
    let parsed = parse_with_source_map(REFERENCE).expect("the reference parses");
    for (reference, field_span) in parsed.source_map.fields() {
        let node_span = parsed
            .source_map
            .node_span(reference.node())
            .expect("a located field's node is located");
        assert!(
            node_span.start <= field_span.start && field_span.end <= node_span.end,
            "{reference:?} at {field_span:?} escapes its node {node_span:?}"
        );
    }
}

// ── B. optionality ──────────────────────────────────────────────────────────

#[test]
fn omitted_words_get_no_phantom_spans() {
    let parsed = parse_with_source_map(MINIMAL).expect("the minimal program parses");
    let (expected_fields, _) = classify(&parsed.value);
    assert_eq!(
        expected_fields.len(),
        15,
        "no prune and no corpus is three fields fewer"
    );
    assert_eq!(parsed.source_map.fields().count(), 15);

    for absent in [
        FieldRef::new(AstId::Fractalize(0), FieldKind::Density),
        FieldRef::new(AstId::Fractalize(0), FieldKind::Seed),
        FieldRef::new(AstId::Generate(0), FieldKind::Corpus),
    ] {
        assert!(
            parsed.source_map.field_span(absent).is_none(),
            "{absent:?} was never written, so it has no location"
        );
    }
    // The generation seed is still there — it is a different owner.
    assert!(parsed
        .source_map
        .field_span(FieldRef::new(AstId::Generate(0), FieldKind::Seed))
        .is_some());
}

// ── C. legal reordering ─────────────────────────────────────────────────────

#[test]
fn reordered_words_keep_the_key_set_and_move_only_the_spans() {
    // §3.2 lets the words of a construct arrive in any order; the canonical
    // formatter decides the output order. Two such sources are the same
    // program, so they must have the same handles — and different bytes.
    let a = MINIMAL;
    let b = r#"swang 1

pattern p {
    ascii "X.X/XX./.XX"
    |> fractalize max_cells 4096 depth 1
    |> linearize snake
    |> map_rhythm tail rest_pad unit 1/16
    |> generate {
        bars 8
        candidates 2
        source "seed.gp5"
        strategy auto
        seed 42
    }
    |> export midi "out.mid"
}
"#;

    let pa = parse_with_source_map(a).expect("a parses");
    let pb = parse_with_source_map(b).expect("b parses");
    assert_eq!(pa.value, pb.value, "legal reordering is the same program");

    let keys = |p: &griff_swang::syntax::Parsed<Program>| {
        (
            p.source_map.nodes().map(|(id, _)| id).collect::<Vec<_>>(),
            p.source_map.fields().map(|(r, _)| r).collect::<Vec<_>>(),
        )
    };
    assert_eq!(keys(&pa), keys(&pb), "equal ASTs, equal handles");

    // The deliberately moved words must slice to the same value from
    // different bytes — a map built from canonical field order instead of
    // the author's tokens would give identical spans here.
    for (node, field) in [
        (AstId::Fractalize(0), FieldKind::Depth),
        (AstId::MapRhythm(0), FieldKind::Unit),
        (AstId::Generate(0), FieldKind::Source),
    ] {
        let sa = pa
            .source_map
            .field_span(FieldRef::new(node, field))
            .expect("present in a");
        let sb = pb
            .source_map
            .field_span(FieldRef::new(node, field))
            .expect("present in b");
        assert_ne!(sa, sb, "{node:?}/{field:?} did not move, but its word did");
        assert_eq!(
            slice(a, sa),
            slice(b, sb),
            "{node:?}/{field:?} must still name the same value"
        );
    }
}

// ── D. UTF-8 boundaries ─────────────────────────────────────────────────────

#[test]
fn every_span_lands_on_a_char_boundary_in_a_multibyte_source() {
    // Multibyte text inside the string literals, which is where level 1
    // lets arbitrary UTF-8 live. A map computed in `chars` rather than
    // bytes slices mid-character or out of range here.
    let source = "swang 1\n\npattern p {\n    ascii \"X.X/XX./.XX\"\n    \
                  |> fractalize depth 1 max_cells 4096\n    |> linearize snake\n    \
                  |> map_rhythm unit 1/16 tail rest_pad\n    |> generate {\n        \
                  source \"корпус/Ω音楽 — café.gp5\"\n        bars 8\n        seed 42\n        \
                  candidates 2\n        strategy auto\n        corpus \"кор—пус\"\n    }\n    \
                  |> export midi \"вы—ход.mid\"\n}\n";
    let parsed = parse_with_source_map(source).expect("multibyte literals parse");

    let mut checked = 0_usize;
    for (id, span) in parsed.source_map.nodes() {
        assert!(span.start <= span.end, "{id:?} is inverted");
        assert!(
            span.end as usize <= source.len(),
            "{id:?} runs past the source"
        );
        assert!(source.is_char_boundary(span.start as usize), "{id:?} start");
        assert!(source.is_char_boundary(span.end as usize), "{id:?} end");
        checked += 1;
    }
    for (reference, span) in parsed.source_map.fields() {
        assert!(span.start <= span.end, "{reference:?} is inverted");
        assert!(
            span.end as usize <= source.len(),
            "{reference:?} runs past the source"
        );
        assert!(
            source.is_char_boundary(span.start as usize),
            "{reference:?} start"
        );
        assert!(
            source.is_char_boundary(span.end as usize),
            "{reference:?} end"
        );
        checked += 1;
    }
    assert!(checked >= 20, "only {checked} spans examined");

    assert_eq!(
        parsed
            .source_map
            .field_span(FieldRef::new(AstId::Generate(0), FieldKind::Source))
            .map(|s| slice(source, s)),
        Some("\"корпус/Ω音楽 — café.gp5\""),
        "the multibyte literal slices whole, quotes included"
    );
}

// ── E. one parser ───────────────────────────────────────────────────────────

#[test]
fn the_two_entry_points_accept_the_same_programs() {
    for source in [REFERENCE, MINIMAL] {
        let plain = parse(source).expect("parses");
        let mapped = parse_with_source_map(source).expect("parses").value;
        assert_eq!(plain, mapped, "one parser, two entry points");
    }
}

#[test]
fn the_two_entry_points_refuse_identically() {
    // Not just "both fail": the same codes, messages, spans, and order.
    let flawed = [
        "swang 1\n\npattern p {\n    ascii \"X.X/XX./.XX\"\n    \
         |> fractalize depth 1 max_cells 4096 density 9500bps\n    |> linearize snake\n    \
         |> map_rhythm unit 1/16 tail rest_pad\n    |> generate {\n        \
         source \"s.gp5\"\n        bars 8\n        seed 42\n        candidates 2\n        \
         strategy auto\n    }\n    |> export midi \"o.mid\"\n}\n",
        "swang 9\n",
        "\u{feff}swang 1\n",
        "swang 1\n\npattern p {\n}\n",
        "not a header at all\n",
    ];
    for source in flawed {
        let a = parse(source).expect_err("refused");
        let b = parse_with_source_map(source).expect_err("refused identically");
        assert_eq!(
            a.len(),
            b.len(),
            "same number of diagnostics for {source:?}"
        );
        for (x, y) in a.iter().zip(b.iter()) {
            assert_eq!(x.code, y.code);
            assert_eq!(x.message, y.message);
            assert_eq!(x.span, y.span);
        }
    }
}

// ── F. the formatter needs no map ───────────────────────────────────────────

#[test]
fn an_ast_built_in_memory_still_formats_and_reparses() {
    // No source text, no source map, no parse: a lifter constructs programs
    // directly. If the formatter ever needed a map, this would not compile.
    let program = Program {
        level: Level::new(1).expect("this build's level"),
        pattern: PatternDef {
            name: Ident::new("built_by_hand").expect("a name"),
            kernel: KernelLiteral::new("X.X/XX./.XX").expect("the spec kernel"),
            fractalize: Fractalize {
                depth: 1,
                max_cells: 4096,
                prune: Some(Prune {
                    density: griff_pattern::DensityBps::new(9500).expect("in scale"),
                    seed: 4,
                }),
            },
            linearize: Linearize {
                traversal: griff_pattern::Traversal::Snake,
            },
            map_rhythm: MapRhythm {
                unit: Unit::new(1, 16).expect("a note value"),
                tail: griff_swang::TailPolicy::RestPad,
            },
            generate: Generate {
                source: StringLiteral::new("seed.gp5").expect("a path"),
                bars: 8,
                seed: 42,
                candidates: 2,
                strategy: StrategyPolicy::Named(StrategyName::RepeatVariation),
                corpus: None,
            },
            export: Export {
                format: griff_swang::syntax::ExportFormat::Midi,
                path: StringLiteral::new("out.mid").expect("a path"),
            },
        },
    };

    let text = format(&program);
    assert_eq!(
        parse(&text).expect("the formatter's output parses"),
        program,
        "parse(format(ast)) == ast, with no source map in sight"
    );
    assert_eq!(
        format(&parse(&text).expect("reparses")),
        text,
        "and it is a fixed point"
    );
}

// ── G. frozen diagnostic ownership ──────────────────────────────────────────

#[test]
fn the_four_released_diagnostic_locations_are_unchanged() {
    // §3.5 already released these four locations. The map now knows `bars`,
    // `candidates`, `strategy` and the rest too — that is editor capability,
    // not permission to move a diagnostic somebody's tooling already parses.
    let parsed = parse_with_source_map(REFERENCE).expect("the reference parses");
    let at = |node: AstId, field: FieldKind| {
        slice(
            REFERENCE,
            parsed
                .source_map
                .field_span(FieldRef::new(node, field))
                .expect("released location"),
        )
    };
    assert_eq!(at(AstId::Pattern(0), FieldKind::Kernel), "\"X.X/XX./.XX\"");
    assert_eq!(at(AstId::MapRhythm(0), FieldKind::Unit), "1/16");
    assert_eq!(at(AstId::MapRhythm(0), FieldKind::Tail), "rest_pad");
    assert_eq!(
        at(AstId::Generate(0), FieldKind::Source),
        "\"corpus/Dance Gavin Dance - The Robot With Human Hair Part 2.gp5\""
    );
}

// ── determinism of the iteration order ──────────────────────────────────────

#[test]
fn iteration_order_is_deterministic_and_sorted() {
    let parsed = parse_with_source_map(REFERENCE).expect("the reference parses");
    let once: Vec<AstId> = parsed.source_map.nodes().map(|(id, _)| id).collect();
    let twice: Vec<AstId> = parsed.source_map.nodes().map(|(id, _)| id).collect();
    assert_eq!(once, twice, "two walks of one map agree");

    let mut sorted = once.clone();
    sorted.sort();
    assert_eq!(once, sorted, "nodes come out in key order");

    let fields: Vec<FieldRef> = parsed.source_map.fields().map(|(r, _)| r).collect();
    let mut sorted_fields = fields.clone();
    sorted_fields.sort();
    assert_eq!(fields, sorted_fields, "fields come out in key order");
}
