//! The Swang evaluator (S16 Phase 3 → S8 Playground seam).
//!
//! Two layers, one boundary. [`compile_program`] knows only text: it parses
//! and statically checks a `.swg` source and returns a [`CompiledProgram`]
//! plus span diagnostics — no filesystem, no score, no generation.
//! [`evaluate_program`] runs a compiled program against **already resolved**
//! inputs ([`ResolvedProgramInputs`] — a loaded seed score and an optional
//! corpus snapshot) and returns an [`EvaluationResult`] in memory: the
//! expansion artifact, the ranked candidate set, the selected candidate, and
//! the program's own `export` declaration (which it does **not** execute).
//!
//! The evaluator knows nothing of a CLI, an editor, or a filesystem: path
//! resolution and file loading happen in the frontend shell before it runs,
//! and writing the export is a separate adapter after. That is what lets
//! `griff swang build` and the cockpit share one evaluator without either
//! learning about the other.

use std::ptr;

use griff_core::generate::GenerationStrategy;
use griff_core::generation_input::{
    ranked_candidates, select_ranked, CorpusMaterial, GenerationAsk, RankedSet,
};
use griff_core::score::Score;
use griff_pattern::NodePath;

use crate::pattern_compile::{
    compile_pattern_flaws, PatternFlaw, PatternPlan, RhythmPatternArgs, TailChoice, TraversalChoice,
};
use crate::syntax::{
    self, Diagnostic, ExportFormat, PatternDef, Program, ProgramSpans, Span, StrategyName,
    StrategyPolicy,
};

/// A statically-checked program: the parsed AST plus the source spans a
/// frontend renders diagnostics at. Text in, structure out — nothing
/// resolved, nothing run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledProgram {
    program: Program,
    spans: ProgramSpans,
}

impl CompiledProgram {
    /// The parsed program.
    #[must_use]
    pub const fn program(&self) -> &Program {
        &self.program
    }

    /// The source-span table.
    #[must_use]
    pub const fn spans(&self) -> &ProgramSpans {
        &self.spans
    }

    /// The seed-score path the program declares (`generate { source … }`).
    /// The frontend resolves this to bytes before evaluating.
    #[must_use]
    pub fn source_path(&self) -> &str {
        self.program.pattern.generate.source.as_str()
    }

    /// The corpus directory the program declares, if any.
    #[must_use]
    pub fn corpus_path(&self) -> Option<&str> {
        self.program
            .pattern
            .generate
            .corpus
            .as_ref()
            .map(syntax::StringLiteral::as_str)
    }

    /// The program's output edge — the single owner of the result (spec
    /// §3.2). Frontends read this to write the export; the evaluator never
    /// writes it.
    #[must_use]
    pub fn export(&self) -> ExportRequest {
        ExportRequest {
            format: self.program.pattern.export.format,
            path: self.program.pattern.export.path.as_str().to_owned(),
        }
    }
}

/// The program's `export` declaration — a request, never executed by the
/// evaluator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportRequest {
    /// The output format.
    pub format: ExportFormat,
    /// The output path, verbatim from the program.
    pub path: String,
}

/// Everything a program needs from the outside world, already resolved. The
/// evaluator takes loaded data, never paths — path resolution and I/O are the
/// frontend shell's job.
#[derive(Debug)]
pub struct ResolvedProgramInputs {
    /// The seed score `generate { source … }` named, already imported.
    pub source_score: Score,
    /// The corpus the program named, already loaded, if any.
    pub corpus: Option<CorpusMaterial>,
}

/// Where an evaluation diagnostic points, by §1.5's layers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiagLocation {
    /// A source span — syntax, transport-class, and score-borne facts.
    Span(Span),
    /// A structural `NodePath` — budget breaches born in the pattern core.
    Node(NodePath),
}

/// A diagnostic from evaluation: a stable registry code, a layered location,
/// and a message — pure data, rendered only at the frontend edge (spec §1.5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvalDiagnostic {
    /// The stable `SWG____` code.
    pub code: &'static str,
    /// Where the user's fix lives.
    pub location: DiagLocation,
    /// What went wrong.
    pub message: String,
}

