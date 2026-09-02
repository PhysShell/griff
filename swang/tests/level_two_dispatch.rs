//! SWG-4A-06: level dispatch, root dispatch, and the level-2 skeleton.
//!
//! ```text
//! header_level -> level dispatch -> root dispatch (pattern | score)
//! ```
//!
//! Spec §5.4 says a build "routes to one of them and never mixes them:
//! there is no single grammar with level-conditioned branches, because such
//! a grammar has no way to prove that level 1's behaviour survived the
//! addition of level 2". These tests are that proof obligation, written
//! down: every case here either pins where a source is routed, or pins that
//! a source which routes nowhere is refused rather than half-read.
//!
//! # The slice boundary
//!
//! This task accepts exactly one level-2 program shape — the minimal empty
//! score, `score { ppqn <n> }`. Every other grammatical `score` word
//! (`master_bar`, `track`, `source`, `loss`) is real grammar owned by
//! SWG-4A-08 and is **refused here**, not tolerated: a parser that ignores
//! a word it does not implement is how exact text stops being exact. The
//! scalar layer is SWG-4A-07's; the two refusals this slice implements
//! (`SWG0505`, `SWG0506`) are the ones reachable from the single word it
//! reads, because the alternative is to accept `ppqn 0` — text §6.6
//! declares invalid — into the accepted set of a level that has not frozen.

// Reason: integration-test code. `unwrap`/`expect`/`panic` abort loudly with
// a clear message, which is exactly what a test harness wants.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::missing_assert_message
)]

use griff_swang::syntax::{
    format, format_document, header_level, parse, parse_document, parse_document_with_source_map,
    AstId, Diagnostic, Document, FieldKind, FieldRef, Level, Span, LANGUAGE_LEVEL,
};

/// The one level-1 program every dispatch test measures level 1 against:
/// spec §3.1's reference program, byte for byte, so "level 1 is unchanged"
/// is measured against the text level 1 froze rather than a sketch of it.
const LEVEL_ONE: &str = r#"swang 1

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

/// The same program under a level-2 header: a level-1 body is not level-2
/// text, however well formed it is at its own level.
const LEVEL_ONE_BODY_UNDER_TWO: &str = r#"swang 2

pattern dgd_fractal {
    ascii "X.X/XX./.XX"
}
"#;

/// The minimal level-2 score: `ppqn` is the construct's only `1` word, and
/// `master_bar`/`track`/`source`/`loss` are omissible (§6.2, §6.4b).
const MINIMAL_SCORE: &str = "swang 2\n\nscore {\n    ppqn 960\n}\n";

fn codes(diagnostics: &[Diagnostic]) -> Vec<&str> {
    diagnostics.iter().map(|d| d.code).collect()
}

fn refusal(source: &str) -> Vec<Diagnostic> {
    parse_document(source).err().unwrap_or_else(|| {
        panic!("this source must be refused, and it was accepted:\n{source}");
    })
}

// ── the level surface ───────────────────────────────────────────────────────

#[test]
fn this_build_supports_level_two() {
    assert_eq!(
        LANGUAGE_LEVEL, 2,
        "SWG-4A-06 is the task that admits level 2"
    );
    assert_eq!(header_level(MINIMAL_SCORE), Ok(2));
    assert_eq!(header_level(LEVEL_ONE), Ok(1));
}

#[test]
fn an_unknown_newer_level_is_still_refused_by_the_frozen_pre_parser() {
    // §1.1: the pre-parser never changes across releases. What changes is
    // the supported range it names, and it must name the range this build
    // actually has.
    let d = header_level("swang 3\n\nscore {\n}\n").expect_err("level 3 is not supported");
    assert_eq!(d.code, "SWG0001");
    assert!(
        d.message.contains("1..=2"),
        "SWG0001 reports the supported range: {}",
        d.message
    );
    assert_eq!(codes(&refusal("swang 3\n\nscore {\n}\n")), ["SWG0001"]);
    assert_eq!(
        codes(&refusal("swang 999999999\n\nscore {\n}\n")),
        ["SWG0001"]
    );
}

