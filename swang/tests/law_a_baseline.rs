//! SWG-INF-06: the frozen Law A baseline for language level 1.
//!
//! Spec §5.5 says a build supporting `1..=N` must treat every `swang 1`
//! source — **including invalid ones** — exactly as a level-1-only build
//! did, compared on verdict, AST, canonical bytes, diagnostic code, message,
//! span, and order. Today `N == 1`, so that comparison has nothing on its
//! right-hand side; the moment SWG-4A-06 adds level dispatch it has
//! everything, and by then the level-1-only build is gone — exactly as the
//! pre-refactor parser SWG-INF-06's original sketch wanted to diff against
//! is already gone.
//!
//! So this suite records the left-hand side now, while a level-1-only build
//! is what the tree contains. The recorded document is a historical
//! artifact, not a re-derivation: it names the commit that produced it, and
//! it is **compare-only**. There is deliberately no "update the snapshot"
//! path — a diff here is a Law A violation until a human proves otherwise.
//!
//! Three deliberate properties of the observation:
//!
//! 1. **The AST observation is not the formatter's.** It is a test-owned
//!    projection that reads accessors and spells every field and every enum
//!    variant itself. Recording `format(ast)` twice would give canonical
//!    bytes and the AST as one witness wearing two hats, and a coordinated
//!    parser+formatter regression could preserve the bytes while changing
//!    what the AST means.
//! 2. **It is not `Debug`, and it is not `serde`.** `Debug` output is a
//!    representation detail no contract pins, and `Program` is deliberately
//!    not serialized. Adding either to manufacture a witness would be
//!    inventing a contract in order to test it.
//! 3. **The observables are outcome-dependent.** An accepted source has an
//!    AST and canonical bytes; a rejected one has ordered diagnostics. No
//!    null fields are invented so a schema can boast of holding seven
//!    things.

// Reason: integration-test code. `unwrap`/`expect`/`panic` abort loudly with
// a clear message, which is exactly what a test harness wants.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::missing_assert_message
)]

use std::fmt::{Arguments, Write as _};

use griff_pattern::{DensityBps, Traversal};
use griff_swang::syntax::{
    format, parse, Diagnostic, Export, ExportFormat, Fractalize, Generate, Ident, KernelLiteral,
    Level, Linearize, MapRhythm, PatternDef, Program, Prune, StrategyName, StrategyPolicy,
    StringLiteral, Unit,
};
use griff_swang::TailPolicy;

/// The commit whose parser produced the recorded observations: a
/// level-1-only build with `LANGUAGE_LEVEL == 1`, one parser module, and no
/// level dispatch.
const PRODUCER: &str = "c44313c0cd82f3f2a8720437824d8cf5058b4e15";

/// The recorded document's schema. Bump it only when the encoding itself
/// changes, and only together with a regenerated baseline and a written
/// reason — never to make a failing comparison pass.
const SCHEMA: u32 = 1;

/// The frozen artifact. `include_str!` so a missing or renamed baseline is a
/// compile error rather than a silently skipped test.
const BASELINE: &str = include_str!("law_a_baseline.golden");

/// The checked-in fuzz seed, included as an input subset of this corpus
/// rather than described from memory.
const FUZZ_SEED: &str = include_str!("../../fuzz/corpus/swang_parse/reference.swg");

// ── encoding ─────────────────────────────────────────────────────────────

/// Appends one formatted line. Writing to a `String` cannot fail; the
/// `expect` documents that rather than hiding it.
fn put(out: &mut String, args: Arguments<'_>) {
    out.write_fmt(args)
        .expect("writing to a String cannot fail");
    out.push('\n');
}

/// Encodes a string as `<byte length> "<escaped>"`.
///
/// Length-prefixed so a truncation cannot hide, and escaped so every value
/// stays on one line. Iteration is over `char`s, not bytes: re-emitting a
/// multi-byte character byte by byte would corrupt it.
fn quoted(text: &str) -> String {
    let mut body = String::from("\"");
    for ch in text.chars() {
        match ch {
            '\\' => body.push_str("\\\\"),
            '"' => body.push_str("\\\""),
            '\n' => body.push_str("\\n"),
            '\r' => body.push_str("\\r"),
            '\t' => body.push_str("\\t"),
            other if u32::from(other) < 0x20 || u32::from(other) == 0x7f => {
                let escape = format!("\\x{:02x}", u32::from(other));
                body.push_str(&escape);
            }
            other => body.push(other),
        }
    }
    body.push('"');
    format!("{} {body}", text.len())
}