/// The in-memory result of running a program: expansion, candidates,
/// selection, and the export declaration. No file was written.
#[derive(Debug)]
pub struct EvaluationResult {
    /// The canonical `griff.pattern-expansion` JSON (spec §1.14) — byte
    /// identical to `griff swang expand` and the Phase-2 transport artifact.
    pub expansion_artifact: String,
    /// The reranked candidate set (every strategy, full provenance).
    pub ranked: RankedSet,
    /// Index into `ranked.ranked` of the candidate the program's strategy
    /// policy selected (spec §3.5 law 5).
    pub selected: usize,
    /// The program's output edge — for the frontend's export adapter.
    pub export: ExportRequest,
}

impl EvaluationResult {
    /// The selected candidate's score — what a frontend draws and exports.
    #[must_use]
    #[allow(
        clippy::indexing_slicing,
        reason = "`selected` is an index `select` produced into this very set"
    )]
    pub fn selected_score(&self) -> &Score {
        &self.ranked.ranked[self.selected].value.score
    }

    /// The selected candidate's strategy — for a frontend's status line.
    #[must_use]
    #[allow(
        clippy::indexing_slicing,
        reason = "`selected` is an index `select` produced into this very set"
    )]
    pub fn selected_strategy(&self) -> GenerationStrategy {
        self.ranked.ranked[self.selected].value.strategy
    }
}

/// Parses and statically checks a Swang source (spec §3). Text only: no
/// inputs are resolved and nothing is generated.
///
/// # Errors
/// The parser's span diagnostics (`SWG0001`–`SWG0404`), never empty on `Err`.
pub fn compile_program(source: &str) -> Result<CompiledProgram, Vec<Diagnostic>> {
    let (program, spans) = syntax::parse_with_spans(source)?;
    Ok(CompiledProgram { program, spans })
}

/// Runs a compiled program's pattern pipeline up to `map_rhythm`.
///
/// Returns the expansion plan — templates plus the canonical artifact. This
/// is `expand`'s engine: no pitch generation happens.
///
/// # Errors
/// An [`EvalDiagnostic`] per §1.5 (structural `NodePath`, or the owning
/// word's span).
pub fn expand_program(
    compiled: &CompiledProgram,
    source_score: &Score,
) -> Result<PatternPlan, Vec<EvalDiagnostic>> {
    let pattern = &compiled.program.pattern;
    let args = rhythm_args(pattern);
    let bars = clamp_bars(pattern.generate.bars);
    compile_pattern_flaws(&args, source_score, bars)
        .map_err(|flaw| vec![flaw_to_diagnostic(flaw, &compiled.spans)])
}

/// Runs a compiled program end to end against resolved inputs.
///
/// Expansion, generation through the shared compiler, and strategy selection
/// (spec §3.5 law 5). Returns the result in memory; the caller owns the
/// export.
///
/// # Errors
/// An [`EvalDiagnostic`] per §1.5 — an expansion flaw, or an empty selection.
pub fn evaluate_program(
    compiled: &CompiledProgram,
    inputs: &ResolvedProgramInputs,
) -> Result<EvaluationResult, Vec<EvalDiagnostic>> {
    let pattern = &compiled.program.pattern;
    let plan = expand_program(compiled, &inputs.source_score)?;

    // A count that does not fit `usize` (reachable on wasm32) or that exceeds
    // the resource bound would drive an unbounded generation loop / OOM — the
    // F-004 family. Reject it typed, before `ranked_candidates` allocates.
    let ask = GenerationAsk {
        seed: pattern.generate.seed,
        bars: checked_count(pattern.generate.bars, MAX_BARS, "bars").map_err(|d| vec![d])?,
        variants_per_strategy: checked_count(
            pattern.generate.candidates,
            MAX_CANDIDATES,
            "candidates",
        )
        .map_err(|d| vec![d])?,
        gesture: true,
    };
    // `bars`/`candidates` were range-checked above, so a failure here is a
    // seed score that cannot seed the request (no pitch material, unusable
    // constraints) — a score-borne fact at the `source` word, never SWG0306
    // (which the registry reserves for expansion onsets, not the set).
    let set = ranked_candidates(
        &inputs.source_score,
        inputs.corpus.as_ref(),
        &ask,
        Some(plan.templates.as_slice()),
    )
    .map_err(|e| {
        vec![EvalDiagnostic {
            code: "SWG0310",
            location: DiagLocation::Span(compiled.spans.source),
            message: format!("the source score cannot seed generation: {e:?}"),
        }]
    })?;

    let selected = select(&set, pattern.generate.strategy).map_err(|d| vec![d])?;

    Ok(EvaluationResult {
        expansion_artifact: plan.artifact_json,
        ranked: set,
        selected,
        export: compiled.export(),
    })
}