#[test]
fn a_malformed_or_truncated_header_is_still_swg0002() {
    for source in [
        "swang\n\nscore {\n}\n",
        "swang 2",
        "swang  2\n",
        "swang 02\n",
        " swang 2\n",
        "SWANG 2\n",
        "swang 2x\n",
        "",
    ] {
        assert_eq!(
            codes(&refusal(source)),
            ["SWG0002"],
            "a malformed header is refused before any grammar sees it: {source:?}"
        );
    }
    assert_eq!(codes(&refusal("\u{feff}swang 2\n")), ["SWG0003"]);
}

// ── level dispatch ──────────────────────────────────────────────────────────

#[test]
fn a_level_one_source_routes_to_the_frozen_level_one_parser() {
    let Ok(Document::Pattern(program)) = parse_document(LEVEL_ONE) else {
        panic!("a valid `swang 1` source routes to the level-1 parser");
    };
    let direct = parse(LEVEL_ONE).expect("the frozen entry point still accepts it");
    assert_eq!(program, direct, "dispatch adds no reinterpretation");
}

#[test]
fn dispatching_a_level_one_source_formats_byte_for_byte_as_before() {
    // Law A observable 3, through the new entry point.
    let document = parse_document(LEVEL_ONE).expect("accepted");
    let program = parse(LEVEL_ONE).expect("accepted");
    assert_eq!(format_document(&document), format(&program));
}

#[test]
fn a_minimal_level_two_score_reaches_the_exact_parser() {
    let Ok(Document::Score(_)) = parse_document(MINIMAL_SCORE) else {
        panic!("`swang 2` with a `score` root routes to the level-2 parser");
    };
}

#[test]
fn the_level_one_entry_point_does_not_read_level_two() {
    // §5.4: one build routes to one entry point and never mixes them. The
    // frozen entry point must not half-read a level it does not own — and
    // must never hand back a level-1 `Program` carrying level 2.
    let diagnostics = parse(MINIMAL_SCORE).expect_err("the level-1 parser does not read level 2");
    assert_eq!(codes(&diagnostics), ["SWG0401"]);
    let under_two = parse(LEVEL_ONE_BODY_UNDER_TWO)
        .expect_err("not even a level-1 body under a level-2 header");
    assert_eq!(codes(&under_two), ["SWG0401"]);
}

// ── root dispatch ───────────────────────────────────────────────────────────

#[test]
fn a_level_two_pattern_root_is_refused() {
    // §5.7: "Level 2 does not admit the level-1 `pattern` root."
    let diagnostics = refusal("swang 2\n\npattern riff {\n}\n");
    assert_eq!(codes(&diagnostics), ["SWG0401"]);
    let first = diagnostics.first().expect("one diagnostic");
    assert!(
        first.message.contains("score") && first.message.contains("pattern"),
        "the refusal names the root it wanted and the root it found: {}",
        first.message
    );
    // And a real level-1 body under a level-2 header is refused too. Its
    // string literal reaches the level-2 lexer before the root check does,
    // so the message is the lexer's — the verdict is what this case pins,
    // and the verdict is closed either way.
    assert_eq!(codes(&refusal(LEVEL_ONE_BODY_UNDER_TWO)), ["SWG0401"]);
}

#[test]
fn a_level_one_score_root_keeps_its_frozen_refusal() {
    // Law A's invalid-body half (§5.5): a level-2 keyword in a `swang 1`
    // script raises exactly what level 1 already raised — not a friendlier
    // "`score` requires language level 2".
    let diagnostics = refusal("swang 1\n\nscore {\n    ppqn 960\n}\n");
    assert_eq!(codes(&diagnostics), ["SWG0401"]);
    let through_frozen =
        parse("swang 1\n\nscore {\n    ppqn 960\n}\n").expect_err("level 1 refuses a `score` root");
    assert_eq!(
        diagnostics, through_frozen,
        "identical code, message, span and order — the frozen verdict is not \
         rewritten by the arrival of level 2"
    );
}

#[test]
fn a_malformed_prefix_is_not_accepted_as_a_root() {
    for source in [
        "swang 2\n\nscor {\n    ppqn 960\n}\n",
        "swang 2\n\nscores {\n    ppqn 960\n}\n",
        "swang 2\n\nscore\n",
        "swang 2\n\nscore {\n",
        "swang 2\n\nscore {\n    ppqn 960\n",
        "swang 2\n\n{\n    ppqn 960\n}\n",
        "swang 2\n\n",
    ] {
        assert_eq!(
            codes(&refusal(source)),
            ["SWG0401"],
            "a prefix of the root is not the root: {source:?}"
        );
    }
}