// ── the AST observation ──────────────────────────────────────────────────
//
// Every struct below is destructured with no `..`, and every enum is matched
// with no wildcard arm. That is the exhaustiveness obligation: adding a
// field or a variant to the level-1 AST breaks this file at compile time,
// so the baseline can never silently stop observing part of the tree.

/// The level, spelled from its accessor rather than its `Debug`.
fn observe_level(out: &mut String, level: Level) {
    put(out, format_args!("ast level {}", level.get()));
}

fn observe_program(out: &mut String, program: &Program) {
    let Program { level, pattern } = program;
    observe_level(out, *level);
    observe_pattern(out, pattern);
}

fn observe_pattern(out: &mut String, pattern: &PatternDef) {
    let PatternDef {
        name,
        kernel,
        fractalize,
        linearize,
        map_rhythm,
        generate,
        export,
    } = pattern;
    put(
        out,
        format_args!("ast pattern.name {}", quoted(name.as_str())),
    );
    put(
        out,
        format_args!("ast pattern.kernel {}", quoted(kernel.as_str())),
    );
    observe_fractalize(out, *fractalize);
    observe_linearize(out, *linearize);
    observe_map_rhythm(out, *map_rhythm);
    observe_generate(out, generate);
    observe_export(out, export);
}

fn observe_fractalize(out: &mut String, fractalize: Fractalize) {
    let Fractalize {
        depth,
        max_cells,
        prune,
    } = fractalize;
    put(out, format_args!("ast fractalize.depth {depth}"));
    put(out, format_args!("ast fractalize.max_cells {max_cells}"));
    match prune {
        None => put(out, format_args!("ast fractalize.prune absent")),
        Some(Prune { density, seed }) => {
            put(out, format_args!("ast fractalize.prune present"));
            put(
                out,
                format_args!("ast fractalize.prune.density {}", density.get()),
            );
            put(out, format_args!("ast fractalize.prune.seed {seed}"));
        }
    }
}

fn observe_linearize(out: &mut String, linearize: Linearize) {
    let Linearize { traversal } = linearize;
    let spelled = match traversal {
        Traversal::RowMajor => "row_major",
        Traversal::Snake => "snake",
    };
    put(out, format_args!("ast linearize.traversal {spelled}"));
}

fn observe_map_rhythm(out: &mut String, map_rhythm: MapRhythm) {
    let MapRhythm { unit, tail } = map_rhythm;
    observe_unit(out, unit);
    let spelled = match tail {
        TailPolicy::Reject => "reject",
        TailPolicy::RestPad => "rest_pad",
    };
    put(out, format_args!("ast map_rhythm.tail {spelled}"));
}

fn observe_unit(out: &mut String, unit: Unit) {
    put(
        out,
        format_args!("ast map_rhythm.unit.numerator {}", unit.numerator()),
    );
    put(
        out,
        format_args!("ast map_rhythm.unit.denominator {}", unit.denominator()),
    );
}

fn observe_generate(out: &mut String, generate: &Generate) {
    let Generate {
        source,
        bars,
        seed,
        candidates,
        strategy,
        corpus,
    } = generate;
    put(
        out,
        format_args!("ast generate.source {}", quoted(source.as_str())),
    );
    put(out, format_args!("ast generate.bars {bars}"));
    put(out, format_args!("ast generate.seed {seed}"));
    put(out, format_args!("ast generate.candidates {candidates}"));
    observe_strategy(out, *strategy);
    match corpus {
        None => put(out, format_args!("ast generate.corpus absent")),
        Some(path) => put(
            out,
            format_args!("ast generate.corpus present {}", quoted(path.as_str())),
        ),
    }
}

fn observe_strategy(out: &mut String, strategy: StrategyPolicy) {
    match strategy {
        StrategyPolicy::Auto => put(out, format_args!("ast generate.strategy auto")),
        StrategyPolicy::Named(name) => {
            let spelled = match name {
                StrategyName::RhythmCopy => "rhythm_copy",
                StrategyName::MotifTranspose => "motif_transpose",
                StrategyName::ConstrainedWalk => "constrained_walk",
                StrategyName::ShuffleMotifs => "shuffle_motifs",
                StrategyName::RepeatVariation => "repeat_variation",
            };
            put(out, format_args!("ast generate.strategy named {spelled}"));
        }
    }
}