/// The most bars a program may generate. Generous — a 4/4 piece this long
/// runs for days — so it never rejects a real program, only an absurd count
/// that would drive an unbounded generation loop (spec `SWG0309`).
const MAX_BARS: u64 = 100_000;

/// The most seed variants a program may ask for per strategy. The set holds
/// this × 5 strategies; a real program asks for single digits.
const MAX_CANDIDATES: u64 = 4_096;

/// A `generate` count validated into `usize`: rejected (`SWG0309`) when it is
/// zero (a request that generates nothing — the generator's `BarCountZero` /
/// `VariantCountZero`), or exceeds `max`, or does not fit the platform's
/// `usize` (reachable on wasm32). An out-of-range source value becomes a
/// typed diagnostic instead of a hang, an OOM, or a mis-coded downstream
/// failure. The bound is a runtime limit, not a language semantic, so it is
/// located at the tree root, never a source span.
fn checked_count(value: u64, max: u64, word: &str) -> Result<usize, EvalDiagnostic> {
    usize::try_from(value)
        .ok()
        .filter(|_| (1..=max).contains(&value))
        .ok_or_else(|| EvalDiagnostic {
            code: "SWG0309",
            location: DiagLocation::Node(NodePath::default()),
            message: format!("{word} {value} is outside the accepted range 1..={max}"),
        })
}

/// A program's bar count clamped into `usize` for the expansion artifact,
/// which allocates nothing per bar (`check_bars_window` reads only the
/// palette prefix), so `expand` keeps its byte-parity with the transport
/// even for an out-of-range count that `build` would reject.
fn clamp_bars(bars: u64) -> usize {
    usize::try_from(bars).unwrap_or(usize::MAX)
}

/// Builds the shared compiler's transport args from a program's typed
/// pattern pipeline — the one mapping `expand` and `build` share, so the two
/// cannot drift.
fn rhythm_args(pattern: &PatternDef) -> RhythmPatternArgs {
    RhythmPatternArgs {
        kernel: pattern.kernel.as_str().to_owned(),
        fractal_depth: pattern.fractalize.depth,
        density_bps: pattern
            .fractalize
            .prune
            .map(|prune| u32::from(prune.density.get())),
        rhythm_seed: pattern.fractalize.prune.map(|prune| prune.seed),
        traversal: match pattern.linearize.traversal {
            griff_pattern::Traversal::RowMajor => TraversalChoice::RowMajor,
            griff_pattern::Traversal::Snake => TraversalChoice::Snake,
        },
        unit: format!(
            "{}/{}",
            pattern.map_rhythm.unit.numerator(),
            pattern.map_rhythm.unit.denominator()
        ),
        max_cells: pattern.fractalize.max_cells,
        tail: match pattern.map_rhythm.tail {
            crate::TailPolicy::Reject => TailChoice::Reject,
            crate::TailPolicy::RestPad => TailChoice::RestPad,
        },
    }
}

/// The S6 strategy a program's named policy selects (spec §3.3).
const fn strategy_kind(name: StrategyName) -> GenerationStrategy {
    match name {
        StrategyName::RhythmCopy => GenerationStrategy::RhythmCopyPitchSubstitute,
        StrategyName::MotifTranspose => GenerationStrategy::MotifTransposeVariation,
        StrategyName::ConstrainedWalk => GenerationStrategy::ConstrainedRandomWalk,
        StrategyName::ShuffleMotifs => GenerationStrategy::ShuffleMotifs,
        StrategyName::RepeatVariation => GenerationStrategy::RepeatVariation,
    }
}