#[test]
fn trailing_material_cannot_bypass_the_root_contract() {
    for source in [
        "swang 2\n\nscore {\n    ppqn 960\n}\nscore {\n    ppqn 480\n}\n",
        "swang 2\n\nscore {\n    ppqn 960\n}\npattern riff {\n    ascii \"x-\"\n}\n",
        "swang 2\n\nscore {\n    ppqn 960\n}\n}\n",
        "swang 2\n\nscore {\n    ppqn 960\n} ppqn 480\n",
    ] {
        assert_eq!(
            codes(&refusal(source)),
            ["SWG0401"],
            "one document holds one root: {source:?}"
        );
    }
}

// ── the score body, and what this slice does not yet parse ──────────────────

#[test]
fn ppqn_is_the_one_required_word() {
    assert_eq!(codes(&refusal("swang 2\n\nscore {\n}\n")), ["SWG0403"]);
    assert_eq!(
        codes(&refusal(
            "swang 2\n\nscore {\n    ppqn 960\n    ppqn 480\n}\n"
        )),
        ["SWG0404"],
        "`ppqn` is a `1` word, so repeating it is SWG0404 (§6.4b)"
    );
}

#[test]
fn an_unknown_score_word_is_refused() {
    assert_eq!(
        codes(&refusal(
            "swang 2\n\nscore {\n    ppqn 960\n    nope 1\n}\n"
        )),
        ["SWG0401"],
        "§6.4b: an unknown field word is SWG0401"
    );
}

#[test]
fn grammatical_words_this_slice_does_not_parse_fail_closed() {
    // `master_bar`, `track`, `source` and `loss` are real `score` words
    // (§6.4b) owned by SWG-4A-08. Until it lands they are refused, never
    // skipped: silently ignoring a word is how exact text stops being exact.
    for word in [
        "master_bar {\n    }",
        "track {\n    }",
        "source {\n    }",
        "loss {\n    }",
    ] {
        let source = format!("swang 2\n\nscore {{\n    ppqn 960\n    {word}\n}}\n");
        assert_eq!(
            codes(&refusal(&source)),
            ["SWG0401"],
            "not yet parsed means refused, not ignored: {word}"
        );
    }
}

#[test]
fn the_one_scalar_this_slice_reads_refuses_what_the_registry_says_it_must() {
    // Scoped deliberately to `ppqn`. The scalar layer — widths, non-zero
    // types, rational tempo, ranges, pitch, velocity, meter, confidence,
    // enums, escapes — is SWG-4A-07's and is absent here.
    assert_eq!(
        codes(&refusal("swang 2\n\nscore {\n    ppqn 0960\n}\n")),
        ["SWG0505"],
        "§6.6: a leading zero is a non-canonical spelling"
    );
    assert_eq!(
        codes(&refusal("swang 2\n\nscore {\n    ppqn 0\n}\n")),
        ["SWG0506"],
        "§6.6 names zero `ppqn` as a canonical-model invariant violation"
    );
    for over in ["ppqn 70000", "ppqn 99999999999999999999"] {
        assert_eq!(
            codes(&refusal(&format!("swang 2\n\nscore {{\n    {over}\n}}\n"))),
            ["SWG0401"],
            "a value that does not fit the field is SWG0401, as at level 1"
        );
    }
    for shape in ["ppqn", "ppqn x", "ppqn \"960\"", "ppqn 9/6", "ppqn 960bps"] {
        assert_eq!(
            codes(&refusal(&format!("swang 2\n\nscore {{\n    {shape}\n}}\n"))),
            ["SWG0401"],
            "and a value of the wrong shape is SWG0401: {shape}"
        );
    }
}

// ── the formatter, dispatched ───────────────────────────────────────────────

#[test]
fn the_level_two_formatter_preserves_the_level_and_is_its_own_fixed_point() {
    // §5.8: the formatter never downgrades or promotes a document.
    let document = parse_document(MINIMAL_SCORE).expect("accepted");
    let canonical = format_document(&document);
    assert!(
        canonical.starts_with("swang 2\n"),
        "the level travels with the document: {canonical:?}"
    );
    let reparsed = parse_document(&canonical).expect("canonical text reparses");
    assert_eq!(format_document(&reparsed), canonical, "law 2: fixed point");
    assert_eq!(reparsed, document, "law 3: and it is the same document");
    let Document::Score(_) = reparsed else {
        panic!("canonical level-2 text is still a score");
    };
}