fn observe_export(out: &mut String, export: &Export) {
    let Export { format: fmt, path } = export;
    let spelled = match fmt {
        ExportFormat::Midi => "midi",
    };
    put(out, format_args!("ast export.format {spelled}"));
    put(
        out,
        format_args!("ast export.path {}", quoted(path.as_str())),
    );
}

/// The diagnostic observation: code, span, and message, in the order the
/// parser returned them. Order is itself an observable (spec §5.5).
fn observe_diagnostic(out: &mut String, index: usize, d: &Diagnostic) {
    let Diagnostic {
        code,
        span,
        message,
    } = d;
    put(
        out,
        format_args!(
            "diagnostic {index} {code} {} {} {}",
            span.start,
            span.end,
            quoted(message)
        ),
    );
}

// ── the corpus ───────────────────────────────────────────────────────────
//
// A deliberate Law A corpus, not a fuzz museum: every case is a fixed,
// deterministic source, and between them they exercise both verdicts, every
// level-1 enum variant, both states of every optional, and every diagnostic
// code level 1 can emit. The checked-in fuzz seed is included as an input
// subset rather than stood in for.

/// One recorded case: a stable name and the exact bytes fed to the parser.
struct Case {
    name: &'static str,
    source: String,
}

/// A level-1 script assembled from replaceable parts, so a case can vary one
/// construct without restating the program.
#[derive(Clone, Copy)]
struct Script {
    kernel: &'static str,
    fractalize: &'static str,
    linearize: &'static str,
    map_rhythm: &'static str,
    generate: &'static str,
    export: &'static str,
}

/// The `generate` body every case shares but the strategy cases.
const GENERATE_AUTO: &str = "        source \"seed.gp5\"\n        bars 8\n        \
                             seed 42\n        candidates 2\n        strategy auto";

impl Script {
    const fn base() -> Self {
        Self {
            kernel: "X.X/XX./.XX",
            fractalize: "depth 1 max_cells 4096",
            linearize: "snake",
            map_rhythm: "unit 1/16 tail rest_pad",
            generate: GENERATE_AUTO,
            export: "midi \"out.mid\"",
        }
    }

    fn render(self) -> String {
        let Self {
            kernel,
            fractalize,
            linearize,
            map_rhythm,
            generate,
            export,
        } = self;
        format!(
            "swang 1\n\npattern p {{\n    ascii \"{kernel}\"\n    \
             |> fractalize {fractalize}\n    |> linearize {linearize}\n    \
             |> map_rhythm {map_rhythm}\n    |> generate {{\n{generate}\n    }}\n    \
             |> export {export}\n}}\n"
        )
    }
}

/// A case built from a script.
fn case(name: &'static str, script: Script) -> Case {
    Case {
        name,
        source: script.render(),
    }
}

/// A case whose only departure from the base script is the named strategy.
fn strategy_case(name: &'static str, generate: &'static str) -> Case {
    case(
        name,
        Script {
            generate,
            ..Script::base()
        },
    )
}

const GEN_RHYTHM_COPY: &str = "        source \"seed.gp5\"\n        bars 8\n        \
                               seed 42\n        candidates 2\n        strategy rhythm_copy";
const GEN_MOTIF_TRANSPOSE: &str = "        source \"seed.gp5\"\n        bars 8\n        \
                                   seed 42\n        candidates 2\n        strategy motif_transpose";
const GEN_CONSTRAINED_WALK: &str = "        source \"seed.gp5\"\n        bars 8\n        \
                                    seed 42\n        candidates 2\n        strategy constrained_walk";
const GEN_SHUFFLE_MOTIFS: &str = "        source \"seed.gp5\"\n        bars 8\n        \
                                  seed 42\n        candidates 2\n        strategy shuffle_motifs";

/// The sources level 1 accepts.
fn accepted_corpus() -> Vec<Case> {
    let base = Script::base();
    vec![
        Case {
            name: "fuzz_seed_reference",
            source: FUZZ_SEED.to_owned(),
        },
        case("minimal_no_prune_no_corpus", base),
        case(
            "prune_present",
            Script {
                fractalize: "depth 1 max_cells 4096 density 9500bps seed 4",
                ..base
            },
        ),
        case(
            "row_major_and_reject_tail",
            Script {
                linearize: "row_major",
                map_rhythm: "unit 1/16 tail reject",
                ..base
            },
        ),
        case(
            "words_reordered_off_canonical",
            Script {
                fractalize: "max_cells 4096 depth 1",
                ..base
            },
        ),
        strategy_case("strategy_rhythm_copy", GEN_RHYTHM_COPY),
        strategy_case("strategy_motif_transpose", GEN_MOTIF_TRANSPOSE),
        strategy_case("strategy_constrained_walk", GEN_CONSTRAINED_WALK),
        strategy_case("strategy_shuffle_motifs", GEN_SHUFFLE_MOTIFS),
    ]
}