/// Maps a pattern-compilation flaw to a layered [`EvalDiagnostic`] (spec
/// §1.5): structural breaches keep their `NodePath`, score-borne facts sit at
/// the `source` word, time-domain flaws at the value that must change.
fn flaw_to_diagnostic(flaw: PatternFlaw, spans: &ProgramSpans) -> EvalDiagnostic {
    let at = |span: Span, code: &'static str, message: String| EvalDiagnostic {
        code,
        location: DiagLocation::Span(span),
        message,
    };
    match flaw {
        PatternFlaw::Kernel(d) | PatternFlaw::Density(d) => at(spans.kernel, d.code, d.message),
        PatternFlaw::Unit(d) => at(spans.unit, d.code, d.message),
        PatternFlaw::Score(d) => at(spans.source, d.code, d.message),
        PatternFlaw::Budget(e) => budget_diagnostic(&e),
        PatternFlaw::Lower(e) => lower_diagnostic(&e, spans),
        PatternFlaw::SilentExpansion => at(
            spans.kernel,
            "SWG0306",
            "the expansion produced no onsets — nothing to generate (change the kernel, \
             depth, density, or rhythm seed)"
                .to_owned(),
        ),
        PatternFlaw::SilentWindow { used } => at(
            spans.kernel,
            "SWG0306",
            format!(
                "the first {used} template(s) the bars window rotates over are all silent — \
                 nothing to generate (raise bars past the silent prefix, or change the \
                 kernel, depth, density, or rhythm seed)"
            ),
        ),
    }
}

/// A structural budget breach keeps its `NodePath` location.
fn budget_diagnostic(e: &griff_pattern::PatternError) -> EvalDiagnostic {
    match e {
        griff_pattern::PatternError::MaxCellsExceeded {
            path,
            needed,
            max_cells,
        } => EvalDiagnostic {
            code: "SWG0201",
            location: DiagLocation::Node(path.clone()),
            message: format!("the expansion needs {needed} cells, over the budget's {max_cells}"),
        },
        griff_pattern::PatternError::MaxDepthExceeded {
            path,
            depth,
            max_depth,
        } => EvalDiagnostic {
            code: "SWG0202",
            location: DiagLocation::Node(path.clone()),
            message: format!("depth {depth} exceeds the budget's max_depth {max_depth}"),
        },
        other => EvalDiagnostic {
            code: "SWG0101",
            location: DiagLocation::Node(NodePath::default()),
            message: other.to_string(),
        },
    }
}

/// A time-domain lowering flaw at the value that must change.
fn lower_diagnostic(e: &crate::LowerError, spans: &ProgramSpans) -> EvalDiagnostic {
    match e {
        crate::LowerError::UnitDoesNotDivideBar { bar_duration, unit } => EvalDiagnostic {
            code: "SWG0301",
            location: DiagLocation::Span(spans.unit),
            message: format!(
                "unit {} does not divide the {}-tick bar exactly",
                unit.0, bar_duration.0
            ),
        },
        crate::LowerError::ZeroUnit => EvalDiagnostic {
            code: "SWG0301",
            location: DiagLocation::Span(spans.unit),
            message: "the rhythm unit is zero ticks".to_owned(),
        },
        crate::LowerError::IncompleteFinalBar {
            have_slots,
            slots_per_bar,
        } => EvalDiagnostic {
            code: "SWG0302",
            location: DiagLocation::Span(spans.tail),
            message: format!(
                "the final bar holds {have_slots} of {slots_per_bar} slots; a rest_pad tail \
                 pads it with timed rests"
            ),
        },
    }
}

/// The candidate the strategy policy selects, or an error naming the empty
/// set.
fn select(set: &RankedSet, policy: StrategyPolicy) -> Result<usize, EvalDiagnostic> {
    let target = match policy {
        StrategyPolicy::Auto => None,
        StrategyPolicy::Named(name) => Some(strategy_kind(name)),
    };
    // An empty candidate set is SWG0310 — the registry keeps SWG0306 for
    // expansion onsets and says explicitly it is *not* the candidate set.
    let chosen = select_ranked(set, target).ok_or_else(|| EvalDiagnostic {
        code: "SWG0310",
        location: DiagLocation::Node(NodePath::default()),
        message: target.map_or_else(
            || "no candidate survived scoring".to_owned(),
            |strategy| format!("no ranked candidate of strategy {strategy:?} survived scoring"),
        ),
    })?;
    // `select_ranked` returns a borrow into `set.ranked`; recover its index.
    Ok(set
        .ranked
        .iter()
        .position(|c| ptr::eq(c, chosen))
        .unwrap_or(0))
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::missing_assert_message,
    clippy::arithmetic_side_effects,
    clippy::absolute_paths,
    clippy::str_to_string
)]
mod tests {
    use griff_core::event::{NoteMarks, Pitch, Tempo, Ticks, TimeSignature, Tuning, Velocity};
    use griff_core::generation_input::{ranked_candidates, select_ranked, GenerationAsk};
    use griff_core::score::{
        AtomEvent, AtomNote, EventGroup, EventGroupKind, LossReport, MasterBar, RepeatMarker,
        Score, Track, Voice,
    };
    use griff_core::slice::TickRange;