#[test]
fn a_level_two_round_trip_preserves_the_value_it_parsed() {
    // Law 3, `parse(format(ast)) == ast`, which the fixed-point law does not
    // imply: a formatter emitting a *constant* `ppqn` is its own fixed point
    // and reparses to something that is not what it was given. A probe found
    // that hole the honest way — replacing the interpolated `ppqn` with the
    // literal 960 was caught by nothing, because every fixture used 960.
    //
    // So the values here are deliberately not the one the other fixtures use.
    for ppqn in ["1", "480", "65535"] {
        let source = format!("swang 2\n\nscore {{\n    ppqn {ppqn}\n}}\n");
        let document = parse_document(&source).expect("a valid minimal score");
        let canonical = format_document(&document);
        assert_eq!(canonical, source, "this fixture is already canonical");
        let reparsed = parse_document(&canonical).expect("canonical text reparses");
        assert_eq!(
            reparsed, document,
            "parse(format(ast)) == ast: the value survives the round trip"
        );
    }
}

#[test]
fn canonical_level_two_text_is_the_one_spelling() {
    let document = parse_document("swang 2\n\nscore {\nppqn 960\n}\n").expect("accepted");
    assert_eq!(format_document(&document), MINIMAL_SCORE);
}

// -- the level a constructed AST may carry ----------------------------------

#[test]
fn a_level_one_ast_cannot_carry_level_two() {
    // The parser guard closed the *parse* path; this closes the
    // *construction* path, which `Program`'s public fields leave open.
    // `ast::v1::Level` validated against `LANGUAGE_LEVEL`, so raising the
    // constant silently made `Level::new(2)` succeed — and a caller could
    // then build a `Program` the formatter renders as `swang 2` above a
    // `pattern` root. Both entry points refuse that text, so
    // `parse(format(ast)) == ast` would fail for an AST anyone can build.
    Level::new(1).expect("level 1 is the level this AST spells");
    for other in [0, 2, 3, LANGUAGE_LEVEL, u32::MAX] {
        assert!(
            other == 1 || Level::new(other).is_err(),
            "the level-1 AST spells no level but 1; `Level::new({other})` must \
             not succeed merely because the build understands that level"
        );
    }
}

#[test]
fn the_level_one_ast_refusal_names_the_level_it_spells() {
    // Codex, on 6d1775d: pinning the constructor to level 1 left its refusal
    // still printing the *build's* range. With `LANGUAGE_LEVEL` at 2 that
    // renders "language level 2 is not supported (1..=2)" — a message that
    // refuses a level while listing it among the supported ones. A public
    // error contradicting itself is a defect on its own: it tells the caller
    // the call it just lost should have succeeded, and sends them looking for
    // the bug anywhere but where it is.
    for level in [0_u32, 2, 3, LANGUAGE_LEVEL, u32::MAX] {
        if level == 1 {
            continue;
        }
        let refusal = Level::new(level)
            .expect_err("the level-1 AST spells no level but 1")
            .to_string();
        assert!(
            refusal.contains(&level.to_string()),
            "the refusal must name the level it rejected; got {refusal:?}"
        );
        assert!(
            !refusal.contains(&format!("1..={LANGUAGE_LEVEL}")),
            "the level-1 AST's refusal must describe the level *it* spells, \
             not the build's supported range — a range that now contains the \
             level just refused; got {refusal:?}"
        );
    }
}

// -- the source map the skeleton ships with ---------------------------------