/// The header pre-parser's three refusals (spec §1.1, frozen).
fn rejected_header_corpus() -> Vec<Case> {
    vec![
        Case {
            name: "header_level_newer_than_build",
            source: Script::base().render().replace("swang 1", "swang 2"),
        },
        Case {
            name: "header_malformed_missing_space",
            source: Script::base().render().replace("swang 1", "swang1"),
        },
        Case {
            name: "header_byte_order_mark",
            source: format!("\u{feff}{}", Script::base().render()),
        },
    ]
}

/// The kernel literal's own registry laws, in the transport's order.
fn rejected_kernel_corpus() -> Vec<Case> {
    let base = Script::base();
    vec![
        case(
            "kernel_whitespace",
            Script {
                kernel: "X X/XX.",
                ..base
            },
        ),
        case(
            "kernel_empty_row",
            Script {
                kernel: "X.X//XX",
                ..base
            },
        ),
        case(
            "kernel_ragged",
            Script {
                kernel: "X.X/XX",
                ..base
            },
        ),
        case(
            "kernel_foreign_cell",
            Script {
                kernel: "XO/XX",
                ..base
            },
        ),
    ]
}

/// The word-level refusals: unknown, missing, repeated, out of range, and
/// the structural catch-all.
fn rejected_word_corpus() -> Vec<Case> {
    let base = Script::base();
    let second_pattern = format!("{}\npattern q {{\n}}\n", base.render().trim_end());
    vec![
        case(
            "unit_zero_numerator",
            Script {
                map_rhythm: "unit 0/16 tail rest_pad",
                ..base
            },
        ),
        case(
            "density_without_seed",
            Script {
                fractalize: "depth 1 max_cells 4096 density 9500bps",
                ..base
            },
        ),
        case(
            "density_out_of_range",
            Script {
                fractalize: "depth 1 max_cells 4096 density 10001bps seed 4",
                ..base
            },
        ),
        case(
            "unknown_traversal",
            Script {
                linearize: "spiral",
                ..base
            },
        ),
        case(
            "missing_max_cells",
            Script {
                fractalize: "depth 1",
                ..base
            },
        ),
        case(
            "repeated_word",
            Script {
                fractalize: "depth 1 depth 2 max_cells 4096",
                ..base
            },
        ),
        Case {
            name: "second_pattern_block",
            source: second_pattern,
        },
    ]
}

/// The whole corpus, in the order the baseline records it.
fn corpus() -> Vec<Case> {
    let mut all = accepted_corpus();
    all.extend(rejected_header_corpus());
    all.extend(rejected_kernel_corpus());
    all.extend(rejected_word_corpus());
    all
}

// ── the recorded document ────────────────────────────────────────────────

/// Renders the observation document for the whole corpus. This is the only
/// producer of the baseline's content, and nothing in the repository writes
/// its output back to disk: the artifact is regenerated by hand, under
/// review, or not at all.
pub(crate) fn render_document() -> String {
    let cases = corpus();
    let mut out = String::new();
    put(&mut out, format_args!("schema {SCHEMA}"));
    put(&mut out, format_args!("producer {PRODUCER}"));
    put(&mut out, format_args!("cases {}", cases.len()));
    for entry in &cases {
        out.push('\n');
        put(&mut out, format_args!("case {}", entry.name));
        put(&mut out, format_args!("source {}", quoted(&entry.source)));
        observe_outcome(&mut out, &entry.source);
        put(&mut out, format_args!("end"));
    }
    out
}

/// Observes one source. The observables are outcome-dependent: an accepted
/// source has an AST and canonical bytes, a rejected one has ordered
/// diagnostics, and neither borrows a null-filled field from the other.
fn observe_outcome(out: &mut String, source: &str) {
    match parse(source) {
        Ok(program) => {
            put(out, format_args!("verdict accepted"));
            observe_program(out, &program);
            let canonical = format(&program);
            put(out, format_args!("canonical {}", quoted(&canonical)));
        }
        Err(diagnostics) => {
            put(out, format_args!("verdict rejected"));
            for (index, diagnostic) in diagnostics.iter().enumerate() {
                observe_diagnostic(out, index, diagnostic);
            }
        }
    }
}