    use super::{compile_program, evaluate_program, expand_program, ResolvedProgramInputs};

    const PPQN: u16 = 480;
    const BAR: u32 = 1920;

    /// A minimal 4/4 score sounding quarters — enough to seed generation.
    fn seed_score(bar_count: usize) -> Score {
        let master_bars = (0..bar_count)
            .map(|i| {
                let start = u32::try_from(i).unwrap() * BAR;
                MasterBar {
                    index: i,
                    tick_range: TickRange::new(Ticks(start), Ticks(start + BAR)).expect("ordered"),
                    time_signature: TimeSignature {
                        numerator: 4,
                        denominator: 4,
                    },
                    tempo: Tempo::from_bpm_integer(120).expect("valid tempo"),
                    repeat: RepeatMarker::default(),
                }
            })
            .collect();
        let mut groups = Vec::new();
        for bar in 0..bar_count {
            let bar_start = u32::try_from(bar).unwrap() * BAR;
            for beat in 0..4_u32 {
                groups.push(EventGroup {
                    kind: EventGroupKind::Single,
                    atoms: vec![AtomEvent::Note(AtomNote {
                        absolute_start: Ticks(bar_start + beat * 480),
                        duration: Ticks(480),
                        pitch: Pitch::new(40 + u8::try_from(beat).unwrap()).expect("pitch"),
                        velocity: Velocity::new(90).expect("velocity"),
                        marks: NoteMarks::empty(),
                        position: None,
                    })],
                    technique_spans: Vec::new(),
                });
            }
        }
        Score {
            ticks_per_quarter: PPQN,
            master_bars,
            tracks: vec![Track {
                name: Some("seed".to_string()),
                channel: 0,
                voices: vec![Voice {
                    id: 0,
                    event_groups: groups,
                }],
                tuning: Tuning::standard_e(),
            }],
            source_meta: None,
            loss: LossReport::new(),
        }
    }

    /// A program around a replaceable strategy word. `source` is a placeholder
    /// path — the evaluator never reads it (inputs are pre-resolved).
    fn program(strategy: &str) -> String {
        format!(
            r#"swang 1

pattern p {{
    ascii "X.X/XX./.XX"
    |> fractalize depth 1 max_cells 4096 density 9500bps seed 4
    |> linearize snake
    |> map_rhythm unit 1/16 tail rest_pad
    |> generate {{
        source "seed.gp5"
        bars 4
        seed 42
        candidates 2
        strategy {strategy}
    }}
    |> export midi "out.mid"
}}
"#
        )
    }

    fn onsets(score: &Score) -> Vec<u32> {
        let mut all: Vec<u32> = score.tracks[0].voices[0]
            .event_groups
            .iter()
            .flat_map(|g| g.atoms.iter())
            .filter_map(|a| match a {
                AtomEvent::Note(n) => Some(n.absolute_start.0),
                AtomEvent::Rest(_) => None,
            })
            .collect();
        all.sort_unstable();
        all
    }

    #[test]
    fn evaluate_auto_matches_ranked_candidates_directly() {
        let compiled = compile_program(&program("auto")).expect("parses");
        let inputs = ResolvedProgramInputs {
            source_score: seed_score(4),
            corpus: None,
        };
        let result = evaluate_program(&compiled, &inputs).expect("evaluates");

        // The plan's templates fed to ranked_candidates directly must produce
        // the same reranked winner the evaluator selected.
        let plan = expand_program(&compiled, &seed_score(4)).expect("expands");
        let direct = ranked_candidates(
            &seed_score(4),
            None,
            &GenerationAsk {
                seed: 42,
                bars: 4,
                variants_per_strategy: 2,
                gesture: true,
            },
            Some(plan.templates.as_slice()),
        )
        .expect("ranks");
        let winner = select_ranked(&direct, None).expect("a winner");
        assert_eq!(
            onsets(result.selected_score()),
            onsets(&winner.value.score),
            "auto selects the reranked winner, same as a direct call"
        );
    }