#[test]
fn a_level_two_parse_carries_a_source_map() {
    // The SWG-4A-06 contract: "the skeleton ships complete: a typed root
    // enum, the source map, deterministic diagnostics, resource budgets, and
    // formatter dispatch." Without it a caller cannot locate a successfully
    // parsed level-2 construct, and 4A-09's builder diagnostics would have
    // to reparse to point anywhere.
    let parsed = parse_document_with_source_map(MINIMAL_SCORE).expect("accepted");
    let Document::Score(_) = parsed.value else {
        panic!("it is a score");
    };
    let map = &parsed.source_map;
    let root = map
        .node_span(AstId::Score(0))
        .expect("the score block has a location");
    assert_eq!(
        MINIMAL_SCORE.get(root.start as usize..root.end as usize),
        Some("score {\n    ppqn 960\n}"),
        "the node span is the construct, from its keyword to its brace"
    );
    let ppqn = map
        .field_span(FieldRef::new(AstId::Score(0), FieldKind::Ppqn))
        .expect("`ppqn` has a location");
    assert_eq!(
        MINIMAL_SCORE.get(ppqn.start as usize..ppqn.end as usize),
        Some("960"),
        "the field span is the value, not the word that introduces it"
    );
}

#[test]
fn the_dispatched_map_and_the_dispatched_parse_agree() {
    // One parser, two entry points: the mapped one is the plain one with the
    // map dropped, so the two cannot drift in what they accept.
    for source in [MINIMAL_SCORE, LEVEL_ONE] {
        let mapped = parse_document_with_source_map(source).expect("accepted");
        let plain = parse_document(source).expect("accepted");
        assert_eq!(mapped.value, plain);
    }
    for source in [
        "swang 2\n\nscore {\n}\n",
        "swang 3\n",
        "swang 2\n\npattern x {\n}\n",
    ] {
        assert_eq!(
            parse_document_with_source_map(source).err(),
            parse_document(source).err(),
            "and they refuse identically: {source:?}"
        );
    }
}

// ── the inherited INF-06 obligations ────────────────────────────────────────

#[test]
fn the_source_byte_budget_breaches_end_to_end_before_any_success() {
    // The declared limit, through the public path, with no scaling and no
    // test-only reconstruction of the parser. The padding is whitespace, so
    // the source is *grammatically* a valid minimal score and would be
    // accepted if the byte check were not consulted first — which is what
    // makes this a witness that no successful `swang 2` result precedes
    // budget wiring, rather than merely a witness that a big string fails.
    let padding = " ".repeat(16 * 1024 * 1024);
    let source = format!("swang 2\n\nscore {{\n    ppqn 960\n{padding}}}\n");
    let diagnostics = refusal(&source);
    assert_eq!(codes(&diagnostics), ["SWG0509"]);
    let first = diagnostics.first().expect("one diagnostic");
    assert!(
        first.message.contains("source bytes") && first.message.contains("16777216"),
        "the breach names its axis and the declared limit: {}",
        first.message
    );
    // Spec §5.11's breach-location table: "source bytes -> the level/header
    // span — no body token has been admitted". Pointing at `0..len` would
    // highlight sixteen megabytes and call it a location.
    assert_eq!(
        first.span,
        Span { start: 6, end: 7 },
        "the breach points at the level digits, not at the whole document"
    );
}

#[test]
fn the_token_budget_breaches_end_to_end_as_a_typed_refusal() {
    // Four million tokens plus one, spelled in under the byte cap, so the
    // token axis is the one that fires. A typed `SWG0509`, not an
    // allocation death.
    let source = format!(
        "swang 2\n\nscore {{\n    ppqn 960\n{}}}\n",
        "a ".repeat(4_000_001)
    );
    let diagnostics = refusal(&source);
    assert_eq!(codes(&diagnostics), ["SWG0509"]);
    assert!(
        diagnostics
            .first()
            .is_some_and(|d| d.message.contains("tokens")),
        "the breach names the token axis"
    );
}

#[test]
fn a_level_one_source_never_reaches_the_level_two_budget() {
    // The budget is level 2's alone (§5.11). A level-1 source larger than
    // the level-2 source cap is still parsed by level 1's rules, because
    // level 1's acceptance set is frozen and a bound that reached it would
    // be a narrowing.
    let padding = " ".repeat(16 * 1024 * 1024);
    let source = LEVEL_ONE.replace(
        "pattern dgd_fractal {",
        &format!("pattern dgd_fractal {{\n{padding}"),
    );
    assert!(
        source.len() > 16 * 1024 * 1024,
        "the fixture really is over the level-2 cap"
    );
    let document = parse_document(&source).expect("level 1 knows no such bound");
    let Document::Pattern(_) = document else {
        panic!("it is still a pattern");
    };
    assert!(parse(&source).is_ok(), "and the frozen entry point agrees");
}