/// The AST observation alone, for the witnesses that compare two trees.
fn observed(program: &Program) -> String {
    let mut out = String::new();
    observe_program(&mut out, program);
    out
}

// ── the tests ────────────────────────────────────────────────────────────

/// Every diagnostic code language level 1 can emit. Spec §5.10 freezes
/// these; a level-2 build must still produce exactly them for a `swang 1`
/// source, which is why the corpus has to reach all of them.
const LEVEL_ONE_CODES: &[&str] = &[
    "SWG0001", "SWG0002", "SWG0003", "SWG0101", "SWG0102", "SWG0103", "SWG0301", "SWG0303",
    "SWG0307", "SWG0308", "SWG0401", "SWG0402", "SWG0403", "SWG0404",
];

#[test]
fn the_recorded_baseline_is_what_this_build_still_produces() {
    assert_eq!(
        render_document(),
        BASELINE,
        "Law A: a level-1 source's verdict, AST, canonical bytes, and \
         diagnostics must not move. Regenerating this artifact is not the \
         fix — the parser changed, or the corpus did."
    );
}

#[test]
fn the_baseline_names_the_build_that_produced_it() {
    let mut lines = BASELINE.lines();
    assert_eq!(lines.next(), Some(format!("schema {SCHEMA}").as_str()));
    assert_eq!(lines.next(), Some(format!("producer {PRODUCER}").as_str()));
}

#[test]
fn the_corpus_reaches_every_level_one_diagnostic_code() {
    let mut seen: Vec<&str> = Vec::new();
    for entry in &corpus() {
        if let Err(diagnostics) = parse(&entry.source) {
            for diagnostic in &diagnostics {
                if !seen.contains(&diagnostic.code) {
                    seen.push(diagnostic.code);
                }
            }
        }
    }
    for code in LEVEL_ONE_CODES {
        assert!(
            seen.contains(code),
            "no corpus case reaches {code}; Law A would go unwitnessed there"
        );
    }
    for code in &seen {
        assert!(
            LEVEL_ONE_CODES.contains(code),
            "{code} is not in the frozen level-1 registry list"
        );
    }
}

#[test]
fn the_corpus_records_both_verdicts_and_names_every_case_once() {
    let all = corpus();
    let accepted = all.iter().filter(|c| parse(&c.source).is_ok()).count();
    assert_eq!(accepted, accepted_corpus().len());
    assert!(accepted < all.len(), "the corpus must record refusals too");
    let mut names: Vec<&str> = all.iter().map(|c| c.name).collect();
    names.sort_unstable();
    let before = names.len();
    names.dedup();
    assert_eq!(names.len(), before, "case names are the artifact's keys");
}