    #[test]
    fn evaluate_named_selects_that_strategy() {
        let compiled = compile_program(&program("repeat_variation")).expect("parses");
        let inputs = ResolvedProgramInputs {
            source_score: seed_score(4),
            corpus: None,
        };
        let result = evaluate_program(&compiled, &inputs).expect("evaluates");
        assert_eq!(
            result.ranked.ranked[result.selected].value.strategy,
            griff_core::generate::GenerationStrategy::RepeatVariation,
            "a named strategy selects that strategy's first ranked candidate"
        );
    }

    #[test]
    fn expand_artifact_is_byte_stable_and_generation_free() {
        let compiled = compile_program(&program("auto")).expect("parses");
        let a = expand_program(&compiled, &seed_score(4)).expect("expands");
        let b = expand_program(&compiled, &seed_score(4)).expect("expands");
        assert_eq!(a.artifact_json, b.artifact_json, "byte-stable");
        assert!(a.artifact_json.ends_with('\n'));
        let v: serde_json::Value =
            serde_json::from_str(&a.artifact_json).expect("valid JSON artifact");
        assert_eq!(v["schema"], "griff.pattern-expansion");
    }

    #[test]
    fn compile_reports_span_diagnostics_without_touching_inputs() {
        // A seedless density is SWG0303 — compile alone catches it, no score.
        let bad = program("auto").replace("density 9500bps seed 4", "density 9500bps");
        let diags = compile_program(&bad).expect_err("seedless density");
        assert_eq!(diags[0].code, "SWG0303");
    }

    #[test]
    fn an_out_of_range_count_is_rejected_not_clamped() {
        let inputs = ResolvedProgramInputs {
            source_score: seed_score(4),
            corpus: None,
        };
        let range_reject = |src: &str, what: &str| {
            let compiled = compile_program(src).expect("parses");
            let diags = evaluate_program(&compiled, &inputs).expect_err(what);
            assert_eq!(diags[0].code, "SWG0309", "{what}");
        };
        // Oversized would hang/OOM (on wasm32 not even fit usize); zero would
        // generate nothing. Both are SWG0309, never a clamp or a mis-code.
        range_reject(
            &program("auto").replace("bars 4", "bars 999999999"),
            "oversized bars",
        );
        range_reject(
            &program("auto").replace("candidates 2", "candidates 99999"),
            "oversized candidates",
        );
        range_reject(&program("auto").replace("bars 4", "bars 0"), "zero bars");
        range_reject(
            &program("auto").replace("candidates 2", "candidates 0"),
            "zero candidates",
        );
    }

    #[test]
    fn a_source_that_cannot_seed_is_swg0310_at_the_source_word_not_swg0306() {
        // A seed score with no sounding notes has no pitch material, so
        // ranked_candidates rejects it — a score-borne fact at the `source`
        // word, never SWG0306 (which is about expansion onsets).
        let mut silent = seed_score(4);
        for track in &mut silent.tracks {
            for voice in &mut track.voices {
                voice.event_groups.clear();
            }
        }
        let compiled = compile_program(&program("auto")).expect("parses");
        let inputs = ResolvedProgramInputs {
            source_score: silent,
            corpus: None,
        };
        let diags = evaluate_program(&compiled, &inputs).expect_err("no pitch material");
        assert_eq!(diags[0].code, "SWG0310");
        assert!(
            matches!(diags[0].location, super::DiagLocation::Span(_)),
            "a score-borne failure locates at the source word"
        );
    }

    #[test]
    fn evaluate_exposes_the_programs_export_without_writing_it() {
        let compiled = compile_program(&program("auto")).expect("parses");
        let inputs = ResolvedProgramInputs {
            source_score: seed_score(4),
            corpus: None,
        };
        let result = evaluate_program(&compiled, &inputs).expect("evaluates");
        assert_eq!(result.export.path, "out.mid");
        assert_eq!(compiled.export().path, "out.mid");
    }
}