/// One leaf mutation: a name for the failure message, and the edit.
type Mutation = (&'static str, fn(&mut Program));

/// Every leaf of the base script's AST that has a second inhabitant.
///
/// Two observed leaves are deliberately absent, because no mutation of them
/// exists to write: `level` accepts only `1` on this build, and
/// `ExportFormat` has exactly one variant. Both are still observed, and both
/// gain a mutation the moment they gain an inhabitant — the exhaustive
/// `match` in the projection will not compile until they do.
fn structural_mutations() -> Vec<Mutation> {
    vec![
        ("pattern.name", |p| {
            p.pattern.name = Ident::new("q").expect("an identifier");
        }),
        ("pattern.kernel", |p| {
            p.pattern.kernel = KernelLiteral::new("XX/XX").expect("a kernel");
        }),
        ("fractalize.depth", |p| p.pattern.fractalize.depth = 2),
        ("fractalize.max_cells", |p| {
            p.pattern.fractalize.max_cells = 4097;
        }),
        ("fractalize.prune presence", |p| {
            p.pattern.fractalize.prune = Some(Prune {
                density: DensityBps::new(1).expect("in scale"),
                seed: 0,
            });
        }),
        ("linearize.traversal", |p| {
            p.pattern.linearize.traversal = Traversal::RowMajor;
        }),
        ("map_rhythm.unit.numerator", |p| {
            p.pattern.map_rhythm.unit = Unit::new(2, 16).expect("a unit");
        }),
        ("map_rhythm.unit.denominator", |p| {
            p.pattern.map_rhythm.unit = Unit::new(1, 8).expect("a unit");
        }),
        ("map_rhythm.tail", |p| {
            p.pattern.map_rhythm.tail = TailPolicy::Reject;
        }),
    ]
}

/// The `generate` and `export` leaves.
fn edge_mutations() -> Vec<Mutation> {
    vec![
        ("generate.source", |p| {
            p.pattern.generate.source = StringLiteral::new("other.gp5").expect("a path");
        }),
        ("generate.bars", |p| p.pattern.generate.bars = 9),
        ("generate.seed", |p| p.pattern.generate.seed = 43),
        ("generate.candidates", |p| p.pattern.generate.candidates = 3),
        ("generate.strategy", |p| {
            p.pattern.generate.strategy = StrategyPolicy::Named(StrategyName::RepeatVariation);
        }),
        ("generate.corpus presence", |p| {
            p.pattern.generate.corpus = Some(StringLiteral::new("corpus").expect("a path"));
        }),
        ("export.path", |p| {
            p.pattern.export.path = StringLiteral::new("other.mid").expect("a path");
        }),
    ]
}

/// The two leaves that only exist once pruning is present.
fn pruning_mutations() -> Vec<Mutation> {
    vec![
        ("prune.density", |p| {
            if let Some(prune) = p.pattern.fractalize.prune.as_mut() {
                prune.density = DensityBps::new(1).expect("in scale");
            }
        }),
        ("prune.seed", |p| {
            if let Some(prune) = p.pattern.fractalize.prune.as_mut() {
                prune.seed = 5;
            }
        }),
    ]
}

/// Proves each mutation actually moves the recorded observation.
fn assert_every_leaf_moves(base: &Program, mutations: Vec<Mutation>) {
    let baseline = observed(base);
    for (leaf, mutate) in mutations {
        let mut mutated = base.clone();
        mutate(&mut mutated);
        assert_ne!(
            observed(&mutated),
            baseline,
            "changing {leaf} left the AST observation identical, so the \
             baseline is not watching that leaf"
        );
    }
}

#[test]
fn the_ast_observation_notices_every_leaf_with_a_second_inhabitant() {
    // The projection is what makes the AST an *independent* witness from the
    // canonical bytes. A leaf it fails to move is a leaf a coordinated
    // parser and formatter change could rewrite while both recorded
    // observables stayed still.
    let base = parse(&Script::base().render()).expect("the base script parses");
    assert_every_leaf_moves(&base, structural_mutations());
    assert_every_leaf_moves(&base, edge_mutations());
}

#[test]
fn the_ast_observation_notices_both_pruning_leaves() {
    let script = Script {
        fractalize: "depth 1 max_cells 4096 density 9500bps seed 4",
        ..Script::base()
    };
    let base = parse(&script.render()).expect("the pruning script parses");
    assert!(base.pattern.fractalize.prune.is_some());
    assert_every_leaf_moves(&base, pruning_mutations());
}

#[test]
fn a_source_off_the_canonical_form_still_records_canonical_bytes() {
    // `words_reordered_off_canonical` exists so the recorded canonical bytes
    // are provably not a copy of the source: if they were, this case would
    // record its own off-canonical spelling.
    let script = Script {
        fractalize: "max_cells 4096 depth 1",
        ..Script::base()
    };
    let source = script.render();
    let program = parse(&source).expect("word order is free within a construct");
    let canonical = format(&program);
    assert_ne!(canonical, source);
    assert!(canonical.contains("depth 1 max_cells 4096"));
}

#[test]
fn every_recorded_code_has_the_one_registry_shape() {
    // The same law the `swang_parse` fuzz oracle asserts, kept where
    // `cargo test` runs it too: the registry has one shape, `SWG` and four
    // digits. `starts_with("SWG")` would accept `SWG`, `SWGxyz`, and
    // `SWG12345` as registry codes.
    let shaped = |code: &str| {
        code.strip_prefix("SWG")
            .is_some_and(|rest| rest.len() == 4 && rest.bytes().all(|b| b.is_ascii_digit()))
    };
    for code in LEVEL_ONE_CODES {
        assert!(shaped(code), "{code} is not SWG followed by four digits");
    }
    for entry in &corpus() {
        if let Err(diagnostics) = parse(&entry.source) {
            for diagnostic in &diagnostics {
                assert!(
                    shaped(diagnostic.code),
                    "{} in {}",
                    diagnostic.code,
                    entry.name
                );
            }
        }
    }
}
