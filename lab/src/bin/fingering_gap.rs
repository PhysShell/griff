//! Fingering optimality-gap runner — the Constraint Lab's optimization phase.
//!
//! Measures fingering models against two different references:
//!
//! 1. **An external optimum** (SLOTHY-style oracle): each model's objective is
//!    exported as solver-neutral IR, solved by OR-Tools CP-SAT
//!    (`cpsat/solve_opt.py`), and every returned optimum is re-verified here
//!    (`optir::verify_record`) before it is compared with the in-repo DP.
//! 2. **Human tablature**: the `(string, fret)` choices of the tab authors in a
//!    Guitar Pro corpus — per-note agreement, how often the human fingering is
//!    itself optimal under the model, and by how much it is not.
//!
//! ```text
//! cargo run --release --bin fingering_gap -- fit    --tabs DIR --out DIR
//! cargo run --release --bin fingering_gap -- export --tabs DIR --out DIR [MODELS]
//! python cpsat/solve_opt.py OUT/NAME.problems.jsonl OUT/NAME.cpsat.jsonl --agreement
//! cargo run --release --bin fingering_gap -- report --tabs DIR --out DIR [MODELS]
//! ```
//!
//! Repeat consistency (a global constraint no chain DP state holds):
//!
//! ```text
//! cargo run --release --bin fingering_gap -- repeat-export --tabs DIR --out DIR [MODELS]
//! python cpsat/solve_opt.py OUT/NAME.tie.problems.jsonl        OUT/NAME.tie.cpsat.jsonl
//! python cpsat/solve_opt.py OUT/NAME.tie-repeat.problems.jsonl OUT/NAME.tie-repeat.cpsat.jsonl
//! cargo run --release --bin fingering_gap -- repeat-report --tabs DIR --out DIR [MODELS]
//! ```
//!
//! Optimum-set analysis and a learned secondary tie-break (exact DPs):
//!
//! ```text
//! cargo run --release --bin fingering_gap -- ties-check --tabs DIR --out DIR --v1 NAME=…
//! cargo run --release --bin fingering_gap -- tiebreak   --tabs DIR --out DIR --v1 NAME=…
//! ```
//!
//! `ties-check` compares the exact DP optimum and agreement ceiling with the
//! verified CP-SAT records in `OUT/NAME.cpsat.jsonl`; `tiebreak` learns a
//! secondary objective on train songs (margin chosen on a validation bucket)
//! and reports the tie-break ladder on holdout songs.
//!
//! Technique-aware fingering, oracle stage (tap labels from the tab):
//!
//! ```text
//! cargo run --release --bin fingering_gap -- taps --tabs DIR --out DIR
//! ```
//!
//! compares the tap-blind `v1` objective with the tap-aware one under the same
//! weights on lines with tapped notes, on the whole corpus and on holdout songs.
//!
//! Oracle stage 2, legato continuity — phase 1, the census of observed legato
//! edges (protocol: `docs/audit/2026-09-fingering-legato-continuity.md`):
//!
//! ```text
//! cargo run --release --bin fingering_gap -- legato-census --tabs DIR --out DIR
//! ```
//!
//! and phase 2, the registered ablation A / B / C1 / C2(k) / D1 / D2 with
//! per-stage length-matched baselines and the leave-one-song-out check:
//!
//! ```text
//! cargo run --release --bin fingering_gap -- legato --tabs DIR --out DIR
//! ```
//!
//! Chord-target feasibility oracle (diagnostic-only follow-up):
//!
//! ```text
//! cargo run --release --bin fingering_gap -- legato-chords --tabs DIR --out DIR
//! ```
//!
//! Preceding-context follow-up (B0/O/A/lexicographic/Pareto):
//!
//! ```text
//! cargo run --release --bin fingering_gap -- legato-chord-context --tabs DIR --out DIR
//! ```
//!
//! `MODELS`: `--v1 NAME=fret,open_string,position_shift,string_change` and
//! `--hand NAME=height,open_string,stretch,shift,shift_distance,string_distance`,
//! repeatable; default `--v1 v1=1,1,2,1` (the production weights).
//!
//! Corpus-derived files (problems, solver records, per-line records) stay in
//! `--out`, which is never committed — tab content is licensed material
//! (ADR-0005). `report` prints and archives aggregates only.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use griff_constraint_lab::boundary_context::{
    condition_consumer_chain, consume_for_line, decode_context, encode_context, produce_context,
    BoundaryContext, ProjectedTechnique, SolvedNote, SolvedPartition, VoiceIdentity,
};
use griff_constraint_lab::chord::{
    analyze_chord, ChordAnalysis, ChordAtom, ChordCostPolicy, ChordOptimum, HumanChordAssessment,
    TargetStringConstraint, TargetStringResult,
};
use griff_constraint_lab::chord_context::{
    analyze_chord_context, ChordContext, ChordContextAnalysis, ContextClassification,
    ContextStringResult, ExactLexMinimum, ExactMinimum, HumanContextAssessment, RankChange,
};
use griff_constraint_lab::fingering::{
    best_hands, decode_positions, hand_problem, holdout_bucket, repeat_pairs, solve_hand, song_key,
    tab_lines, v1_cost, v1_problem, with_repeat_consistency, with_string_tiebreak,
    within_line_span, CrossLineBoundary, CutStats, HandModel, HandWeights, ImportedChordAtom,
    LineBoundary, LineBoundaryCause, LineCut, TabLine, TargetDisposition, TechniqueEdge,
    TechniqueKind, TechniqueSpanStats, HAND_VARS_PER_NOTE, V1_VARS_PER_NOTE,
};
use griff_constraint_lab::forensics::{distribution, top_n_longest, Distribution, ExactRatio};
use griff_constraint_lab::ir::VarId;
use griff_constraint_lab::optir::{
    verify_agreement, verify_record, OptProblem, ProblemRecord, SolveRecord, Verdict,
};
use griff_constraint_lab::technique::{
    derived_direction, direction_between, tap_aware_chain, tap_aware_cost, technique_chain,
    technique_cost, Continuity, LegatoDirection, TechniqueObjective,
};
use griff_constraint_lab::ties::{
    lexicographic_path, optimum_set, path_matches, train_secondary, Chain, Example, Features,
    PerceptronConfig, FEATURES, FEATURE_NAMES,
};
use griff_core::event::FretboardPosition;
use griff_core::fretboard::{infer_positions, FingeringWeights, STANDARD_MAX_FRET};
use griff_core::gp::import_gp_score;
use griff_core::ingest::select_ingest_tracks;
use serde::Serialize;

/// Song-level holdout: `holdout_bucket(song_key, 5) == 0` is the test split.
const HOLDOUT_BUCKETS: u64 = 5;

// ── corpus ────────────────────────────────────────────────────────────────────

struct Line {
    id: String,
    file: usize,
    test: bool,
    /// Song-level holdout bucket in `0..HOLDOUT_BUCKETS` (0 = test).
    bucket: u64,
    tab: TabLine,
}

#[derive(Serialize)]
struct CorpusFacts {
    files: usize,
    import_failures: usize,
    guitar_tracks: usize,
    songs_train: usize,
    songs_test: usize,
    lines_train: usize,
    lines_test: usize,
    notes_train: u64,
    notes_test: u64,
    /// FNV-1a 64 over the per-file content hashes, in file-name order.
    corpus_fingerprint_hex: String,
    cut: LineCut,
    cut_stats: CutStats,
}

struct Corpus {
    names: Vec<String>,
    lines: Vec<Line>,
    facts: CorpusFacts,
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |acc, &b| {
        (acc ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

fn load(tabs: &Path, cut: &LineCut) -> std::io::Result<Corpus> {
    let mut paths: Vec<PathBuf> = fs::read_dir(tabs)?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .collect();
    paths.sort();
    let mut names = Vec::with_capacity(paths.len());
    let mut lines = Vec::new();
    let mut stats = CutStats::default();
    let (mut import_failures, mut guitar_tracks) = (0, 0);
    let mut corpus_hash = Vec::with_capacity(paths.len() * 8);
    let mut songs: BTreeMap<String, bool> = BTreeMap::new();
    for (file, path) in paths.iter().enumerate() {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let bytes = fs::read(path)?;
        corpus_hash.extend_from_slice(&fnv1a64(&bytes).to_le_bytes());
        let key = song_key(&name);
        let bucket = holdout_bucket(&key, HOLDOUT_BUCKETS);
        let test = bucket == 0;
        names.push(name);
        let Ok(score) = import_gp_score(&bytes) else {
            import_failures += 1;
            continue;
        };
        songs.insert(key, test);
        for track in select_ingest_tracks(&score, false) {
            guitar_tracks += 1;
            let Ok((track_lines, track_stats)) = tab_lines(&score, track, cut) else {
                continue;
            };
            stats.absorb(&track_stats);
            lines.extend(track_lines.into_iter().map(|tab| Line {
                id: format!(
                    "f{file:03}.t{}.v{}.at{}",
                    tab.track, tab.voice, tab.start_tick
                ),
                file,
                test,
                bucket,
                tab,
            }));
        }
    }
    let notes = |test: bool| {
        lines
            .iter()
            .filter(|l| l.test == test)
            .map(|l| l.tab.pitches.len() as u64)
            .sum()
    };
    let facts = CorpusFacts {
        files: paths.len(),
        import_failures,
        guitar_tracks,
        songs_train: songs.values().filter(|t| !**t).count(),
        songs_test: songs.values().filter(|t| **t).count(),
        lines_train: lines.iter().filter(|l| !l.test).count(),
        lines_test: lines.iter().filter(|l| l.test).count(),
        notes_train: notes(false),
        notes_test: notes(true),
        corpus_fingerprint_hex: format!("{:016x}", fnv1a64(&corpus_hash)),
        cut: *cut,
        cut_stats: stats,
    };
    Ok(Corpus {
        names,
        lines,
        facts,
    })
}

fn par_map<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let threads = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
    let chunk = items.len().div_ceil(threads * 4).max(1);
    let f = &f;
    std::thread::scope(|scope| {
        let handles: Vec<_> = items
            .chunks(chunk)
            .map(|c| scope.spawn(move || c.iter().map(f).collect::<Vec<R>>()))
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().expect("worker thread panicked"))
            .collect()
    })
}

// ── models ────────────────────────────────────────────────────────────────────

#[derive(Clone)]
enum Model {
    /// Per note the lowest-fret candidate (prior-art baseline); no objective.
    LowestFret,
    /// The production DP (`infer_positions`) under the given weights.
    V1 {
        name: String,
        weights: FingeringWeights,
    },
    /// The hand-position model's exact DP.
    Hand { name: String, model: HandModel },
}

struct Prediction {
    positions: Vec<FretboardPosition>,
    /// The model's own cost of its prediction (`None` for the baseline).
    cost: Option<i64>,
}

impl Model {
    fn name(&self) -> &str {
        match self {
            Self::LowestFret => "lowest-fret",
            Self::V1 { name, .. } | Self::Hand { name, .. } => name,
        }
    }

    fn description_short(&self) -> String {
        match self {
            Self::V1 { name, weights: w } => format!(
                "{name} (fret {}, open_string {}, position_shift {}, string_change {})",
                w.fret, w.open_string, w.position_shift, w.string_change
            ),
            other => other.name().to_string(),
        }
    }

    fn describe(&self) -> String {
        match self {
            Self::LowestFret => "lowest fret per note".into(),
            Self::V1 { weights: w, .. } => format!(
                "production DP, fret={} open_string={} position_shift={} string_change={}",
                w.fret, w.open_string, w.position_shift, w.string_change
            ),
            Self::Hand { model, .. } => {
                let w = model.weights();
                format!(
                    "hand DP, height={} open_string={} stretch={} shift={} shift_distance={} string_distance={}",
                    w.height, w.open_string, w.stretch, w.shift, w.shift_distance, w.string_distance
                )
            }
        }
    }

    fn predict(&self, line: &TabLine) -> Prediction {
        match self {
            Self::LowestFret => Prediction {
                positions: line
                    .pitches
                    .iter()
                    .map(|&p| {
                        line.tuning
                            .candidates(p, STANDARD_MAX_FRET)
                            .into_iter()
                            .min_by_key(|c| c.fret)
                            .expect("tab lines only hold positionable pitches")
                    })
                    .collect(),
                cost: None,
            },
            Self::V1 { weights, .. } => {
                let positions: Vec<FretboardPosition> =
                    infer_positions(&line.pitches, &line.tuning, weights, STANDARD_MAX_FRET)
                        .into_iter()
                        .map(|p| p.expect("tab lines only hold positionable pitches"))
                        .collect();
                let cost = v1_cost(&positions, weights);
                Prediction {
                    positions,
                    cost: Some(cost),
                }
            }
            Self::Hand { model, .. } => {
                let sol = solve_hand(&line.pitches, &line.tuning, model)
                    .expect("tab lines only hold positionable pitches");
                Prediction {
                    positions: sol.positions,
                    cost: Some(sol.cost),
                }
            }
        }
    }

    fn human_cost(&self, line: &TabLine) -> Option<i64> {
        match self {
            Self::LowestFret => None,
            Self::V1 { weights, .. } => Some(v1_cost(&line.human, weights)),
            Self::Hand { model, .. } => best_hands(&line.human, model).map(|b| b.0),
        }
    }

    fn problem(&self, line: &TabLine) -> Option<(OptProblem, usize)> {
        match self {
            Self::LowestFret => None,
            Self::V1 { weights, .. } => {
                v1_problem(&line.pitches, &line.tuning, weights, STANDARD_MAX_FRET)
                    .ok()
                    .map(|p| (p, V1_VARS_PER_NOTE))
            }
            Self::Hand { model, .. } => hand_problem(&line.pitches, &line.tuning, model)
                .ok()
                .map(|p| (p, HAND_VARS_PER_NOTE)),
        }
    }
}

fn human_reference(line: &TabLine, vars_per_note: usize) -> Vec<(VarId, i64)> {
    line.human
        .iter()
        .enumerate()
        .map(|(i, p)| (VarId(i * vars_per_note), i64::from(p.string)))
        .collect()
}

fn parse_list(raw: &str) -> Result<Vec<i64>, String> {
    raw.split(',')
        .map(|x| x.trim().parse::<i64>().map_err(|e| format!("{x:?}: {e}")))
        .collect()
}

fn parse_model(flag: &str, spec: &str) -> Result<Model, String> {
    let (name, weights) = spec
        .split_once('=')
        .ok_or_else(|| format!("{flag} expects NAME=w1,w2,…, got {spec:?}"))?;
    let w = parse_list(weights)?;
    match (flag, w.as_slice()) {
        ("--v1", &[fret, open_string, position_shift, string_change]) => Ok(Model::V1 {
            name: name.into(),
            weights: FingeringWeights {
                fret,
                open_string,
                position_shift,
                string_change,
            },
        }),
        ("--hand", &[height, open_string, stretch, shift, shift_distance, string_distance]) => {
            let weights = HandWeights {
                height,
                open_string,
                stretch,
                shift,
                shift_distance,
                string_distance,
            };
            HandModel::new(weights, STANDARD_MAX_FRET)
                .map(|model| Model::Hand {
                    name: name.into(),
                    model,
                })
                .map_err(|e| e.to_string())
        }
        _ => Err(format!("{flag} {spec:?}: wrong weight count")),
    }
}

// ── metrics ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Default, Serialize)]
struct Agreement {
    notes: u64,
    agree: u64,
    lines: u64,
    exact_lines: u64,
}

impl Agreement {
    fn of(human: &[FretboardPosition], predicted: &[FretboardPosition]) -> Self {
        let agree = human.iter().zip(predicted).filter(|(a, b)| a == b).count() as u64;
        let notes = human.len() as u64;
        Self {
            notes,
            agree,
            lines: 1,
            exact_lines: u64::from(agree == notes),
        }
    }

    fn add(&mut self, other: Self) {
        self.notes += other.notes;
        self.agree += other.agree;
        self.lines += other.lines;
        self.exact_lines += other.exact_lines;
    }

    #[allow(clippy::cast_precision_loss)]
    fn rate(&self) -> f64 {
        if self.notes == 0 {
            0.0
        } else {
            self.agree as f64 / self.notes as f64
        }
    }
}

#[derive(Debug, Clone, Default, Serialize)]
struct Quantiles {
    count: usize,
    p50: i64,
    p75: i64,
    p90: i64,
    p99: i64,
    max: i64,
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
fn quantiles(mut values: Vec<i64>) -> Quantiles {
    values.sort_unstable();
    let at = |q: f64| {
        values
            .get(((values.len().saturating_sub(1)) as f64 * q).round() as usize)
            .copied()
            .unwrap_or(0)
    };
    Quantiles {
        count: values.len(),
        p50: at(0.5),
        p75: at(0.75),
        p90: at(0.9),
        p99: at(0.99),
        max: values.last().copied().unwrap_or(0),
    }
}

#[derive(Debug, Clone, Default, Serialize)]
struct SplitEval {
    agreement: Agreement,
    agreement_rate: f64,
    /// Lines whose human fingering is optimal under the model.
    human_optimal_lines: Option<u64>,
    /// `cost(human) − cost(model optimum)` per line.
    human_excess: Option<Quantiles>,
}

struct LineEval {
    test: bool,
    agreement: Agreement,
    excess: Option<i64>,
}

fn eval_line(model: &Model, line: &Line) -> LineEval {
    let prediction = model.predict(&line.tab);
    let excess = match (model.human_cost(&line.tab), prediction.cost) {
        (Some(human), Some(best)) => Some(human - best),
        _ => None,
    };
    LineEval {
        test: line.test,
        agreement: Agreement::of(&line.tab.human, &prediction.positions),
        excess,
    }
}

fn split_eval<'a>(evals: impl Iterator<Item = &'a LineEval>) -> SplitEval {
    let mut agreement = Agreement::default();
    let mut excess = Vec::new();
    let mut has_cost = false;
    for e in evals {
        agreement.add(e.agreement);
        if let Some(x) = e.excess {
            has_cost = true;
            excess.push(x);
        }
    }
    SplitEval {
        agreement,
        agreement_rate: agreement.rate(),
        human_optimal_lines: has_cost.then(|| excess.iter().filter(|&&x| x == 0).count() as u64),
        human_excess: has_cost.then(|| quantiles(excess)),
    }
}

#[derive(Debug, Clone, Serialize)]
struct ModelEval {
    name: String,
    description: String,
    all: SplitEval,
    train: SplitEval,
    test: SplitEval,
    in_repo_ms: u128,
}

fn evaluate(model: &Model, lines: &[Line]) -> ModelEval {
    let started = Instant::now();
    let evals = par_map(lines, |l| eval_line(model, l));
    let in_repo_ms = started.elapsed().as_millis();
    ModelEval {
        name: model.name().into(),
        description: model.describe(),
        all: split_eval(evals.iter()),
        train: split_eval(evals.iter().filter(|e| !e.test)),
        test: split_eval(evals.iter().filter(|e| e.test)),
        in_repo_ms,
    }
}

fn train_agreement(model: &Model, train: &[&Line]) -> u64 {
    par_map(train, |l| {
        Agreement::of(&l.tab.human, &model.predict(&l.tab).positions).agree
    })
    .into_iter()
    .sum()
}

// ── commands ──────────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct FitResult {
    corpus: CorpusFacts,
    v1_grid_evaluated: usize,
    v1_best: [i64; 4],
    hand_evaluated: usize,
    hand_best: [i64; 6],
    evaluations: Vec<ModelEval>,
}

fn fit(corpus: Corpus, out: &Path) -> std::io::Result<()> {
    let train: Vec<&Line> = corpus.lines.iter().filter(|l| !l.test).collect();

    // v1 family: exhaustive integer grid.
    let started = Instant::now();
    let mut grid = Vec::new();
    for fret in 0..=4 {
        for open_string in -4..=4 {
            for position_shift in 0..=6 {
                for string_change in 0..=6 {
                    grid.push([fret, open_string, position_shift, string_change]);
                }
            }
        }
    }
    let mut v1_best = ([1, 1, 2, 1], 0_u64);
    for w in &grid {
        let model = Model::V1 {
            name: String::new(),
            weights: FingeringWeights {
                fret: w[0],
                open_string: w[1],
                position_shift: w[2],
                string_change: w[3],
            },
        };
        let score = train_agreement(&model, &train);
        if score > v1_best.1 {
            v1_best = (*w, score);
        }
    }
    eprintln!(
        "v1 grid: {} weight sets in {:.1}s, best {:?}",
        grid.len(),
        started.elapsed().as_secs_f64(),
        v1_best.0
    );

    // Hand family: coordinate descent from several starts.
    let ranges: [(i64, i64); 6] = [(-2, 3), (-6, 6), (0, 10), (0, 12), (0, 6), (0, 6)];
    let starts: [[i64; 6]; 3] = [[1, 0, 2, 4, 1, 1], [0, 0, 0, 0, 0, 0], [0, -2, 4, 8, 0, 2]];
    let hand = |w: [i64; 6]| Model::Hand {
        name: String::new(),
        model: HandModel::new(
            HandWeights {
                height: w[0],
                open_string: w[1],
                stretch: w[2],
                shift: w[3],
                shift_distance: w[4],
                string_distance: w[5],
            },
            STANDARD_MAX_FRET,
        )
        .expect("ranges keep weights valid"),
    };
    let mut cache: HashMap<[i64; 6], u64> = HashMap::new();
    let mut score_of = |w: [i64; 6]| {
        *cache
            .entry(w)
            .or_insert_with(|| train_agreement(&hand(w), &train))
    };
    let mut hand_best = ([0_i64; 6], 0_u64);
    for start in starts {
        let mut current = (start, score_of(start));
        for pass in 0..8 {
            let before = current.1;
            for (coord, &(lo, hi)) in ranges.iter().enumerate() {
                for value in lo..=hi {
                    let mut w = current.0;
                    w[coord] = value;
                    let s = score_of(w);
                    if s > current.1 {
                        current = (w, s);
                    }
                }
            }
            eprintln!(
                "hand descent from {start:?}, pass {pass}: {:?} agree {} ({:.1}s)",
                current.0,
                current.1,
                started.elapsed().as_secs_f64()
            );
            if current.1 == before {
                break;
            }
        }
        if current.1 > hand_best.1 {
            hand_best = current;
        }
    }
    let hand_evaluated = cache.len();

    let models = [
        Model::LowestFret,
        parse_model("--v1", "v1=1,1,2,1").expect("production weights"),
        Model::V1 {
            name: "v1-fit".into(),
            weights: FingeringWeights {
                fret: v1_best.0[0],
                open_string: v1_best.0[1],
                position_shift: v1_best.0[2],
                string_change: v1_best.0[3],
            },
        },
        match hand(hand_best.0) {
            Model::Hand { model, .. } => Model::Hand {
                name: "hand-fit".into(),
                model,
            },
            other => other,
        },
    ];
    let evaluations: Vec<ModelEval> = models.iter().map(|m| evaluate(m, &corpus.lines)).collect();
    print_table(&evaluations);
    let result = FitResult {
        corpus: corpus.facts,
        v1_grid_evaluated: grid.len(),
        v1_best: v1_best.0,
        hand_evaluated,
        hand_best: hand_best.0,
        evaluations,
    };
    write_json(&out.join("fit.json"), &result)?;
    println!(
        "\nsuggested models: --v1 v1-fit={} --hand hand-fit={}",
        join(&v1_best.0),
        join(&hand_best.0)
    );
    Ok(())
}

fn join(values: &[i64]) -> String {
    values
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

fn export(corpus: &Corpus, models: &[Model], out: &Path) -> std::io::Result<()> {
    {
        let mut w = BufWriter::new(fs::File::create(out.join("lines.jsonl"))?);
        for line in &corpus.lines {
            let record = serde_json::json!({
                "id": line.id,
                "file": corpus.names.get(line.file),
                "test": line.test,
                "notes": line.tab.pitches.len(),
                "human": line.tab.human.iter().map(|p| [p.string, p.fret]).collect::<Vec<_>>(),
            });
            serde_json::to_writer(&mut w, &record).map_err(std::io::Error::other)?;
            w.write_all(b"\n")?;
        }
    }
    for model in models {
        let path = out.join(format!("{}.problems.jsonl", model.name()));
        let records = par_map(&corpus.lines, |line| {
            model.problem(&line.tab).map(|(problem, vpn)| {
                let record =
                    ProblemRecord::new(line.id.clone(), problem, human_reference(&line.tab, vpn));
                serde_json::to_string(&record).expect("problem records serialize")
            })
        });
        let mut w = BufWriter::new(fs::File::create(&path)?);
        let mut written = 0;
        for record in records.into_iter().flatten() {
            w.write_all(record.as_bytes())?;
            w.write_all(b"\n")?;
            written += 1;
        }
        eprintln!("{}: {written} problems → {}", model.name(), path.display());
    }
    Ok(())
}

#[derive(Debug, Clone, Default, Serialize)]
struct OracleEval {
    records: usize,
    /// Records re-solved by the adapter's multi-worker escalation tier.
    escalated: usize,
    missing: usize,
    proven: usize,
    not_proven: usize,
    fingerprint_mismatch: usize,
    witness_invalid: usize,
    objective_mismatch: usize,
    /// In-repo DP cost equals the verified optimum.
    gap_zero: usize,
    /// In-repo DP cost above the verified optimum (the heuristic is suboptimal).
    gap_positive: usize,
    /// In-repo DP cost *below* the verified optimum — impossible unless the
    /// encoding and the evaluator disagree; any count here is a defect.
    gap_negative: usize,
    max_gap: i64,
    /// Agreement passes verified (recounted, pinned to the optimum).
    agreement_verified: usize,
    agreement_refused: usize,
    /// Over lines with a verified agreement pass: notes, DP agreement, and the
    /// tie-insensitive ceiling at the optimum.
    ceiling_notes: u64,
    ceiling_dp_agree: u64,
    ceiling_best_agree: u64,
    solver: Option<String>,
    solver_wall_us: Quantiles,
    solver_total_s: f64,
}

#[allow(clippy::cast_precision_loss)]
fn oracle_eval(
    model: &Model,
    lines: &[Line],
    records: &HashMap<String, SolveRecord>,
) -> OracleEval {
    struct One {
        verdict: Option<Verdict>,
        in_repo: Option<i64>,
        agreement: Option<Result<u64, ()>>,
        notes: u64,
        dp_agree: u64,
        wall_us: Option<u64>,
    }
    let ones = par_map(lines, |line| {
        let Some(record) = records.get(&line.id) else {
            return One {
                verdict: None,
                in_repo: None,
                agreement: None,
                notes: 0,
                dp_agree: 0,
                wall_us: None,
            };
        };
        let (problem, vpn) = model.problem(&line.tab).expect("exported models build");
        let verdict = verify_record(&problem, record);
        let prediction = model.predict(&line.tab);
        let agreement = match (&verdict, &record.agreement) {
            (Verdict::Proven { optimum }, Some(pass)) => Some(
                verify_agreement(&problem, &human_reference(&line.tab, vpn), *optimum, pass)
                    .map_err(|_| ()),
            ),
            _ => None,
        };
        One {
            verdict: Some(verdict),
            in_repo: prediction.cost,
            agreement,
            notes: line.tab.human.len() as u64,
            dp_agree: Agreement::of(&line.tab.human, &prediction.positions).agree,
            wall_us: Some(record.wall_us),
        }
    });
    let mut e = OracleEval {
        records: records.len(),
        escalated: records
            .values()
            .filter(|r| r.solver.version.contains("escalated"))
            .count(),
        ..OracleEval::default()
    };
    let mut walls = Vec::new();
    for one in ones {
        let Some(verdict) = one.verdict else {
            e.missing += 1;
            continue;
        };
        if let Some(w) = one.wall_us {
            walls.push(i64::try_from(w).unwrap_or(i64::MAX));
        }
        match verdict {
            Verdict::Proven { optimum } => {
                e.proven += 1;
                let gap = one.in_repo.unwrap_or(optimum) - optimum;
                match gap.cmp(&0) {
                    std::cmp::Ordering::Equal => e.gap_zero += 1,
                    std::cmp::Ordering::Greater => e.gap_positive += 1,
                    std::cmp::Ordering::Less => e.gap_negative += 1,
                }
                e.max_gap = e.max_gap.max(gap);
            }
            Verdict::NotProven { .. } | Verdict::MissingWitness => e.not_proven += 1,
            Verdict::FingerprintMismatch => e.fingerprint_mismatch += 1,
            Verdict::WitnessInvalid(_) => e.witness_invalid += 1,
            Verdict::ObjectiveMismatch { .. } => e.objective_mismatch += 1,
        }
        match one.agreement {
            Some(Ok(best)) => {
                e.agreement_verified += 1;
                e.ceiling_notes += one.notes;
                e.ceiling_dp_agree += one.dp_agree;
                e.ceiling_best_agree += best;
            }
            Some(Err(())) => e.agreement_refused += 1,
            None => {}
        }
    }
    e.solver = records
        .values()
        .find(|r| !r.solver.version.contains("escalated"))
        .map(|r| format!("{} {}", r.solver.name, r.solver.version));
    e.solver_total_s = walls.iter().sum::<i64>() as f64 / 1e6;
    e.solver_wall_us = quantiles(walls);
    e
}

#[derive(Serialize)]
struct Report {
    schema: &'static str,
    version: u32,
    corpus: CorpusFacts,
    /// Files (≥ 50 kept notes) whose tab agrees ≥ 99% with the lowest-fret
    /// baseline — candidates for machine-generated fingering.
    lowest_fret_lookalike_files: usize,
    files_with_50_notes: usize,
    models: Vec<ModelEval>,
    oracle: BTreeMap<String, OracleEval>,
}

fn report(corpus: Corpus, models: &[Model], out: &Path) -> std::io::Result<()> {
    let mut all_models = vec![Model::LowestFret];
    all_models.extend(models.iter().cloned());
    let evaluations: Vec<ModelEval> = all_models
        .iter()
        .map(|m| evaluate(m, &corpus.lines))
        .collect();

    let mut per_file: BTreeMap<usize, Agreement> = BTreeMap::new();
    for (line, agreement) in corpus.lines.iter().zip(par_map(&corpus.lines, |l| {
        Agreement::of(&l.tab.human, &Model::LowestFret.predict(&l.tab).positions)
    })) {
        per_file.entry(line.file).or_default().add(agreement);
    }
    let big: Vec<&Agreement> = per_file.values().filter(|a| a.notes >= 50).collect();
    let lookalikes = big.iter().filter(|a| a.rate() >= 0.99).count();

    let mut oracle = BTreeMap::new();
    for model in models {
        let path = out.join(format!("{}.cpsat.jsonl", model.name()));
        let Ok(file) = fs::File::open(&path) else {
            continue;
        };
        let mut records = HashMap::new();
        for line in BufReader::new(file).lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let record: SolveRecord = serde_json::from_str(&line).map_err(std::io::Error::other)?;
            records.insert(record.id.clone(), record);
        }
        oracle.insert(
            model.name().to_string(),
            oracle_eval(model, &corpus.lines, &records),
        );
    }

    print_table(&evaluations);
    println!(
        "\nlowest-fret look-alike files (≥50 notes, ≥99% agreement): {lookalikes} of {}",
        big.len()
    );
    print_oracle(&oracle);
    let report = Report {
        schema: "griff.constraint-lab-fingering-gap",
        version: 1,
        corpus: corpus.facts,
        lowest_fret_lookalike_files: lookalikes,
        files_with_50_notes: big.len(),
        models: evaluations,
        oracle,
    };
    write_json(&out.join("report.json"), &report)
}

#[allow(clippy::cast_precision_loss)]
fn print_table(evaluations: &[ModelEval]) {
    println!("\n| model | agreement all | train | test (holdout songs) | exact lines (test) | human optimal (test) | human excess p50 / p90 (test) | in-repo ms |");
    println!("|---|---|---|---|---|---|---|---|");
    for e in evaluations {
        let optimal = e.test.human_optimal_lines.map_or("—".into(), |n| {
            format!(
                "{:.1}%",
                100.0 * n as f64 / e.test.agreement.lines.max(1) as f64
            )
        });
        let excess = e
            .test
            .human_excess
            .as_ref()
            .map_or("—".into(), |q| format!("{} / {}", q.p50, q.p90));
        println!(
            "| {} | {:.1}% | {:.1}% | {:.1}% | {:.1}% | {optimal} | {excess} | {} |",
            e.name,
            100.0 * e.all.agreement_rate,
            100.0 * e.train.agreement_rate,
            100.0 * e.test.agreement_rate,
            100.0 * e.test.agreement.exact_lines as f64 / e.test.agreement.lines.max(1) as f64,
            e.in_repo_ms
        );
    }
    for e in evaluations {
        println!("  {}: {}", e.name, e.description);
    }
}

#[allow(clippy::cast_precision_loss)]
fn print_oracle(oracle: &BTreeMap<String, OracleEval>) {
    if oracle.is_empty() {
        return;
    }
    println!("\n| model | records | not run | proven | not proven | invalid | gap = 0 | gap > 0 | gap < 0 | max gap | DP agreement | ceiling at optimum | solver p50 / p99 / max ms | solver total s |");
    println!("|---|---|---|---|---|---|---|---|---|---|---|---|---|---|");
    for (name, e) in oracle {
        // No agreement pass verified: say so rather than print 0%.
        let rate = |x: u64| {
            if e.ceiling_notes == 0 {
                "—".to_string()
            } else {
                format!("{:.1}%", 100.0 * x as f64 / e.ceiling_notes as f64)
            }
        };
        println!(
            "| {name} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {:.1} / {:.1} / {:.1} | {:.1} |",
            e.records,
            e.missing,
            e.proven,
            e.not_proven,
            e.witness_invalid + e.objective_mismatch + e.fingerprint_mismatch,
            e.gap_zero,
            e.gap_positive,
            e.gap_negative,
            e.max_gap,
            rate(e.ceiling_dp_agree),
            rate(e.ceiling_best_agree),
            e.solver_wall_us.p50 as f64 / 1e3,
            e.solver_wall_us.p99 as f64 / 1e3,
            e.solver_wall_us.max as f64 / 1e3,
            e.solver_total_s
        );
    }
    for (name, e) in oracle {
        if let Some(s) = &e.solver {
            println!(
                "  {name}: {s}; escalated {}; agreement passes verified {} refused {}",
                e.escalated, e.agreement_verified, e.agreement_refused
            );
        }
    }
}

// ── optimum sets and the learned tie-break ────────────────────────────────────

fn v1_weights(model: &Model) -> Option<FingeringWeights> {
    match model {
        Model::V1 { weights, .. } => Some(*weights),
        Model::LowestFret | Model::Hand { .. } => None,
    }
}

fn chain_of(line: &Line, weights: &FingeringWeights) -> Chain {
    Chain::v1(
        &line.tab.pitches,
        &line.tab.tuning,
        weights,
        STANDARD_MAX_FRET,
    )
    .expect("tab lines only hold positionable pitches")
}

/// A chain with or without the line's hand anchor (the tie-break ablation).
fn feature_chain(line: &Line, weights: &FingeringWeights, anchored: bool) -> Chain {
    let chain = chain_of(line, weights);
    if anchored {
        chain.with_anchor(line.tab.anchor_fret)
    } else {
        chain
    }
}

#[derive(Debug, Clone, Default, Serialize)]
struct TiesCheck {
    records: usize,
    compared: usize,
    optimum_equal: usize,
    optimum_differs: usize,
    ceiling_equal: usize,
    ceiling_differs: usize,
    skipped_unverified: usize,
}

/// The exact DPs against the verified CP-SAT optima and agreement passes.
fn ties_check(corpus: &Corpus, models: &[Model], out: &Path) -> std::io::Result<()> {
    let mut checks = BTreeMap::new();
    for model in models {
        let Some(weights) = v1_weights(model) else {
            continue;
        };
        let records = read_records(&out.join(format!("{}.cpsat.jsonl", model.name())))?;
        let rows = par_map(&corpus.lines, |line| {
            let record = records.get(&line.id)?;
            let (problem, vpn) = model.problem(&line.tab)?;
            let Verdict::Proven { optimum } = verify_record(&problem, record) else {
                return Some(None);
            };
            let ceiling = record.agreement.as_ref().and_then(|pass| {
                verify_agreement(&problem, &human_reference(&line.tab, vpn), optimum, pass).ok()
            })?;
            let set = optimum_set(&chain_of(line, &weights), Some(&line.tab.human));
            let max = set.agreement.map_or(0, |a| a.max) as u64;
            Some(Some((set.optimum == optimum, max == ceiling)))
        });
        let mut check = TiesCheck {
            records: records.len(),
            ..TiesCheck::default()
        };
        for row in rows.into_iter().flatten() {
            let Some((optimum_ok, ceiling_ok)) = row else {
                check.skipped_unverified += 1;
                continue;
            };
            check.compared += 1;
            if optimum_ok {
                check.optimum_equal += 1;
            } else {
                check.optimum_differs += 1;
            }
            if ceiling_ok {
                check.ceiling_equal += 1;
            } else {
                check.ceiling_differs += 1;
            }
        }
        println!(
            "{}: {} records, {} compared — optimum equal {} / differs {}, ceiling equal {} / differs {}, unverified {}",
            model.name(),
            check.records,
            check.compared,
            check.optimum_equal,
            check.optimum_differs,
            check.ceiling_equal,
            check.ceiling_differs,
            check.skipped_unverified
        );
        checks.insert(model.name().to_string(), check);
    }
    write_json(&out.join("ties-check.json"), &checks)
}

#[derive(Debug, Clone, Default, Serialize)]
struct Ladder {
    lines: usize,
    notes: u64,
    human_optimal_lines: usize,
    unique_optimum_lines: usize,
    ln_count: Quantiles,
    floor: f64,
    uniform: f64,
    production: f64,
    learned: Option<f64>,
    /// Lines where the learned tie-break picks a different path than production.
    learned_changed_lines: Option<usize>,
    ceiling: f64,
}

struct LineTies {
    notes: u64,
    human_optimal: bool,
    unique: bool,
    ln_count_milli: i64,
    min: u64,
    expected: f64,
    production: u64,
    max: u64,
}

#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
fn line_ties(line: &Line, weights: &FingeringWeights) -> LineTies {
    let chain = chain_of(line, weights);
    let human = &line.tab.human;
    let set = optimum_set(&chain, Some(human));
    let range = set.agreement.clone().expect("human positions per note");
    let production = lexicographic_path(&chain, &[0; FEATURES], None);
    LineTies {
        notes: human.len() as u64,
        human_optimal: v1_cost(human, weights) == set.optimum,
        unique: !set.count.saturated && set.count.exact == 1,
        ln_count_milli: (set.count.ln * 1000.0).round() as i64,
        min: range.min as u64,
        expected: range.expected,
        production: path_matches(&chain, &production, human).unwrap_or(0) as u64,
        max: range.max as u64,
    }
}

#[allow(clippy::cast_precision_loss)]
fn ladder(
    lines: &[&Line],
    weights: &FingeringWeights,
    learned: Option<&Features>,
    anchored: bool,
) -> Ladder {
    let rows = par_map(lines, |line| {
        let ties = line_ties(line, weights);
        let learned_matches = learned.map(|w| {
            let chain = feature_chain(line, weights, anchored);
            let path = lexicographic_path(&chain, w, None);
            let production = lexicographic_path(&chain, &[0; FEATURES], None);
            (
                path_matches(&chain, &path, &line.tab.human).unwrap_or(0) as u64,
                path != production,
            )
        });
        (ties, learned_matches)
    });
    let notes: u64 = rows.iter().map(|(t, _)| t.notes).sum();
    let rate = |x: f64| x / notes.max(1) as f64;
    Ladder {
        lines: rows.len(),
        notes,
        human_optimal_lines: rows.iter().filter(|(t, _)| t.human_optimal).count(),
        unique_optimum_lines: rows.iter().filter(|(t, _)| t.unique).count(),
        ln_count: quantiles(rows.iter().map(|(t, _)| t.ln_count_milli).collect()),
        floor: rate(rows.iter().map(|(t, _)| t.min as f64).sum()),
        uniform: rate(rows.iter().map(|(t, _)| t.expected).sum()),
        production: rate(rows.iter().map(|(t, _)| t.production as f64).sum()),
        learned: learned.map(|_| {
            rate(
                rows.iter()
                    .filter_map(|(_, l)| *l)
                    .map(|(x, _)| x as f64)
                    .sum(),
            )
        }),
        learned_changed_lines: learned.map(|_| {
            rows.iter()
                .filter(|(_, l)| l.is_some_and(|(_, changed)| changed))
                .count()
        }),
        ceiling: rate(rows.iter().map(|(t, _)| t.max as f64).sum()),
    }
}

fn examples_of(lines: &[&Line], weights: &FingeringWeights, anchored: bool) -> Vec<Example> {
    lines
        .iter()
        .map(|line| Example {
            chain: feature_chain(line, weights, anchored),
            human: line.tab.human.clone(),
        })
        .collect()
}

#[derive(Serialize)]
struct MarginTrial {
    margin: i64,
    epochs: usize,
    updates: u64,
    validation_agreement: f64,
}

#[derive(Serialize)]
struct TiebreakVariant {
    features: &'static str,
    trials: Vec<MarginTrial>,
    chosen_margin: i64,
    final_updates: u64,
    final_epochs: usize,
    weights: BTreeMap<&'static str, i64>,
    train: Ladder,
    test: Ladder,
}

#[derive(Serialize)]
struct TiebreakReport {
    schema: &'static str,
    version: u32,
    primary: String,
    epochs: usize,
    variants: Vec<TiebreakVariant>,
    corpus: CorpusFacts,
}

const VALIDATION_BUCKET: u64 = 1;
const TIEBREAK_EPOCHS: usize = 20;
const MARGINS: [i64; 7] = [0, 100, 1_000, 10_000, 100_000, 1_000_000, 10_000_000];

#[allow(clippy::cast_precision_loss)]
fn tiebreak(corpus: Corpus, models: &[Model], out: &Path) -> std::io::Result<()> {
    let Some(model) = models.iter().find(|m| v1_weights(m).is_some()) else {
        return Err(std::io::Error::other("tiebreak needs a --v1 primary model"));
    };
    let weights = v1_weights(model).expect("checked above");
    let train: Vec<&Line> = corpus.lines.iter().filter(|l| !l.test).collect();
    let fit: Vec<&Line> = train
        .iter()
        .copied()
        .filter(|l| l.bucket != VALIDATION_BUCKET)
        .collect();
    let validation: Vec<&Line> = train
        .iter()
        .copied()
        .filter(|l| l.bucket == VALIDATION_BUCKET)
        .collect();
    let test: Vec<&Line> = corpus.lines.iter().filter(|l| l.test).collect();
    eprintln!(
        "primary {}: fit {} lines, validation {} lines, test {} lines",
        model.name(),
        fit.len(),
        validation.len(),
        test.len()
    );

    let mut variants = Vec::new();
    for anchored in [false, true] {
        let label = if anchored {
            "local + anchor"
        } else {
            "local only"
        };
        let fit_examples = examples_of(&fit, &weights, anchored);
        let mut trials = Vec::new();
        for margin in MARGINS {
            let started = Instant::now();
            let trained = train_secondary(
                &fit_examples,
                &PerceptronConfig {
                    epochs: TIEBREAK_EPOCHS,
                    margin,
                },
            );
            let score = ladder(&validation, &weights, Some(&trained.weights), anchored)
                .learned
                .unwrap_or(0.0);
            eprintln!(
                "[{label}] margin {margin}: {} updates, {} epochs, validation agreement {:.2}% ({:.1}s)",
                trained.updates,
                trained.epochs,
                100.0 * score,
                started.elapsed().as_secs_f64()
            );
            trials.push(MarginTrial {
                margin,
                epochs: trained.epochs,
                updates: trained.updates,
                validation_agreement: score,
            });
        }
        let chosen_margin = trials
            .iter()
            .fold(None::<&MarginTrial>, |best, t| match best {
                Some(b) if b.validation_agreement >= t.validation_agreement => Some(b),
                _ => Some(t),
            })
            .map_or(0, |t| t.margin);
        let final_trained = train_secondary(
            &examples_of(&train, &weights, anchored),
            &PerceptronConfig {
                epochs: TIEBREAK_EPOCHS,
                margin: chosen_margin,
            },
        );
        variants.push(TiebreakVariant {
            features: label,
            trials,
            chosen_margin,
            final_updates: final_trained.updates,
            final_epochs: final_trained.epochs,
            weights: FEATURE_NAMES
                .iter()
                .copied()
                .zip(final_trained.weights)
                .collect(),
            train: ladder(&train, &weights, Some(&final_trained.weights), anchored),
            test: ladder(&test, &weights, Some(&final_trained.weights), anchored),
        });
    }

    println!("\nprimary {}", model.description_short());
    println!("\n| features | split | lines | human optimal | unique optimum | ln #optima p50 / p90 | floor | uniform over optima | production tie-break | learned tie-break | lines changed | ceiling | margin |");
    println!("|---|---|---|---|---|---|---|---|---|---|---|---|---|");
    for v in &variants {
        for (name, l) in [("train", &v.train), ("test (holdout)", &v.test)] {
            println!(
                "| {} | {name} | {} | {:.1}% | {:.1}% | {:.2} / {:.2} | {:.1}% | {:.1}% | {:.1}% | {} | {} | {:.1}% | {} |",
                v.features,
                l.lines,
                100.0 * l.human_optimal_lines as f64 / l.lines.max(1) as f64,
                100.0 * l.unique_optimum_lines as f64 / l.lines.max(1) as f64,
                l.ln_count.p50 as f64 / 1000.0,
                l.ln_count.p90 as f64 / 1000.0,
                100.0 * l.floor,
                100.0 * l.uniform,
                100.0 * l.production,
                l.learned.map_or("—".into(), |x| format!("{:.1}%", 100.0 * x)),
                l.learned_changed_lines.map_or("—".into(), |x| x.to_string()),
                100.0 * l.ceiling,
                v.chosen_margin
            );
        }
    }
    for v in &variants {
        println!(
            "\n[{}] learned secondary weights (averaged, unnormalized):",
            v.features
        );
        for (name, w) in &v.weights {
            println!("  {name:>20} {w}");
        }
    }
    let report = TiebreakReport {
        schema: "griff.constraint-lab-tiebreak",
        version: 2,
        primary: model.description_short(),
        epochs: TIEBREAK_EPOCHS,
        variants,
        corpus: corpus.facts,
    };
    write_json(&out.join("tiebreak.json"), &report)
}

// ── technique-aware fingering (oracle labels) ────────────────────────────────

#[derive(Debug, Clone, Default, Serialize)]
struct TapSlice {
    lines: usize,
    notes: u64,
    tapped_notes: u64,
    /// Lines whose human path lies in the model's optimum set.
    human_in_optimum_set: usize,
    /// `cost(human) − optimum` per line, in the model's cost units.
    human_excess: Quantiles,
    /// Total excess over total notes — comparable across line lengths.
    excess_per_note: f64,
    unique_optimum_lines: usize,
    /// Agreement of the production-order path (lowest candidate on ties).
    agree: f64,
    agree_tapped: f64,
    agree_fretted: f64,
    /// Most agreement reachable inside the optimum set.
    ceiling: f64,
}

struct TapLine {
    notes: u64,
    tapped: u64,
    in_set: bool,
    excess: i64,
    unique: bool,
    agree: u64,
    agree_tapped: u64,
    ceiling: u64,
}

fn tap_line(line: &Line, weights: &FingeringWeights, tap_shift: Option<i64>) -> TapLine {
    let tab = &line.tab;
    let (chain, human_cost) = match tap_shift {
        None => (chain_of(line, weights), v1_cost(&tab.human, weights)),
        Some(shift) => (
            tap_aware_chain(
                &tab.pitches,
                &tab.tuning,
                weights,
                shift,
                &tab.tapped,
                STANDARD_MAX_FRET,
            )
            .expect("tab lines are positionable and fully labelled"),
            tap_aware_cost(&tab.human, &tab.tapped, weights, shift).expect("labels cover the line"),
        ),
    };
    let set = optimum_set(&chain, Some(&tab.human));
    let range = set.agreement.expect("human positions per note");
    let path = chain
        .positions_of(&lexicographic_path(&chain, &[0; FEATURES], None))
        .expect("a path of the chain");
    let matched: Vec<bool> = path.iter().zip(&tab.human).map(|(a, h)| a == h).collect();
    TapLine {
        notes: tab.human.len() as u64,
        tapped: tab.tapped.iter().filter(|t| **t).count() as u64,
        in_set: human_cost == set.optimum,
        excess: human_cost - set.optimum,
        unique: !set.count.saturated && set.count.exact == 1,
        agree: matched.iter().filter(|m| **m).count() as u64,
        agree_tapped: matched
            .iter()
            .zip(&tab.tapped)
            .filter(|(m, t)| **m && **t)
            .count() as u64,
        ceiling: range.max as u64,
    }
}

#[allow(clippy::cast_precision_loss)]
fn tap_slice(lines: &[&Line], weights: &FingeringWeights, tap_shift: Option<i64>) -> TapSlice {
    let rows = par_map(lines, |line| tap_line(line, weights, tap_shift));
    let notes: u64 = rows.iter().map(|r| r.notes).sum();
    let tapped: u64 = rows.iter().map(|r| r.tapped).sum();
    let agree: u64 = rows.iter().map(|r| r.agree).sum();
    let agree_tapped: u64 = rows.iter().map(|r| r.agree_tapped).sum();
    let share = |x: u64, of: u64| x as f64 / of.max(1) as f64;
    TapSlice {
        lines: rows.len(),
        notes,
        tapped_notes: tapped,
        human_in_optimum_set: rows.iter().filter(|r| r.in_set).count(),
        human_excess: quantiles(rows.iter().map(|r| r.excess).collect()),
        excess_per_note: rows.iter().map(|r| r.excess as f64).sum::<f64>() / notes.max(1) as f64,
        unique_optimum_lines: rows.iter().filter(|r| r.unique).count(),
        agree: share(agree, notes),
        agree_tapped: share(agree_tapped, tapped),
        agree_fretted: share(agree - agree_tapped, notes - tapped),
        ceiling: share(rows.iter().map(|r| r.ceiling).sum(), notes),
    }
}

/// Line-length bins for the length-matched baseline.
fn length_bin(notes: u64) -> usize {
    match notes {
        0..=15 => 0,
        16..=31 => 1,
        32..=63 => 2,
        64..=127 => 3,
        _ => 4,
    }
}

/// Untapped lines reweighted to the tapped slice's length distribution — the
/// baseline a tap-aware model should be compared with, since exact line
/// optimality gets rarer as lines grow.
#[derive(Debug, Clone, Default, Serialize)]
struct LengthMatched {
    human_in_optimum_set: f64,
    excess_per_note: f64,
    agree: f64,
    ceiling: f64,
}

#[allow(clippy::cast_precision_loss)]
fn length_matched(
    tapped: &[&Line],
    untapped: &[&Line],
    weights: &FingeringWeights,
) -> LengthMatched {
    const BINS: usize = 5;
    let mut target_lines = [0_f64; BINS];
    let mut target_notes = [0_f64; BINS];
    for line in tapped {
        let n = line.tab.human.len() as u64;
        target_lines[length_bin(n)] += 1.0;
        target_notes[length_bin(n)] += n as f64;
    }
    let rows = par_map(untapped, |line| tap_line(line, weights, None));
    let mut lines = [0_f64; BINS];
    let mut in_set = [0_f64; BINS];
    let mut notes = [0_f64; BINS];
    let mut excess = [0_f64; BINS];
    let mut agree = [0_f64; BINS];
    let mut ceiling = [0_f64; BINS];
    for r in &rows {
        let b = length_bin(r.notes);
        lines[b] += 1.0;
        in_set[b] += f64::from(u8::from(r.in_set));
        notes[b] += r.notes as f64;
        excess[b] += r.excess as f64;
        agree[b] += r.agree as f64;
        ceiling[b] += r.ceiling as f64;
    }
    let (mut m, mut line_weight, mut note_weight) = (LengthMatched::default(), 0.0, 0.0);
    for b in 0..BINS {
        if lines[b] == 0.0 || target_lines[b] == 0.0 {
            continue;
        }
        m.human_in_optimum_set += target_lines[b] * in_set[b] / lines[b];
        line_weight += target_lines[b];
        m.excess_per_note += target_notes[b] * excess[b] / notes[b];
        m.agree += target_notes[b] * agree[b] / notes[b];
        m.ceiling += target_notes[b] * ceiling[b] / notes[b];
        note_weight += target_notes[b];
    }
    m.human_in_optimum_set /= line_weight.max(1.0);
    m.excess_per_note /= note_weight.max(1.0);
    m.agree /= note_weight.max(1.0);
    m.ceiling /= note_weight.max(1.0);
    m
}

#[derive(Serialize)]
struct TapTrial {
    weights: String,
    model: String,
    split: &'static str,
    slice: TapSlice,
}

#[derive(Serialize)]
struct TapsReport {
    schema: &'static str,
    version: u32,
    /// Untapped lines where the tap-aware and tap-blind objectives disagree on
    /// the optimum or the production-order path (must be 0).
    control_mismatches: usize,
    control_lines: usize,
    trials: Vec<TapTrial>,
    /// Per weight set: untapped lines reweighted to the tapped slice's lengths
    /// (whole corpus).
    length_matched: BTreeMap<&'static str, LengthMatched>,
    corpus: CorpusFacts,
}

#[allow(clippy::cast_precision_loss)]
fn taps(corpus: Corpus, out: &Path) -> std::io::Result<()> {
    let tap_lines: Vec<&Line> = corpus
        .lines
        .iter()
        .filter(|l| l.tab.tapped.iter().any(|t| *t))
        .collect();
    let untapped: Vec<&Line> = corpus
        .lines
        .iter()
        .filter(|l| l.tab.tapped.iter().all(|t| !*t))
        .collect();
    let weight_sets = [
        (
            "v1-fit",
            FingeringWeights {
                fret: 0,
                open_string: -3,
                position_shift: 1,
                string_change: 0,
            },
        ),
        ("v1", FingeringWeights::v1()),
    ];

    // Control: without taps the two objectives must be the same objective.
    let control_mismatches = par_map(&untapped, |line| {
        weight_sets.iter().any(|(_, w)| {
            let blind = chain_of(line, w);
            let aware = tap_aware_chain(
                &line.tab.pitches,
                &line.tab.tuning,
                w,
                w.position_shift,
                &line.tab.tapped,
                STANDARD_MAX_FRET,
            )
            .expect("positionable");
            let zero = [0; FEATURES];
            optimum_set(&blind, None).optimum != optimum_set(&aware, None).optimum
                || blind.positions_of(&lexicographic_path(&blind, &zero, None))
                    != aware.positions_of(&lexicographic_path(&aware, &zero, None))
        })
    })
    .into_iter()
    .filter(|m| *m)
    .count();

    let mut trials = Vec::new();
    for (name, w) in &weight_sets {
        let models: [(String, Option<i64>); 3] = [
            ("tap-blind".into(), None),
            (
                format!(
                    "tap-aware, tap_shift = position_shift ({})",
                    w.position_shift
                ),
                Some(w.position_shift),
            ),
            ("tap-aware, tap_shift = 0".into(), Some(0)),
        ];
        for (model, shift) in models {
            for (split, lines) in [
                ("whole corpus", tap_lines.clone()),
                (
                    "holdout songs",
                    tap_lines
                        .iter()
                        .copied()
                        .filter(|l| l.test)
                        .collect::<Vec<_>>(),
                ),
            ] {
                trials.push(TapTrial {
                    weights: (*name).to_string(),
                    model: model.clone(),
                    split,
                    slice: tap_slice(&lines, w, shift),
                });
            }
        }
    }

    println!(
        "\ncontrol: {control_mismatches} of {} untapped lines differ between tap-blind and tap-aware objectives",
        untapped.len()
    );
    let mut matched = BTreeMap::new();
    for (name, w) in &weight_sets {
        matched.insert(*name, length_matched(&tap_lines, &untapped, w));
    }
    println!("\n| weights | model | split | lines | notes (tapped) | human path in optimum set | human excess p50 / p90 | excess per note | unique optimum | agreement | on tapped notes | on fretted notes | ceiling |");
    println!("|---|---|---|---|---|---|---|---|---|---|---|---|---|");
    for t in &trials {
        let s = &t.slice;
        println!(
            "| {} | {} | {} | {} | {} ({}) | {:.1}% | {} / {} | {:.2} | {:.1}% | {:.1}% | {:.1}% | {:.1}% | {:.1}% |",
            t.weights,
            t.model,
            t.split,
            s.lines,
            s.notes,
            s.tapped_notes,
            100.0 * s.human_in_optimum_set as f64 / s.lines.max(1) as f64,
            s.human_excess.p50,
            s.human_excess.p90,
            s.excess_per_note,
            100.0 * s.unique_optimum_lines as f64 / s.lines.max(1) as f64,
            100.0 * s.agree,
            100.0 * s.agree_tapped,
            100.0 * s.agree_fretted,
            100.0 * s.ceiling
        );
    }
    println!("\nLength-matched untapped baseline (whole corpus, reweighted to the tapped slice's line lengths):");
    println!("\n| weights | human path in optimum set | excess per note | agreement | ceiling |");
    println!("|---|---|---|---|---|");
    for (name, m) in &matched {
        println!(
            "| {name} | {:.1}% | {:.2} | {:.1}% | {:.1}% |",
            100.0 * m.human_in_optimum_set,
            m.excess_per_note,
            100.0 * m.agree,
            100.0 * m.ceiling
        );
    }
    let report = TapsReport {
        schema: "griff.constraint-lab-taps",
        version: 1,
        control_mismatches,
        control_lines: untapped.len(),
        trials,
        length_matched: matched,
        corpus: corpus.facts,
    };
    write_json(&out.join("taps.json"), &report)
}

// ── legato census (oracle stage 2, phase 1) ──────────────────────────────────

/// Format family of a corpus file, by extension: GPIF (`.gp`, `.gpx`) or the
/// GP3–5 binaries.
fn format_family(name: &str) -> &'static str {
    let extension = Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    match extension.as_deref() {
        Some("gp" | "gpx") => "GP6/7",
        _ => "GP3–5",
    }
}

/// Edges, and how many keep both notes on one string.
#[derive(Debug, Clone, Copy, Default, Serialize)]
struct SameString {
    edges: u64,
    same_string: u64,
}

impl SameString {
    fn add(&mut self, same: bool) {
        self.edges += 1;
        self.same_string += u64::from(same);
    }
}

/// Edges, and how many land on an open string.
#[derive(Debug, Clone, Copy, Default, Serialize)]
struct OpenTarget {
    edges: u64,
    open_target: u64,
}

impl OpenTarget {
    fn add(&mut self, open: bool) {
        self.edges += 1;
        self.open_target += u64::from(open);
    }
}

/// Edge counts behind the stage-2 laws. Legato edges are observed (imported
/// span kinds); directions are derived from pitch.
#[derive(Debug, Clone, Default, Serialize)]
struct LegatoCensus {
    lines: u64,
    edges: u64,
    /// L1: all legato edges, then per imported kind; `plain` is the base rate.
    legato: SameString,
    hammer_on: SameString,
    pull_off: SameString,
    legato_span: SameString,
    plain: SameString,
    /// L2: legato edges per derived direction.
    ascending: SameString,
    descending: SameString,
    unison: SameString,
    /// L3: open-string targets of descending edges, legato against plain.
    descending_legato_open: OpenTarget,
    descending_plain_open: OpenTarget,
    /// L4: edges out of a tapped note; their legato edges per direction.
    out_of_tap: u64,
    out_of_tap_legato: SameString,
    out_of_tap_legato_ascending: u64,
    out_of_tap_legato_descending: u64,
    out_of_tap_legato_unison: u64,
    out_of_tap_legato_open_target: u64,
    /// L4: edges into a tapped note.
    into_tap: u64,
    into_tap_legato: SameString,
}

impl LegatoCensus {
    fn record(&mut self, tab: &TabLine) {
        self.lines += 1;
        for i in 1..tab.pitches.len() {
            let (before, here) = (tab.human[i - 1], tab.human[i]);
            let same = before.string == here.string;
            let open = here.fret == 0;
            let direction = derived_direction(&tab.pitches, i);
            self.edges += 1;
            let observed = tab
                .edges
                .iter()
                .any(|edge| edge.from == i - 1 && edge.to == i);
            if !observed {
                self.plain.add(same);
                if direction == Some(LegatoDirection::Descending) {
                    self.descending_plain_open.add(open);
                }
            }
            if tab.tapped[i - 1] {
                self.out_of_tap += 1;
            }
            if tab.tapped[i] {
                self.into_tap += 1;
            }
        }
        for edge in &tab.edges {
            let before = tab.human[edge.from];
            let here = tab.human[edge.to];
            let same = before.string == here.string;
            let open = here.fret == 0;
            let direction = direction_between(&tab.pitches, edge.from, edge.to);
            self.legato.add(same);
            match edge.kind {
                TechniqueKind::HammerOn => self.hammer_on.add(same),
                TechniqueKind::PullOff => self.pull_off.add(same),
                TechniqueKind::Legato => self.legato_span.add(same),
            }
            match direction {
                Some(LegatoDirection::Ascending) => self.ascending.add(same),
                Some(LegatoDirection::Descending) => {
                    self.descending.add(same);
                    self.descending_legato_open.add(open);
                }
                Some(LegatoDirection::Unison) => self.unison.add(same),
                None => {}
            }
            if tab.tapped[edge.from] {
                self.out_of_tap_legato.add(same);
                match direction {
                    Some(LegatoDirection::Ascending) => self.out_of_tap_legato_ascending += 1,
                    Some(LegatoDirection::Descending) => {
                        self.out_of_tap_legato_descending += 1;
                    }
                    Some(LegatoDirection::Unison) => self.out_of_tap_legato_unison += 1,
                    None => {}
                }
                self.out_of_tap_legato_open_target += u64::from(open);
            }
            if tab.tapped[edge.to] {
                self.into_tap_legato.add(same);
            }
        }
    }
}

#[derive(Serialize)]
struct CensusRow {
    split: &'static str,
    family: &'static str,
    population: &'static str,
    census: LegatoCensus,
}

#[derive(Serialize)]
struct CensusReport {
    schema: &'static str,
    version: u32,
    /// Legato origins in kept lines with no later same-string note in the
    /// imported voice.
    dangling_legato: u64,
    /// Resolved same-string targets that fall outside their origin's kept line.
    cross_line_legato: u64,
    projection_forensics: ProjectionForensicSummary,
    rows: Vec<CensusRow>,
    corpus: CorpusFacts,
}

#[derive(Debug, Clone, Serialize)]
struct ProjectionForensicSummary {
    cross_line: u64,
    within_line_edges: u64,
    non_adjacent_within_line: u64,
    top_longest_written: u64,
    targets_in_next_kept_line: u64,
    boundary_causes: BTreeMap<&'static str, u64>,
    first_boundary_causes: BTreeMap<&'static str, u64>,
    target_exclusion_causes: BTreeMap<&'static str, u64>,
    target_dispositions: BTreeMap<&'static str, u64>,
    boundaries_crossed: Distribution<u64>,
    cross_line_delta_quarters: Distribution<ExactRatio>,
}

#[derive(Debug, Clone, Serialize)]
struct ProjectionPoint {
    line_index: Option<usize>,
    voice_note_id: usize,
    onset: u32,
    duration: u32,
    pitch: u8,
    string: u8,
    fret: u8,
    tapped: bool,
}

#[derive(Debug, Clone, Serialize)]
struct CrossLineForensic {
    schema: &'static str,
    version: u32,
    id: String,
    file: String,
    song: String,
    family: &'static str,
    test: bool,
    track: usize,
    voice: u8,
    line_start_tick: u32,
    ticks_per_quarter: u32,
    kind: String,
    origin: ProjectionPoint,
    target: ProjectionPoint,
    span: TechniqueSpanStats,
    first_boundary: Option<LineBoundary>,
    boundary: CrossLineBoundary,
}

#[derive(Debug, Clone, Serialize)]
struct WithinLineForensic {
    schema: &'static str,
    version: u32,
    id: String,
    file: String,
    song: String,
    family: &'static str,
    test: bool,
    track: usize,
    voice: u8,
    line_start_tick: u32,
    ticks_per_quarter: u32,
    kind: String,
    derived_direction: &'static str,
    origin: ProjectionPoint,
    target: ProjectionPoint,
    span: TechniqueSpanStats,
}

#[derive(Serialize)]
struct SpanMetrics {
    note_distance: Distribution<u64>,
    intervening_onsets: Distribution<u64>,
    intervening_note_atoms: Distribution<u64>,
    delta_ticks: Distribution<u64>,
    delta_quarters: Distribution<ExactRatio>,
}

#[derive(Serialize)]
struct SpanGroup {
    dimension: &'static str,
    value: String,
    metrics: SpanMetrics,
}

#[derive(Serialize)]
struct SpanCensus {
    schema: &'static str,
    version: u32,
    quantile_method: &'static str,
    top_n: usize,
    overall: SpanMetrics,
    groups: Vec<SpanGroup>,
}

fn projection_point(tab: &TabLine, index: usize) -> ProjectionPoint {
    let position = tab.original_positions[index];
    ProjectionPoint {
        line_index: Some(index),
        voice_note_id: tab.note_ids[index],
        onset: tab.onsets[index],
        duration: tab.durations[index],
        pitch: tab.pitches[index].0,
        string: position.string,
        fret: position.fret,
        tapped: tab.tapped[index],
    }
}

fn direction_name(direction: Option<LegatoDirection>) -> &'static str {
    match direction {
        Some(LegatoDirection::Ascending) => "ascending",
        Some(LegatoDirection::Descending) => "descending",
        Some(LegatoDirection::Unison) => "unison",
        None => "unknown",
    }
}

const fn boundary_cause_name(cause: LineBoundaryCause) -> &'static str {
    match cause {
        LineBoundaryCause::RestCut => "rest_cut",
        LineBoundaryCause::ChordOnset => "chord_onset",
        LineBoundaryCause::Unpositioned => "unpositioned",
        LineBoundaryCause::BeyondMaxFret => "beyond_max_fret",
        LineBoundaryCause::PitchMismatch => "pitch_mismatch",
    }
}

const fn disposition_name(disposition: TargetDisposition) -> &'static str {
    match disposition {
        TargetDisposition::KeptLine => "kept_line",
        TargetDisposition::DroppedShortLine => "dropped_short_line",
        TargetDisposition::Excluded => "excluded",
    }
}

fn span_metrics<'a>(spans: impl Iterator<Item = &'a TechniqueSpanStats>) -> SpanMetrics {
    let spans: Vec<&TechniqueSpanStats> = spans.collect();
    let summarize = |select: fn(&TechniqueSpanStats) -> u64| {
        distribution(&spans.iter().map(|span| select(span)).collect::<Vec<_>>())
            .expect("span census group is non-empty")
    };
    SpanMetrics {
        note_distance: summarize(|span| span.note_distance as u64),
        intervening_onsets: summarize(|span| span.intervening_onsets as u64),
        intervening_note_atoms: summarize(|span| span.intervening_note_atoms as u64),
        delta_ticks: summarize(|span| u64::from(span.delta_ticks)),
        delta_quarters: distribution(
            &spans
                .iter()
                .map(|span| span.delta_quarters)
                .collect::<Vec<_>>(),
        )
        .expect("span census group is non-empty"),
    }
}

fn span_groups(records: &[WithinLineForensic]) -> Vec<SpanGroup> {
    let mut grouped: BTreeMap<(&'static str, String), Vec<&TechniqueSpanStats>> = BTreeMap::new();
    for record in records {
        for key in [
            ("technique_kind", record.kind.clone()),
            ("derived_direction", record.derived_direction.to_owned()),
            ("origin_tapped", record.origin.tapped.to_string()),
            ("target_tapped", record.target.tapped.to_string()),
            ("target_open", record.span.target_open.to_string()),
        ] {
            grouped.entry(key).or_default().push(&record.span);
        }
    }
    grouped
        .into_iter()
        .map(|((dimension, value), spans)| SpanGroup {
            dimension,
            value,
            metrics: span_metrics(spans.into_iter()),
        })
        .collect()
}

fn projection_forensics(corpus: &Corpus, out: &Path) -> std::io::Result<ProjectionForensicSummary> {
    const TOP_N: usize = 50;
    let mut cross_line = Vec::new();
    let mut within_line = Vec::new();
    for line in &corpus.lines {
        let tab = &line.tab;
        let file = corpus.names[line.file].clone();
        for &edge in &tab.edges {
            let Some(span) = within_line_span(tab, edge) else {
                continue;
            };
            within_line.push(WithinLineForensic {
                schema: "griff.constraint-lab-legato-within-line",
                version: 1,
                id: line.id.clone(),
                file: file.clone(),
                song: song_key(&file),
                family: format_family(&file),
                test: line.test,
                track: tab.track,
                voice: tab.voice,
                line_start_tick: tab.start_tick,
                ticks_per_quarter: tab.ticks_per_quarter,
                kind: format!("{:?}", edge.kind),
                derived_direction: direction_name(direction_between(
                    &tab.pitches,
                    edge.from,
                    edge.to,
                )),
                origin: projection_point(tab, edge.from),
                target: projection_point(tab, edge.to),
                span,
            });
        }
        for edge in &tab.cross_line_edges {
            let target = ProjectionPoint {
                line_index: None,
                voice_note_id: edge.target.note_id,
                onset: edge.target.onset,
                duration: edge.target.duration,
                pitch: edge.target.pitch.0,
                string: edge.target.original_position.string,
                fret: edge.target.original_position.fret,
                tapped: edge.target.tapped,
            };
            cross_line.push(CrossLineForensic {
                schema: "griff.constraint-lab-legato-cross-line",
                version: 1,
                id: line.id.clone(),
                file: file.clone(),
                song: song_key(&file),
                family: format_family(&file),
                test: line.test,
                track: tab.track,
                voice: tab.voice,
                line_start_tick: tab.start_tick,
                ticks_per_quarter: tab.ticks_per_quarter,
                kind: format!("{:?}", edge.kind),
                origin: projection_point(tab, edge.from),
                target,
                span: edge.span,
                first_boundary: edge.boundary.boundaries.first().cloned(),
                boundary: edge.boundary.clone(),
            });
        }
    }
    cross_line.sort_by(|a, b| {
        (
            &a.file,
            a.track,
            a.voice,
            a.origin.onset,
            a.origin.voice_note_id,
        )
            .cmp(&(
                &b.file,
                b.track,
                b.voice,
                b.origin.onset,
                b.origin.voice_note_id,
            ))
    });
    let top = top_n_longest(
        within_line.clone(),
        TOP_N,
        |record| record.span.delta_quarters,
        |record| {
            (
                record.file.clone(),
                record.track,
                record.voice,
                record.origin.onset,
                record.origin.voice_note_id,
            )
        },
    );
    let census = SpanCensus {
        schema: "griff.constraint-lab-legato-span-census",
        version: 1,
        quantile_method: "nearest_rank",
        top_n: TOP_N,
        overall: span_metrics(within_line.iter().map(|record| &record.span)),
        groups: span_groups(&within_line),
    };

    let cross_path = out.join("legato-cross-line.jsonl");
    let mut writer = BufWriter::new(fs::File::create(&cross_path)?);
    for record in &cross_line {
        serde_json::to_writer(&mut writer, record).map_err(std::io::Error::other)?;
        writer.write_all(b"\n")?;
    }
    writer.flush()?;
    eprintln!("wrote {}", cross_path.display());
    let longest_path = out.join("legato-longest-within-line.jsonl");
    let mut writer = BufWriter::new(fs::File::create(&longest_path)?);
    for record in &top {
        serde_json::to_writer(&mut writer, record).map_err(std::io::Error::other)?;
        writer.write_all(b"\n")?;
    }
    writer.flush()?;
    eprintln!("wrote {}", longest_path.display());
    write_json(&out.join("legato-span-census.json"), &census)?;

    let mut boundary_causes = BTreeMap::new();
    let mut first_boundary_causes = BTreeMap::new();
    let mut target_exclusion_causes = BTreeMap::new();
    let mut target_dispositions = BTreeMap::new();
    for record in &cross_line {
        for boundary in &record.boundary.boundaries {
            for &cause in &boundary.causes {
                *boundary_causes
                    .entry(boundary_cause_name(cause))
                    .or_default() += 1;
            }
        }
        if let Some(first) = record.boundary.boundaries.first() {
            for &cause in &first.causes {
                *first_boundary_causes
                    .entry(boundary_cause_name(cause))
                    .or_default() += 1;
            }
        }
        if record.boundary.target_disposition == TargetDisposition::Excluded {
            if let Some(target_boundary) = record.boundary.boundaries.iter().find(|boundary| {
                boundary.before_note_id <= record.target.voice_note_id
                    && record.target.voice_note_id < boundary.excluded_note_ids_end
            }) {
                for &cause in target_boundary
                    .causes
                    .iter()
                    .filter(|cause| **cause != LineBoundaryCause::RestCut)
                {
                    *target_exclusion_causes
                        .entry(boundary_cause_name(cause))
                        .or_default() += 1;
                }
            }
        }
        *target_dispositions
            .entry(disposition_name(record.boundary.target_disposition))
            .or_default() += 1;
    }
    let summary = ProjectionForensicSummary {
        cross_line: cross_line.len() as u64,
        within_line_edges: within_line.len() as u64,
        non_adjacent_within_line: within_line
            .iter()
            .filter(|record| record.span.note_distance > 1)
            .count() as u64,
        top_longest_written: top.len() as u64,
        targets_in_next_kept_line: cross_line
            .iter()
            .filter(|record| record.boundary.target_in_next_kept_line)
            .count() as u64,
        boundary_causes,
        first_boundary_causes,
        target_exclusion_causes,
        target_dispositions,
        boundaries_crossed: distribution(
            &cross_line
                .iter()
                .map(|record| record.boundary.line_boundaries_crossed as u64)
                .collect::<Vec<_>>(),
        )
        .expect("cross-line census is non-empty"),
        cross_line_delta_quarters: distribution(
            &cross_line
                .iter()
                .map(|record| record.span.delta_quarters)
                .collect::<Vec<_>>(),
        )
        .expect("cross-line census is non-empty"),
    };
    println!(
        "projection forensics: {} cross-line relations; {} within-line edges; {} targets in the next kept line; top {} longest written",
        summary.cross_line,
        summary.within_line_edges,
        summary.targets_in_next_kept_line,
        summary.top_longest_written,
    );
    Ok(summary)
}

#[allow(clippy::cast_precision_loss)]
fn pct(part: u64, whole: u64) -> String {
    if whole == 0 {
        "—".into()
    } else {
        format!("{:.1}%", 100.0 * part as f64 / whole as f64)
    }
}

fn same_cell(s: SameString) -> String {
    format!("{} ({})", pct(s.same_string, s.edges), s.edges)
}

#[allow(clippy::too_many_lines)]
fn legato_census(corpus: Corpus, out: &Path) -> std::io::Result<()> {
    const SPLITS: [&str; 2] = ["whole corpus", "holdout songs"];
    const FAMILIES: [&str; 3] = ["GP3–5", "GP6/7", "all"];
    const POPULATIONS: [&str; 2] = ["all lines", "tap slice"];
    let mut census: BTreeMap<(usize, usize, usize), LegatoCensus> = BTreeMap::new();
    for line in &corpus.lines {
        let family = format_family(&corpus.names[line.file]);
        let tapped = line.tab.tapped.iter().any(|t| *t);
        for (split, _) in SPLITS
            .iter()
            .enumerate()
            .filter(|(s, _)| *s == 0 || line.test)
        {
            for (fam, _) in FAMILIES
                .iter()
                .enumerate()
                .filter(|(_, f)| **f == family || **f == "all")
            {
                for (pop, _) in POPULATIONS
                    .iter()
                    .enumerate()
                    .filter(|(p, _)| *p == 0 || tapped)
                {
                    census
                        .entry((split, fam, pop))
                        .or_default()
                        .record(&line.tab);
                }
            }
        }
    }
    let rows: Vec<CensusRow> = census
        .into_iter()
        .map(|((split, fam, pop), census)| CensusRow {
            split: SPLITS[split],
            family: FAMILIES[fam],
            population: POPULATIONS[pop],
            census,
        })
        .collect();

    let dangling = corpus.facts.cut_stats.dangling_legato;
    let cross_line = corpus.facts.cut_stats.cross_line_legato;
    println!("\nunresolved legato origins (no later same-string voice note): {dangling}");
    println!("resolved legato targets outside their origin's kept line: {cross_line}");
    println!("\nL1/L2 — P(same string | edge): share (edges). Legato kinds are imported; directions are derived from pitch.\n");
    println!("| split | family | population | legato edges | HammerOn | PullOff | Legato | plain edges (base rate) | ascending legato | descending legato | unison legato |");
    println!("|---|---|---|---|---|---|---|---|---|---|---|");
    for r in &rows {
        let c = &r.census;
        println!(
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |",
            r.split,
            r.family,
            r.population,
            same_cell(c.legato),
            same_cell(c.hammer_on),
            same_cell(c.pull_off),
            same_cell(c.legato_span),
            same_cell(c.plain),
            same_cell(c.ascending),
            same_cell(c.descending),
            same_cell(c.unison)
        );
    }
    println!("\nL3 — P(target open | descending edge): share (edges).\n");
    println!("| split | family | population | descending legato edges | descending plain edges (base rate) |");
    println!("|---|---|---|---|---|");
    for r in &rows {
        let c = &r.census;
        println!(
            "| {} | {} | {} | {} ({}) | {} ({}) |",
            r.split,
            r.family,
            r.population,
            pct(
                c.descending_legato_open.open_target,
                c.descending_legato_open.edges
            ),
            c.descending_legato_open.edges,
            pct(
                c.descending_plain_open.open_target,
                c.descending_plain_open.edges
            ),
            c.descending_plain_open.edges
        );
    }
    println!("\nL4 — tap-adjacent edges.\n");
    println!("| split | family | population | edges out of a tapped note | legato share | legato: same string | legato: ascending / descending / unison | legato: open target | edges into a tapped note | legato share | legato: same string |");
    println!("|---|---|---|---|---|---|---|---|---|---|---|");
    for r in &rows {
        let c = &r.census;
        let legato_out = c.out_of_tap_legato.edges;
        println!(
            "| {} | {} | {} | {} | {} | {} | {} / {} / {} | {} | {} | {} | {} |",
            r.split,
            r.family,
            r.population,
            c.out_of_tap,
            pct(legato_out, c.out_of_tap),
            pct(c.out_of_tap_legato.same_string, legato_out),
            pct(c.out_of_tap_legato_ascending, legato_out),
            pct(c.out_of_tap_legato_descending, legato_out),
            pct(c.out_of_tap_legato_unison, legato_out),
            pct(c.out_of_tap_legato_open_target, legato_out),
            c.into_tap,
            pct(c.into_tap_legato.edges, c.into_tap),
            pct(c.into_tap_legato.same_string, c.into_tap_legato.edges)
        );
    }
    let projection_forensics = projection_forensics(&corpus, out)?;
    if projection_forensics.cross_line != cross_line {
        return Err(std::io::Error::other(format!(
            "projection forensic manifest contains {} cross-line relations, but cut stats report {cross_line}",
            projection_forensics.cross_line
        )));
    }
    let report = CensusReport {
        schema: "griff.constraint-lab-legato-census",
        version: 3,
        dangling_legato: dangling,
        cross_line_legato: cross_line,
        projection_forensics,
        rows,
        corpus: corpus.facts,
    };
    write_json(&out.join("legato-census.json"), &report)
}

// ── legato continuity ablation (oracle stage 2, phase 2) ─────────────────────

/// A stage of the registered ablation (protocol:
/// `docs/audit/2026-09-fingering-legato-continuity.md`). Each differs from its
/// predecessor by one term.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LegatoStage {
    /// Tap-blind `v1`.
    A,
    /// Tap-aware (stage 1).
    B,
    /// B + hard same-string continuity across legato edges.
    C1,
    /// B + soft continuity, `k · position_shift` per cross-string legato edge.
    C2(i64),
    /// C1 + the pull-off open-string waiver.
    D1,
    /// C2(3) + the pull-off open-string waiver.
    D2,
}

const LEGATO_STAGES: [LegatoStage; 8] = [
    LegatoStage::A,
    LegatoStage::B,
    LegatoStage::C1,
    LegatoStage::C2(1),
    LegatoStage::C2(3),
    LegatoStage::C2(10),
    LegatoStage::D1,
    LegatoStage::D2,
];

/// Index of C1 in `LEGATO_STAGES`, used only to select the registered hard
/// stage for the forensic violator manifest.
const LEGATO_C1_INDEX: usize = 2;

/// The registered steps for the leave-one-song-out check, as indices into
/// [`LEGATO_STAGES`]: A→B, B→C1, B→C2(3), C1→D1, C2(3)→D2.
const LEGATO_STEPS: [(usize, usize); 5] = [(0, 1), (1, 2), (1, 4), (2, 6), (4, 7)];

impl LegatoStage {
    fn name(self) -> String {
        match self {
            Self::A => "A tap-blind".into(),
            Self::B => "B tap-aware".into(),
            Self::C1 => "C1 hard continuity".into(),
            Self::C2(k) => format!("C2 soft continuity, k = {k}"),
            Self::D1 => "D1 = C1 + pull-off open waiver".into(),
            Self::D2 => "D2 = C2(3) + pull-off open waiver".into(),
        }
    }

    /// The stage's technique objective; `None` for the tap-blind `v1` chain.
    const fn objective(self, weights: FingeringWeights) -> Option<TechniqueObjective> {
        let base = TechniqueObjective::tap_aware(weights, weights.position_shift);
        let (continuity, pull_open_waiver) = match self {
            Self::A => return None,
            Self::B => (Continuity::Off, false),
            Self::C1 => (Continuity::Hard, false),
            Self::C2(k) => (Continuity::Soft { k }, false),
            Self::D1 => (Continuity::Hard, true),
            Self::D2 => (Continuity::Soft { k: 3 }, true),
        };
        Some(TechniqueObjective {
            continuity,
            pull_open_waiver,
            ..base
        })
    }

    const fn hard(self) -> bool {
        matches!(self, Self::C1 | Self::D1)
    }
}

/// One line under one stage.
#[derive(Debug, Clone, Copy, Serialize)]
struct StageRow {
    in_set: bool,
    unique: bool,
    agree: u64,
    agree_tapped: u64,
    ceiling: u64,
    /// `cost(human) − optimum`; `None` under a hard stage when the human path
    /// has more cross-string legato edges than the optimum.
    excess: Option<i64>,
    /// Cross-string legato edges of the human path and of the optimum.
    human_violations: u64,
    optimum_violations: u64,
}

struct LegatoLine {
    notes: u64,
    tapped: u64,
    rows: Vec<StageRow>,
    /// No legato edge, yet a legato stage moved B's optimum or path.
    legato_control_mismatch: bool,
    /// No tapped note, yet B's optimum or path differs from A's.
    tap_control_mismatch: bool,
}

fn cross_string_legato(path: &[FretboardPosition], edges: &[TechniqueEdge]) -> u64 {
    edges
        .iter()
        .filter(|edge| path[edge.from].string != path[edge.to].string)
        .count() as u64
}

/// Any human cross-string legato edges left after same-string projection.
/// This is a fail-closed regression manifest only; it does not feed any
/// objective and is expected to be empty on the research corpus.
fn cross_string_legato_forensics(tab: &TabLine) -> Vec<serde_json::Value> {
    tab.edges
        .iter()
        .filter_map(|edge| {
            let from = tab.human[edge.from];
            let target = tab.human[edge.to];
            if from.string == target.string {
                return None;
            }

            let direction = match direction_between(&tab.pitches, edge.from, edge.to) {
                Some(LegatoDirection::Ascending) => "ascending",
                Some(LegatoDirection::Descending) => "descending",
                Some(LegatoDirection::Unison) => "unison",
                None => "unknown",
            };

            Some(serde_json::json!({
                "kind": format!("{:?}", edge.kind),
                "derived_direction": direction,
                "from": {
                    "index": edge.from,
                    "pitch": tab.pitches[edge.from].0,
                    "string": from.string,
                    "fret": from.fret,
                    "tapped": tab.tapped[edge.from],
                },
                "target": {
                    "index": edge.to,
                    "notes_after_origin": edge.to - edge.from,
                    "pitch": tab.pitches[edge.to].0,
                    "string": target.string,
                    "fret": target.fret,
                    "tapped": tab.tapped[edge.to],
                },
            }))
        })
        .collect()
}

fn legato_line(line: &Line, weights: &FingeringWeights) -> LegatoLine {
    let tab = &line.tab;
    let zero = [0; FEATURES];
    let mut rows = Vec::with_capacity(LEGATO_STAGES.len());
    let mut optima = Vec::with_capacity(LEGATO_STAGES.len());
    let mut paths = Vec::with_capacity(LEGATO_STAGES.len());
    for stage in LEGATO_STAGES {
        let (chain, human_cost) = match stage.objective(*weights) {
            None => (chain_of(line, weights), v1_cost(&tab.human, weights)),
            Some(objective) => (
                technique_chain(
                    &tab.pitches,
                    &tab.tuning,
                    &tab.tapped,
                    &tab.edges,
                    &objective,
                    STANDARD_MAX_FRET,
                )
                .expect("tab lines are positionable and fully labelled"),
                technique_cost(
                    &tab.human,
                    &tab.pitches,
                    &tab.tapped,
                    &tab.edges,
                    &objective,
                )
                .expect("labels cover the line"),
            ),
        };
        let set = optimum_set(&chain, Some(&tab.human));
        let range = set.agreement.expect("human positions per note");
        let path = chain
            .positions_of(&lexicographic_path(&chain, &zero, None))
            .expect("a path of the chain");
        let matched: Vec<bool> = path.iter().zip(&tab.human).map(|(a, h)| a == h).collect();
        let human_violations = cross_string_legato(&tab.human, &tab.edges);
        let optimum_violations = cross_string_legato(&path, &tab.edges);
        rows.push(StageRow {
            in_set: human_cost == set.optimum,
            unique: !set.count.saturated && set.count.exact == 1,
            agree: matched.iter().filter(|m| **m).count() as u64,
            agree_tapped: matched
                .iter()
                .zip(&tab.tapped)
                .filter(|(m, t)| **m && **t)
                .count() as u64,
            ceiling: range.max as u64,
            excess: (!stage.hard() || human_violations == optimum_violations)
                .then(|| human_cost - set.optimum),
            human_violations,
            optimum_violations,
        });
        optima.push(set.optimum);
        paths.push(path);
    }
    let tapped = tab.tapped.iter().filter(|t| **t).count() as u64;
    let has_legato = !tab.edges.is_empty();
    LegatoLine {
        notes: tab.human.len() as u64,
        tapped,
        legato_control_mismatch: !has_legato
            && (2..LEGATO_STAGES.len()).any(|s| optima[s] != optima[1] || paths[s] != paths[1]),
        tap_control_mismatch: tapped == 0 && (optima[0] != optima[1] || paths[0] != paths[1]),
        rows,
    }
}

/// Sums of [`StageRow`]s over lines.
#[derive(Debug, Clone, Copy, Default, Serialize)]
struct StageAgg {
    lines: u64,
    notes: u64,
    tapped_notes: u64,
    in_set: u64,
    unique: u64,
    agree: u64,
    agree_tapped: u64,
    ceiling: u64,
    /// Lines (and their notes) with a defined excess.
    excess_lines: u64,
    excess_notes: u64,
    excess: i64,
    /// Lines whose human path has more cross-string legato edges than the optimum.
    human_violates_more: u64,
    /// Lines whose optimum keeps a cross-string legato edge.
    optimum_violates: u64,
}

impl StageAgg {
    fn add(&mut self, line: &LegatoLine, row: &StageRow) {
        self.lines += 1;
        self.notes += line.notes;
        self.tapped_notes += line.tapped;
        self.in_set += u64::from(row.in_set);
        self.unique += u64::from(row.unique);
        self.agree += row.agree;
        self.agree_tapped += row.agree_tapped;
        self.ceiling += row.ceiling;
        if let Some(excess) = row.excess {
            self.excess_lines += 1;
            self.excess_notes += line.notes;
            self.excess += excess;
        }
        self.human_violates_more += u64::from(row.human_violations > row.optimum_violations);
        self.optimum_violates += u64::from(row.optimum_violations > 0);
    }
}

/// An untapped pool reweighted to a subset's line lengths, under one stage.
#[derive(Debug, Clone, Copy, Default, Serialize)]
struct MatchedBaseline {
    pool_lines: usize,
    in_set: f64,
    excess_per_note: f64,
    agree: f64,
    ceiling: f64,
}

#[allow(clippy::cast_precision_loss)]
fn matched_baseline(target: &[&LegatoLine], pool: &[&LegatoLine], stage: usize) -> MatchedBaseline {
    const BINS: usize = 5;
    let mut target_lines = [0_f64; BINS];
    let mut target_notes = [0_f64; BINS];
    for line in target {
        target_lines[length_bin(line.notes)] += 1.0;
        target_notes[length_bin(line.notes)] += line.notes as f64;
    }
    let mut bins = [StageAgg::default(); BINS];
    for line in pool {
        bins[length_bin(line.notes)].add(line, &line.rows[stage]);
    }
    let mut m = MatchedBaseline {
        pool_lines: pool.len(),
        ..MatchedBaseline::default()
    };
    let (mut line_w, mut note_w, mut excess_w) = (0.0, 0.0, 0.0);
    for b in 0..BINS {
        let bin = &bins[b];
        if bin.lines == 0 || target_lines[b] == 0.0 {
            continue;
        }
        m.in_set += target_lines[b] * bin.in_set as f64 / bin.lines as f64;
        line_w += target_lines[b];
        m.agree += target_notes[b] * bin.agree as f64 / bin.notes as f64;
        m.ceiling += target_notes[b] * bin.ceiling as f64 / bin.notes as f64;
        note_w += target_notes[b];
        if bin.excess_notes > 0 {
            m.excess_per_note += target_notes[b] * bin.excess as f64 / bin.excess_notes as f64;
            excess_w += target_notes[b];
        }
    }
    m.in_set /= f64::max(line_w, 1.0);
    m.agree /= f64::max(note_w, 1.0);
    m.ceiling /= f64::max(note_w, 1.0);
    m.excess_per_note /= f64::max(excess_w, 1.0);
    m
}

#[derive(Serialize)]
struct StageReport {
    stage: String,
    slice: StageAgg,
    baseline_all_untapped: MatchedBaseline,
    baseline_same_format: MatchedBaseline,
}

#[derive(Serialize)]
struct SubsetReport {
    split: &'static str,
    family: &'static str,
    stages: Vec<StageReport>,
}

/// Leave-one-song-out for one registered step on one subset.
#[derive(Serialize)]
struct StepReport {
    family: &'static str,
    step: String,
    lines: usize,
    songs: usize,
    /// Lines entering the optimum set minus lines leaving it.
    net_line_gain: i64,
    delta_pp: f64,
    loso_min_pp: f64,
    loso_max_pp: f64,
    /// The largest single song's share of the net gain (when it is positive).
    largest_song_share: Option<f64>,
    /// Δ > 0 on the full subset and under every leave-one-song-out removal.
    corpus_evidence: bool,
}

#[derive(Serialize)]
struct WeightsReport {
    weights: &'static str,
    subsets: Vec<SubsetReport>,
    steps: Vec<StepReport>,
}

#[derive(Serialize)]
struct LegatoReport {
    schema: &'static str,
    version: u32,
    legato_control_mismatches: usize,
    lines_without_legato: usize,
    tap_control_mismatches: usize,
    untapped_lines: usize,
    reports: Vec<WeightsReport>,
    corpus: CorpusFacts,
}

#[allow(clippy::cast_precision_loss)]
fn share(part: u64, whole: u64) -> f64 {
    part as f64 / whole.max(1) as f64
}

#[allow(clippy::cast_precision_loss, clippy::cast_possible_wrap)]
fn step_report(
    family: &'static str,
    lines: &[(usize, &LegatoLine)],
    (from, to): (usize, usize),
) -> StepReport {
    let mut per_song: BTreeMap<usize, (i64, i64)> = BTreeMap::new();
    let (mut net, mut n) = (0_i64, 0_i64);
    for (song, line) in lines {
        let gain = i64::from(line.rows[to].in_set) - i64::from(line.rows[from].in_set);
        let entry = per_song.entry(*song).or_default();
        entry.0 += gain;
        entry.1 += 1;
        net += gain;
        n += 1;
    }
    let pp = |gain: i64, count: i64| 100.0 * gain as f64 / count.max(1) as f64;
    let loso: Vec<f64> = per_song
        .values()
        .filter(|(_, count)| *count < n)
        .map(|(gain, count)| pp(net - gain, n - count))
        .collect();
    let corpus_evidence = net > 0
        && per_song
            .values()
            .all(|(gain, count)| *count < n && net - gain > 0);
    StepReport {
        family,
        step: format!(
            "{} → {}",
            LEGATO_STAGES[from].name(),
            LEGATO_STAGES[to].name()
        ),
        lines: lines.len(),
        songs: per_song.len(),
        net_line_gain: net,
        delta_pp: pp(net, n),
        loso_min_pp: loso.iter().copied().fold(f64::INFINITY, f64::min),
        loso_max_pp: loso.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        largest_song_share: (net > 0).then(|| {
            per_song.values().map(|(gain, _)| *gain).max().unwrap_or(0) as f64 / net as f64
        }),
        corpus_evidence,
    }
}

#[allow(clippy::too_many_lines, clippy::cast_precision_loss)]
fn legato(corpus: Corpus, out: &Path) -> std::io::Result<()> {
    const SPLITS: [&str; 2] = ["whole corpus", "holdout songs"];
    const FAMILIES: [&str; 3] = ["all", "GP3–5", "GP6/7"];
    let weight_sets = [
        (
            "v1-fit",
            FingeringWeights {
                fret: 0,
                open_string: -3,
                position_shift: 1,
                string_change: 0,
            },
        ),
        ("v1", FingeringWeights::v1()),
    ];
    let families: Vec<&'static str> = corpus
        .lines
        .iter()
        .map(|l| format_family(&corpus.names[l.file]))
        .collect();
    let mut song_ids: BTreeMap<String, usize> = BTreeMap::new();
    let songs: Vec<usize> = corpus
        .lines
        .iter()
        .map(|l| {
            let next = song_ids.len();
            *song_ids
                .entry(song_key(&corpus.names[l.file]))
                .or_insert(next)
        })
        .collect();
    let refs: Vec<&Line> = corpus.lines.iter().collect();

    let mut reports = Vec::new();
    let mut controls = (0, 0);
    let mut dump = BufWriter::new(fs::File::create(out.join("legato-lines.jsonl"))?);
    let mut violators = BufWriter::new(fs::File::create(out.join("legato-violators.jsonl"))?);
    for (weights_name, weights) in &weight_sets {
        let started = Instant::now();
        let computed = par_map(&refs, |line| legato_line(line, weights));
        eprintln!(
            "{weights_name}: {} lines × {} stages in {:.1}s",
            computed.len(),
            LEGATO_STAGES.len(),
            started.elapsed().as_secs_f64()
        );
        controls.0 += computed
            .iter()
            .filter(|l| l.legato_control_mismatch)
            .count();
        controls.1 += computed.iter().filter(|l| l.tap_control_mismatch).count();
        for (line, c) in corpus.lines.iter().zip(&computed) {
            if c.tapped > 0 {
                let record = serde_json::json!({
                    "weights": weights_name,
                    "id": line.id,
                    "file": corpus.names.get(line.file),
                    "test": line.test,
                    "notes": c.notes,
                    "tapped": c.tapped,
                    "stages": c.rows,
                });
                serde_json::to_writer(&mut dump, &record).map_err(std::io::Error::other)?;
                dump.write_all(b"\n")?;

                // The hard-stage anomaly is independent of the second weight
                // set for the human path. Emit it once, under the primary
                // v1-fit pass, with the actual offending edges rather than
                // only aggregate counts.
                let c1 = c.rows[LEGATO_C1_INDEX];
                if *weights_name == "v1-fit" && c1.human_violations > c1.optimum_violations {
                    let file = corpus.names.get(line.file);
                    let forensic = serde_json::json!({
                        "schema": "griff.constraint-lab-legato-violator",
                        "version": 2,
                        "id": line.id,
                        "file": file,
                        "song": file.map(|name| song_key(name)),
                        "family": file.map(|name| format_family(name)),
                        "test": line.test,
                        "notes": c.notes,
                        "tapped": c.tapped,
                        "c1": c1,
                        "human_cross_string_legato": cross_string_legato_forensics(&line.tab),
                    });
                    serde_json::to_writer(&mut violators, &forensic)
                        .map_err(std::io::Error::other)?;
                    violators.write_all(b"\n")?;
                }
            }
        }

        let mut subsets = Vec::new();
        for (split_index, split) in SPLITS.iter().enumerate() {
            for family in FAMILIES {
                let in_subset = |i: usize| {
                    (split_index == 0 || corpus.lines[i].test)
                        && (family == "all" || families[i] == family)
                };
                let target: Vec<&LegatoLine> = (0..computed.len())
                    .filter(|&i| in_subset(i) && computed[i].tapped > 0)
                    .map(|i| &computed[i])
                    .collect();
                let pool_all: Vec<&LegatoLine> = (0..computed.len())
                    .filter(|&i| {
                        (split_index == 0 || corpus.lines[i].test) && computed[i].tapped == 0
                    })
                    .map(|i| &computed[i])
                    .collect();
                let pool_family: Vec<&LegatoLine> = (0..computed.len())
                    .filter(|&i| in_subset(i) && computed[i].tapped == 0)
                    .map(|i| &computed[i])
                    .collect();
                let stages = (0..LEGATO_STAGES.len())
                    .map(|s| {
                        let mut slice = StageAgg::default();
                        for line in &target {
                            slice.add(line, &line.rows[s]);
                        }
                        StageReport {
                            stage: LEGATO_STAGES[s].name(),
                            slice,
                            baseline_all_untapped: matched_baseline(&target, &pool_all, s),
                            baseline_same_format: matched_baseline(&target, &pool_family, s),
                        }
                    })
                    .collect();
                subsets.push(SubsetReport {
                    split,
                    family,
                    stages,
                });
            }
        }

        let mut steps = Vec::new();
        for family in FAMILIES {
            let lines: Vec<(usize, &LegatoLine)> = (0..computed.len())
                .filter(|&i| computed[i].tapped > 0 && (family == "all" || families[i] == family))
                .map(|i| (songs[i], &computed[i]))
                .collect();
            for step in LEGATO_STEPS {
                steps.push(step_report(family, &lines, step));
            }
        }
        reports.push(WeightsReport {
            weights: weights_name,
            subsets,
            steps,
        });
    }
    dump.flush()?;
    violators.flush()?;

    let without_legato = corpus
        .lines
        .iter()
        .filter(|l| l.tab.edges.is_empty())
        .count();
    let untapped = corpus
        .lines
        .iter()
        .filter(|l| l.tab.tapped.iter().all(|t| !*t))
        .count();
    println!(
        "\ncontrol: {} of {} (lines without legato edges × weight sets) differ between B and a legato stage; {} of {} (untapped lines × weight sets) between A and B",
        controls.0,
        without_legato * weight_sets.len(),
        controls.1,
        untapped * weight_sets.len()
    );
    let pc = |x: f64| format!("{:.1}%", 100.0 * x);
    for r in &reports {
        println!("\n### {} — slice\n", r.weights);
        println!("| split | family | stage | lines (tapped notes) | human path in optimum set | excess per note (lines) | agreement | on tapped notes | ceiling | unique optimum | human more cross-string legato / optimum keeps one |");
        println!("|---|---|---|---|---|---|---|---|---|---|---|");
        for sub in &r.subsets {
            for st in &sub.stages {
                let a = &st.slice;
                println!(
                    "| {} | {} | {} | {} ({}) | {} | {:.2} ({}) | {} | {} | {} | {} | {} / {} |",
                    sub.split,
                    sub.family,
                    st.stage,
                    a.lines,
                    a.tapped_notes,
                    pc(share(a.in_set, a.lines)),
                    a.excess as f64 / a.excess_notes.max(1) as f64,
                    a.excess_lines,
                    pc(share(a.agree, a.notes)),
                    pc(share(a.agree_tapped, a.tapped_notes)),
                    pc(share(a.ceiling, a.notes)),
                    pc(share(a.unique, a.lines)),
                    a.human_violates_more,
                    a.optimum_violates
                );
            }
        }
        println!("\n### {} — baselines (untapped lines, same stage objective, length-matched) and the exactness gap\n", r.weights);
        println!("| split | family | stage | slice in optimum set | all untapped: in set / excess per note / agreement / ceiling | same format: in set / excess per note | gap to all untapped (pt) | gap to same format (pt) |");
        println!("|---|---|---|---|---|---|---|---|");
        for sub in &r.subsets {
            for st in &sub.stages {
                let slice = share(st.slice.in_set, st.slice.lines);
                let (b, f) = (&st.baseline_all_untapped, &st.baseline_same_format);
                println!(
                    "| {} | {} | {} | {} | {} / {:.2} / {} / {} | {} / {:.2} | {:.1} | {:.1} |",
                    sub.split,
                    sub.family,
                    st.stage,
                    pc(slice),
                    pc(b.in_set),
                    b.excess_per_note,
                    pc(b.agree),
                    pc(b.ceiling),
                    pc(f.in_set),
                    f.excess_per_note,
                    100.0 * (b.in_set - slice),
                    100.0 * (f.in_set - slice)
                );
            }
        }
        println!(
            "\n### {} — leave one song out (whole corpus, tapped lines)\n",
            r.weights
        );
        println!("| family | step | lines | songs | net line gain | Δ exactness (pt) | leave-one-song-out min / max (pt) | largest song share of gain | corpus evidence |");
        println!("|---|---|---|---|---|---|---|---|---|");
        for st in &r.steps {
            println!(
                "| {} | {} | {} | {} | {} | {:+.1} | {:+.1} / {:+.1} | {} | {} |",
                st.family,
                st.step,
                st.lines,
                st.songs,
                st.net_line_gain,
                st.delta_pp,
                st.loso_min_pp,
                st.loso_max_pp,
                st.largest_song_share.map_or_else(|| "—".into(), &pc),
                if st.corpus_evidence { "yes" } else { "no" }
            );
        }
    }
    let report = LegatoReport {
        schema: "griff.constraint-lab-legato",
        version: 1,
        legato_control_mismatches: controls.0,
        lines_without_legato: without_legato * weight_sets.len(),
        tap_control_mismatches: controls.1,
        untapped_lines: untapped * weight_sets.len(),
        reports,
        corpus: corpus.facts,
    };
    write_json(&out.join("legato.json"), &report)
}

// ── repeat consistency ────────────────────────────────────────────────────────

/// Window of a repeated figure, in notes.
const REPEAT_WINDOW: usize = 6;

/// The two solver variants of a line with repeats: the model under a
/// deterministic string tie-break (`tie`), and the same plus the
/// repeat-consistency constraint (`tie-repeat`).
fn repeat_variants(model: &Model, line: &TabLine) -> Option<RepeatVariants> {
    let pairs = repeat_pairs(&line.pitches, REPEAT_WINDOW);
    if pairs.is_empty() {
        return None;
    }
    let (base, vpn) = model.problem(line)?;
    let (tie, scale) = with_string_tiebreak(&base, vpn).ok()?;
    let constrained = with_repeat_consistency(&tie, vpn, &pairs, REPEAT_WINDOW).ok()?;
    Some(RepeatVariants {
        tie,
        constrained,
        scale,
        vpn,
        pairs,
    })
}

struct RepeatVariants {
    tie: OptProblem,
    constrained: OptProblem,
    scale: i64,
    vpn: usize,
    pairs: Vec<(usize, usize)>,
}

fn repeat_export(corpus: &Corpus, models: &[Model], out: &Path) -> std::io::Result<()> {
    for model in models {
        let records = par_map(&corpus.lines, |line| {
            repeat_variants(model, &line.tab).map(|v| {
                let a = ProblemRecord::new(line.id.clone(), v.tie, Vec::new());
                let b = ProblemRecord::new(line.id.clone(), v.constrained, Vec::new());
                (
                    serde_json::to_string(&a).expect("problem records serialize"),
                    serde_json::to_string(&b).expect("problem records serialize"),
                )
            })
        });
        let tie_path = out.join(format!("{}.tie.problems.jsonl", model.name()));
        let rep_path = out.join(format!("{}.tie-repeat.problems.jsonl", model.name()));
        let mut tie_w = BufWriter::new(fs::File::create(&tie_path)?);
        let mut rep_w = BufWriter::new(fs::File::create(&rep_path)?);
        let mut written = 0;
        for (a, b) in records.into_iter().flatten() {
            tie_w.write_all(a.as_bytes())?;
            tie_w.write_all(b"\n")?;
            rep_w.write_all(b.as_bytes())?;
            rep_w.write_all(b"\n")?;
            written += 1;
        }
        eprintln!(
            "{}: {written} lines with repeats → {}, {}",
            model.name(),
            tie_path.display(),
            rep_path.display()
        );
    }
    Ok(())
}

fn read_records(path: &Path) -> std::io::Result<HashMap<String, SolveRecord>> {
    let mut records = HashMap::new();
    for line in BufReader::new(fs::File::open(path)?).lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let record: SolveRecord = serde_json::from_str(&line).map_err(std::io::Error::other)?;
        records.insert(record.id.clone(), record);
    }
    Ok(records)
}

#[derive(Debug, Clone, Default, Serialize)]
struct RepeatEval {
    lines: usize,
    notes: u64,
    pairs: u64,
    /// Both variants proven and verified.
    verified_lines: usize,
    refused_lines: usize,
    consistent_pairs_human: u64,
    consistent_pairs_dp: u64,
    consistent_pairs_tie: u64,
    consistent_pairs_repeat: u64,
    agree_dp: u64,
    agree_tie: u64,
    agree_repeat: u64,
    agree_rate_dp: f64,
    agree_rate_tie: f64,
    agree_rate_repeat: f64,
    /// Lines where the constraint raised the model cost, and by how much.
    lines_cost_raised: usize,
    cost_raise: Quantiles,
    /// Lines where the human fingering satisfies the constraint.
    human_consistent_lines: usize,
    solver_total_s_tie: f64,
    solver_total_s_repeat: f64,
}

#[allow(clippy::cast_precision_loss, clippy::too_many_lines)]
fn repeat_report(corpus: &Corpus, models: &[Model], out: &Path) -> std::io::Result<()> {
    let consistent = |positions: &[FretboardPosition], pairs: &[(usize, usize)]| {
        pairs
            .iter()
            .filter(|&&(i, j)| {
                positions.get(i..i + REPEAT_WINDOW) == positions.get(j..j + REPEAT_WINDOW)
            })
            .count() as u64
    };
    let mut evals = BTreeMap::new();
    for model in models {
        let tie = read_records(&out.join(format!("{}.tie.cpsat.jsonl", model.name())))?;
        let repeat = read_records(&out.join(format!("{}.tie-repeat.cpsat.jsonl", model.name())))?;
        struct One {
            notes: u64,
            pairs: u64,
            verified: Option<[u64; 7]>,
            raise: Option<i64>,
            human_consistent: bool,
            wall: (u64, u64),
        }
        let ones = par_map(&corpus.lines, |line| {
            let RepeatVariants {
                tie: tie_p,
                constrained: rep_p,
                scale,
                vpn,
                pairs,
            } = repeat_variants(model, &line.tab)?;
            // Lines the solver was not run on (e.g. a holdout-only run) are
            // outside the sample, not refusals.
            let (Some(a), Some(b)) = (tie.get(&line.id), repeat.get(&line.id)) else {
                return None;
            };
            let human = &line.tab.human;
            let human_consistent = consistent(human, &pairs) == pairs.len() as u64;
            let wall = (a.wall_us, b.wall_us);
            let (Verdict::Proven { optimum: oa }, Verdict::Proven { optimum: ob }) =
                (verify_record(&tie_p, a), verify_record(&rep_p, b))
            else {
                return Some(One {
                    notes: human.len() as u64,
                    pairs: pairs.len() as u64,
                    verified: None,
                    raise: None,
                    human_consistent,
                    wall,
                });
            };
            let pa = decode_positions(a.witness.as_deref()?, vpn)?;
            let pb = decode_positions(b.witness.as_deref()?, vpn)?;
            let dp = model.predict(&line.tab).positions;
            let agree = |p: &[FretboardPosition]| Agreement::of(human, p).agree;
            Some(One {
                notes: human.len() as u64,
                pairs: pairs.len() as u64,
                verified: Some([
                    consistent(human, &pairs),
                    consistent(&dp, &pairs),
                    consistent(&pa, &pairs),
                    consistent(&pb, &pairs),
                    agree(&dp),
                    agree(&pa),
                    agree(&pb),
                ]),
                raise: Some(ob.div_euclid(scale) - oa.div_euclid(scale)),
                human_consistent,
                wall,
            })
        });
        let mut e = RepeatEval::default();
        let mut raises = Vec::new();
        let mut verified_notes = 0_u64;
        for one in ones.into_iter().flatten() {
            e.lines += 1;
            e.notes += one.notes;
            e.pairs += one.pairs;
            e.human_consistent_lines += usize::from(one.human_consistent);
            e.solver_total_s_tie += one.wall.0 as f64 / 1e6;
            e.solver_total_s_repeat += one.wall.1 as f64 / 1e6;
            let Some(v) = one.verified else {
                e.refused_lines += 1;
                continue;
            };
            e.verified_lines += 1;
            verified_notes += one.notes;
            e.consistent_pairs_human += v[0];
            e.consistent_pairs_dp += v[1];
            e.consistent_pairs_tie += v[2];
            e.consistent_pairs_repeat += v[3];
            e.agree_dp += v[4];
            e.agree_tie += v[5];
            e.agree_repeat += v[6];
            if let Some(r) = one.raise {
                e.lines_cost_raised += usize::from(r > 0);
                raises.push(r);
            }
        }
        let rate = |x: u64| x as f64 / verified_notes.max(1) as f64;
        e.agree_rate_dp = rate(e.agree_dp);
        e.agree_rate_tie = rate(e.agree_tie);
        e.agree_rate_repeat = rate(e.agree_repeat);
        e.cost_raise = quantiles(raises);
        evals.insert(model.name().to_string(), e);
    }

    println!("\n| model | lines (verified / refused) | pairs | consistent pairs: human / DP / solver / solver+constraint | agreement: DP / solver / solver+constraint | cost raised (lines, p50 / p90 / max) | solver s (tie / +constraint) |");
    println!("|---|---|---|---|---|---|---|");
    for (name, e) in &evals {
        let pct = |x: u64| 100.0 * x as f64 / e.pairs.max(1) as f64;
        println!(
            "| {name} | {} ({} / {}) | {} | {:.1}% / {:.1}% / {:.1}% / {:.1}% | {:.1}% / {:.1}% / {:.1}% | {} ({} / {} / {}) | {:.0} / {:.0} |",
            e.lines,
            e.verified_lines,
            e.refused_lines,
            e.pairs,
            pct(e.consistent_pairs_human),
            pct(e.consistent_pairs_dp),
            pct(e.consistent_pairs_tie),
            pct(e.consistent_pairs_repeat),
            100.0 * e.agree_rate_dp,
            100.0 * e.agree_rate_tie,
            100.0 * e.agree_rate_repeat,
            e.lines_cost_raised,
            e.cost_raise.p50,
            e.cost_raise.p90,
            e.cost_raise.max,
            e.solver_total_s_tie,
            e.solver_total_s_repeat
        );
    }
    write_json(&out.join("repeat-report.json"), &evals)
}

fn write_json(path: &Path, value: &impl Serialize) -> std::io::Result<()> {
    let mut text = serde_json::to_string_pretty(value).map_err(std::io::Error::other)?;
    text.push('\n');
    fs::write(path, text)?;
    eprintln!("wrote {}", path.display());
    Ok(())
}

// ── legato into chord targets ────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Serialize)]
struct PositionRecord {
    string: u8,
    fret: u8,
}

impl From<FretboardPosition> for PositionRecord {
    fn from(position: FretboardPosition) -> Self {
        Self {
            string: position.string,
            fret: position.fret,
        }
    }
}

#[derive(Serialize)]
struct ChordAtomRecord {
    voice_note_id: usize,
    duration: u32,
    pitch: u8,
    imported_position: Option<PositionRecord>,
    tapped: bool,
    legato_target: bool,
}

#[derive(Serialize)]
struct ChordTargetRecord {
    schema: &'static str,
    version: u32,
    id: String,
    file: String,
    song: String,
    family: &'static str,
    test: bool,
    track: usize,
    voice: u8,
    origin_line_start_tick: u32,
    origin_note_id: usize,
    target_note_id: usize,
    chord_onset_tick: u32,
    ticks_per_quarter: u32,
    original_tuning: Vec<u8>,
    technique_kind: String,
    derived_direction: &'static str,
    boundary_class: &'static str,
    origin: ProjectionPoint,
    chord: Vec<ChordAtomRecord>,
}

#[derive(Serialize)]
struct ChordOptimumRecord {
    optimum: i64,
    optimum_count: u64,
    admissible_count: u64,
    chosen: Vec<PositionRecord>,
}

impl From<&ChordOptimum> for ChordOptimumRecord {
    fn from(optimum: &ChordOptimum) -> Self {
        Self {
            optimum: optimum.optimum,
            optimum_count: optimum.optimum_count,
            admissible_count: optimum.admissible_count,
            chosen: optimum.chosen.iter().copied().map(Into::into).collect(),
        }
    }
}

#[derive(Serialize)]
struct TargetStringRecord {
    string: u8,
    result: Option<ChordOptimumRecord>,
}

impl From<&TargetStringResult> for TargetStringRecord {
    fn from(condition: &TargetStringResult) -> Self {
        Self {
            string: condition.string,
            result: condition.result.as_ref().map(Into::into),
        }
    }
}

#[allow(clippy::struct_excessive_bools)] // independent exported measurements
#[derive(Serialize)]
struct HumanChordRecord {
    feasible: bool,
    satisfies_observed_constraint: bool,
    cost: Option<i64>,
    in_unconstrained_optimum: bool,
    in_observed_optimum: bool,
    excess_unconstrained: Option<i64>,
    excess_observed: Option<i64>,
}

impl From<&HumanChordAssessment> for HumanChordRecord {
    fn from(human: &HumanChordAssessment) -> Self {
        Self {
            feasible: human.feasible,
            satisfies_observed_constraint: human.satisfies_observed_constraint,
            cost: human.cost,
            in_unconstrained_optimum: human.in_unconstrained_optimum,
            in_observed_optimum: human.in_observed_optimum,
            excess_unconstrained: human.excess_unconstrained,
            excess_observed: human.excess_observed,
        }
    }
}

#[derive(Serialize)]
struct ChordFeasibilityRecord {
    schema: &'static str,
    version: u32,
    id: String,
    file: String,
    song: String,
    family: &'static str,
    track: usize,
    voice: u8,
    origin_note_id: usize,
    target_note_id: usize,
    chord_onset_tick: u32,
    boundary_class: &'static str,
    derived_direction: &'static str,
    origin_tapped: bool,
    chord_size: usize,
    observed_string: u8,
    outcome: &'static str,
    unconstrained: ChordOptimumRecord,
    observed: Option<ChordOptimumRecord>,
    delta_cost: Option<i64>,
    unconstrained_optimum_survivors: u64,
    unconstrained_optimum_set_reduction: u64,
    admissible_count_reduction: Option<u64>,
    target_strings: Vec<TargetStringRecord>,
    observed_dense_rank: Option<usize>,
    observed_cost_tie_size: Option<usize>,
    human: Option<HumanChordRecord>,
}

#[derive(Default, Serialize)]
struct OutcomeCounts {
    total: usize,
    free: usize,
    costly: usize,
    infeasible: usize,
}

impl OutcomeCounts {
    fn add(&mut self, outcome: &str) {
        self.total += 1;
        match outcome {
            "feasible_free" => self.free += 1,
            "feasible_costly" => self.costly += 1,
            "infeasible" => self.infeasible += 1,
            _ => {}
        }
    }
}

#[derive(Serialize)]
struct ChordStratum {
    dimension: &'static str,
    value: String,
    counts: OutcomeCounts,
}

#[derive(Default, Serialize)]
struct ChordControlSummary {
    observed_rank_one: usize,
    observed_unique_cheapest: usize,
    legal_target_string_conditions: usize,
    feasible_target_string_conditions: usize,
}

#[derive(Default, Serialize)]
struct ChordHumanSummary {
    complete: usize,
    feasible: usize,
    satisfies_observed_constraint: usize,
    in_unconstrained_optimum: usize,
    in_observed_optimum: usize,
    excess_unconstrained: Option<Distribution<i64>>,
    excess_observed: Option<Distribution<i64>>,
}

#[derive(Serialize)]
struct ChordSummary {
    schema: &'static str,
    version: u32,
    model: &'static str,
    max_fret: u8,
    population: usize,
    outcomes: OutcomeCounts,
    delta_cost: Option<Distribution<i64>>,
    unconstrained_optimum_survivors: Option<Distribution<u64>>,
    unconstrained_optimum_set_reduction: Option<Distribution<u64>>,
    admissible_count_reduction: Option<Distribution<u64>>,
    observed_dense_rank: Option<Distribution<u64>>,
    observed_cost_tie_size: Option<Distribution<u64>>,
    control: ChordControlSummary,
    human: ChordHumanSummary,
    strata: Vec<ChordStratum>,
    corpus: CorpusFacts,
}

fn chord_boundary_class(
    edge: &griff_constraint_lab::fingering::CrossLineTechniqueEdge,
) -> &'static str {
    let causes = edge
        .boundary
        .boundaries
        .iter()
        .flat_map(|boundary| boundary.causes.iter());
    let mut rest = false;
    let mut chord = false;
    for cause in causes {
        rest |= *cause == LineBoundaryCause::RestCut;
        chord |= *cause == LineBoundaryCause::ChordOnset;
    }
    match (rest, chord) {
        (true, true) => "rest_plus_chord",
        (false, true) => "chord_only",
        (true, false) => "rest_only",
        (false, false) => "other",
    }
}

fn imported_chord_atom(atom: ImportedChordAtom) -> ChordAtom {
    ChordAtom {
        note_id: atom.note_id,
        pitch: atom.pitch,
        imported_position: atom.original_position,
        tapped: atom.tapped,
    }
}

fn chord_outcome(analysis: &ChordAnalysis) -> &'static str {
    match &analysis.observed {
        None => "infeasible",
        Some(observed) if observed.optimum == analysis.unconstrained.optimum => "feasible_free",
        Some(_) => "feasible_costly",
    }
}

fn legato_chords(corpus: Corpus, out: &Path) -> std::io::Result<()> {
    let policy = ChordCostPolicy::v1_unary();
    let targets_path = out.join("legato-chord-targets.jsonl");
    let feasibility_path = out.join("legato-chord-feasibility.jsonl");
    let mut targets_writer = BufWriter::new(fs::File::create(&targets_path)?);
    let mut feasibility_writer = BufWriter::new(fs::File::create(&feasibility_path)?);
    let mut records = Vec::new();

    for line in &corpus.lines {
        for edge in &line.tab.cross_line_edges {
            if edge.boundary.target_disposition != TargetDisposition::Excluded
                || edge.target_chord.len() < 2
                || !edge
                    .target_chord
                    .iter()
                    .any(|atom| atom.note_id == edge.target.note_id)
            {
                continue;
            }
            let file = corpus.names[line.file].clone();
            let boundary_class = chord_boundary_class(edge);
            let direction = match edge.span.pitch_interval_semitones.cmp(&0) {
                std::cmp::Ordering::Greater => "ascending",
                std::cmp::Ordering::Less => "descending",
                std::cmp::Ordering::Equal => "unison",
            };
            let origin = projection_point(&line.tab, edge.from);
            let chord: Vec<ChordAtomRecord> = edge
                .target_chord
                .iter()
                .map(|atom| ChordAtomRecord {
                    voice_note_id: atom.note_id,
                    duration: atom.duration,
                    pitch: atom.pitch.0,
                    imported_position: atom.original_position.map(Into::into),
                    tapped: atom.tapped,
                    legato_target: atom.note_id == edge.target.note_id,
                })
                .collect();
            let target_record = ChordTargetRecord {
                schema: "griff.constraint-lab-legato-chord-target",
                version: 1,
                id: line.id.clone(),
                file: file.clone(),
                song: song_key(&file),
                family: format_family(&file),
                test: line.test,
                track: line.tab.track,
                voice: line.tab.voice,
                origin_line_start_tick: line.tab.start_tick,
                origin_note_id: edge.origin_note_id,
                target_note_id: edge.target.note_id,
                chord_onset_tick: edge.target.onset,
                ticks_per_quarter: line.tab.ticks_per_quarter,
                original_tuning: line
                    .tab
                    .original_tuning
                    .open_strings()
                    .iter()
                    .map(|pitch| pitch.0)
                    .collect(),
                technique_kind: format!("{:?}", edge.kind),
                derived_direction: direction,
                boundary_class,
                origin,
                chord,
            };
            serde_json::to_writer(&mut targets_writer, &target_record)
                .map_err(std::io::Error::other)?;
            targets_writer.write_all(b"\n")?;

            let atoms: Vec<ChordAtom> = edge
                .target_chord
                .iter()
                .copied()
                .map(imported_chord_atom)
                .collect();
            let analysis = analyze_chord(
                &atoms,
                &line.tab.original_tuning,
                STANDARD_MAX_FRET,
                TargetStringConstraint {
                    atom_id: edge.target.note_id,
                    string: edge.target.original_position.string,
                },
                &policy,
            )
            .map_err(std::io::Error::other)?;
            let outcome = chord_outcome(&analysis);
            let delta_cost = analysis
                .observed
                .as_ref()
                .map(|observed| observed.optimum - analysis.unconstrained.optimum);
            // C's optimum count is not comparable with U's when C has a higher
            // optimum. Count only members of U's original optimum set that
            // survive the observed-string restriction.
            let unconstrained_optimum_survivors = analysis
                .observed
                .as_ref()
                .filter(|observed| observed.optimum == analysis.unconstrained.optimum)
                .map_or(0, |observed| observed.optimum_count);
            let unconstrained_optimum_set_reduction = analysis
                .unconstrained
                .optimum_count
                .saturating_sub(unconstrained_optimum_survivors);
            let admissible_count_reduction = analysis.observed.as_ref().map(|observed| {
                analysis
                    .unconstrained
                    .admissible_count
                    .saturating_sub(observed.admissible_count)
            });
            records.push(ChordFeasibilityRecord {
                schema: "griff.constraint-lab-legato-chord-feasibility",
                version: 1,
                id: line.id.clone(),
                file: file.clone(),
                song: song_key(&file),
                family: format_family(&file),
                track: line.tab.track,
                voice: line.tab.voice,
                origin_note_id: edge.origin_note_id,
                target_note_id: edge.target.note_id,
                chord_onset_tick: edge.target.onset,
                boundary_class,
                derived_direction: direction,
                origin_tapped: line.tab.tapped[edge.from],
                chord_size: atoms.len(),
                observed_string: edge.target.original_position.string,
                outcome,
                unconstrained: (&analysis.unconstrained).into(),
                observed: analysis.observed.as_ref().map(Into::into),
                delta_cost,
                unconstrained_optimum_survivors,
                unconstrained_optimum_set_reduction,
                admissible_count_reduction,
                target_strings: analysis.target_strings.iter().map(Into::into).collect(),
                observed_dense_rank: analysis.observed_dense_rank,
                observed_cost_tie_size: analysis.observed_best_tie_size,
                human: analysis.human.as_ref().map(Into::into),
            });
        }
    }
    targets_writer.flush()?;
    records.sort_by(|a, b| {
        (
            &a.file,
            a.track,
            a.voice,
            a.chord_onset_tick,
            a.target_note_id,
        )
            .cmp(&(
                &b.file,
                b.track,
                b.voice,
                b.chord_onset_tick,
                b.target_note_id,
            ))
    });
    for record in &records {
        serde_json::to_writer(&mut feasibility_writer, record).map_err(std::io::Error::other)?;
        feasibility_writer.write_all(b"\n")?;
    }
    feasibility_writer.flush()?;
    eprintln!("wrote {}", targets_path.display());
    eprintln!("wrote {}", feasibility_path.display());

    let mut outcomes = OutcomeCounts::default();
    let mut control = ChordControlSummary::default();
    let mut human = ChordHumanSummary::default();
    let mut delta_costs = Vec::new();
    let mut optimum_survivors = Vec::new();
    let mut optimum_set_reductions = Vec::new();
    let mut admissible_count_reductions = Vec::new();
    let mut ranks = Vec::new();
    let mut tie_sizes = Vec::new();
    let mut human_excess_u = Vec::new();
    let mut human_excess_c = Vec::new();
    let mut strata: BTreeMap<(&'static str, String), OutcomeCounts> = BTreeMap::new();
    for record in &records {
        outcomes.add(record.outcome);
        if let Some(delta) = record.delta_cost {
            delta_costs.push(delta);
        }
        optimum_survivors.push(record.unconstrained_optimum_survivors);
        optimum_set_reductions.push(record.unconstrained_optimum_set_reduction);
        if let Some(reduction) = record.admissible_count_reduction {
            admissible_count_reductions.push(reduction);
        }
        if let Some(rank) = record.observed_dense_rank {
            ranks.push(rank as u64);
            control.observed_rank_one += usize::from(rank == 1);
        }
        if let Some(tie_size) = record.observed_cost_tie_size {
            tie_sizes.push(tie_size as u64);
        }
        control.observed_unique_cheapest += usize::from(
            record.observed_dense_rank == Some(1) && record.observed_cost_tie_size == Some(1),
        );
        control.legal_target_string_conditions += record.target_strings.len();
        control.feasible_target_string_conditions += record
            .target_strings
            .iter()
            .filter(|condition| condition.result.is_some())
            .count();
        if let Some(observed) = &record.human {
            human.complete += 1;
            human.feasible += usize::from(observed.feasible);
            human.satisfies_observed_constraint +=
                usize::from(observed.satisfies_observed_constraint);
            human.in_unconstrained_optimum += usize::from(observed.in_unconstrained_optimum);
            human.in_observed_optimum += usize::from(observed.in_observed_optimum);
            if let Some(excess) = observed.excess_unconstrained {
                human_excess_u.push(excess);
            }
            if let Some(excess) = observed.excess_observed {
                human_excess_c.push(excess);
            }
        }
        for key in [
            ("boundary", record.boundary_class.to_owned()),
            ("direction", record.derived_direction.to_owned()),
            ("origin_tapped", record.origin_tapped.to_string()),
            ("chord_size", record.chord_size.to_string()),
            ("family", record.family.to_owned()),
        ] {
            strata.entry(key).or_default().add(record.outcome);
        }
    }
    human.excess_unconstrained = distribution(&human_excess_u);
    human.excess_observed = distribution(&human_excess_c);
    let summary = ChordSummary {
        schema: "griff.constraint-lab-legato-chord-summary",
        version: 1,
        model: "distinct_strings_plus_v1_unary",
        max_fret: STANDARD_MAX_FRET,
        population: records.len(),
        outcomes,
        delta_cost: distribution(&delta_costs),
        unconstrained_optimum_survivors: distribution(&optimum_survivors),
        unconstrained_optimum_set_reduction: distribution(&optimum_set_reductions),
        admissible_count_reduction: distribution(&admissible_count_reductions),
        observed_dense_rank: distribution(&ranks),
        observed_cost_tie_size: distribution(&tie_sizes),
        control,
        human,
        strata: strata
            .into_iter()
            .map(|((dimension, value), counts)| ChordStratum {
                dimension,
                value,
                counts,
            })
            .collect(),
        corpus: corpus.facts,
    };
    if summary.population != 38 {
        return Err(std::io::Error::other(format!(
            "expected the preregistered 38 chord-target relations, got {}",
            summary.population
        )));
    }
    write_json(&out.join("legato-chord-summary.json"), &summary)?;
    println!(
        "legato chord targets: {} total; {} free, {} costly, {} infeasible",
        summary.population,
        summary.outcomes.free,
        summary.outcomes.costly,
        summary.outcomes.infeasible
    );
    Ok(())
}

// ── chord-target preceding context ───────────────────────────────────────────

#[derive(Serialize)]
struct ContextMinimumRecord {
    optimum: i64,
    optimum_count: u64,
    chosen: Vec<PositionRecord>,
}

impl From<&ExactMinimum> for ContextMinimumRecord {
    fn from(metric: &ExactMinimum) -> Self {
        Self {
            optimum: metric.optimum,
            optimum_count: metric.optimum_count,
            chosen: metric.chosen.iter().copied().map(Into::into).collect(),
        }
    }
}

#[derive(Serialize)]
struct ContextLexRecord {
    first: i64,
    second: i64,
    optimum_count: u64,
    chosen: Vec<PositionRecord>,
}

impl From<&ExactLexMinimum> for ContextLexRecord {
    fn from(metric: &ExactLexMinimum) -> Self {
        Self {
            first: metric.first,
            second: metric.second,
            optimum_count: metric.optimum_count,
            chosen: metric.chosen.iter().copied().map(Into::into).collect(),
        }
    }
}

#[derive(Serialize)]
struct ContextStringRecord {
    string: u8,
    base: Option<ContextMinimumRecord>,
    origin: Option<ContextMinimumRecord>,
    anchor: Option<ContextMinimumRecord>,
    origin_anchor: Option<ContextLexRecord>,
    anchor_origin: Option<ContextLexRecord>,
    pareto_member: Option<bool>,
    pareto_count: Option<u64>,
    pareto_chosen: Option<Vec<PositionRecord>>,
}

impl From<&ContextStringResult> for ContextStringRecord {
    fn from(condition: &ContextStringResult) -> Self {
        Self {
            string: condition.string,
            base: condition.base.as_ref().map(Into::into),
            origin: condition.origin.as_ref().map(Into::into),
            anchor: condition.anchor.as_ref().map(Into::into),
            origin_anchor: condition.origin_anchor.as_ref().map(Into::into),
            anchor_origin: condition.anchor_origin.as_ref().map(Into::into),
            pareto_member: condition.pareto_member,
            pareto_count: condition.pareto_count,
            pareto_chosen: condition
                .pareto_chosen
                .as_ref()
                .map(|positions| positions.iter().copied().map(Into::into).collect()),
        }
    }
}

#[derive(Serialize)]
struct RankChangeRecord {
    rank_delta: Option<i64>,
    classification: &'static str,
}

impl From<RankChange> for RankChangeRecord {
    fn from(change: RankChange) -> Self {
        let classification = match change.classification {
            ContextClassification::Improved => "improved",
            ContextClassification::Unchanged => "unchanged",
            ContextClassification::Worsened => "worsened",
            ContextClassification::Unavailable => "unavailable",
        };
        Self {
            rank_delta: change.rank_delta,
            classification,
        }
    }
}

#[derive(Serialize)]
struct ContextHumanRecord {
    positions: Vec<PositionRecord>,
    base: Option<i64>,
    origin: Option<i64>,
    anchor: Option<i64>,
    origin_excess: Option<i64>,
    anchor_excess: Option<i64>,
    pareto_member: Option<bool>,
}

impl From<&HumanContextAssessment> for ContextHumanRecord {
    fn from(human: &HumanContextAssessment) -> Self {
        Self {
            positions: human.positions.iter().copied().map(Into::into).collect(),
            base: human.base,
            origin: human.origin,
            anchor: human.anchor,
            origin_excess: human.origin_excess,
            anchor_excess: human.anchor_excess,
            pareto_member: human.pareto_member,
        }
    }
}

#[derive(Serialize)]
struct ContextCaseRecord {
    schema: &'static str,
    version: u32,
    id: String,
    file: String,
    song: String,
    family: &'static str,
    test: bool,
    track: usize,
    voice: u8,
    origin_line_start_tick: u32,
    origin_note_id: usize,
    origin_onset_tick: u32,
    origin_pitch: u8,
    origin_position: PositionRecord,
    origin_tapped: bool,
    anchor_fret: Option<u8>,
    target_note_id: usize,
    target_pitch: u8,
    chord_onset_tick: u32,
    observed_string: u8,
    chord: Vec<ChordAtomRecord>,
    baseline_outcome: &'static str,
    baseline_delta_cost: Option<i64>,
    base_rank: Option<usize>,
    origin_rank: Option<usize>,
    anchor_rank: Option<usize>,
    origin_anchor_rank: Option<usize>,
    anchor_origin_rank: Option<usize>,
    pareto_member: Option<bool>,
    origin_change: RankChangeRecord,
    anchor_change: RankChangeRecord,
    origin_anchor_change: RankChangeRecord,
    anchor_origin_change: RankChangeRecord,
    target_strings: Vec<ContextStringRecord>,
    human: Option<ContextHumanRecord>,
}

#[derive(Clone, Copy)]
enum ContextView {
    Origin,
    Anchor,
    OriginAnchor,
    AnchorOrigin,
}

impl ContextView {
    const ALL: [Self; 4] = [
        Self::Origin,
        Self::Anchor,
        Self::OriginAnchor,
        Self::AnchorOrigin,
    ];

    const fn name(self) -> &'static str {
        match self {
            Self::Origin => "O",
            Self::Anchor => "A",
            Self::OriginAnchor => "O_to_A",
            Self::AnchorOrigin => "A_to_O",
        }
    }

    fn rank(self, record: &ContextCaseRecord) -> Option<usize> {
        match self {
            Self::Origin => record.origin_rank,
            Self::Anchor => record.anchor_rank,
            Self::OriginAnchor => record.origin_anchor_rank,
            Self::AnchorOrigin => record.anchor_origin_rank,
        }
    }

    fn change(self, record: &ContextCaseRecord) -> &RankChangeRecord {
        match self {
            Self::Origin => &record.origin_change,
            Self::Anchor => &record.anchor_change,
            Self::OriginAnchor => &record.origin_anchor_change,
            Self::AnchorOrigin => &record.anchor_origin_change,
        }
    }
}

#[derive(Default, Serialize)]
struct ChangeCounts {
    improved: usize,
    unchanged: usize,
    worsened: usize,
    unavailable: usize,
}

impl ChangeCounts {
    fn add(&mut self, classification: &str) {
        match classification {
            "improved" => self.improved += 1,
            "unchanged" => self.unchanged += 1,
            "worsened" => self.worsened += 1,
            _ => self.unavailable += 1,
        }
    }
}

#[derive(Serialize)]
struct ViewAggregate {
    view: &'static str,
    available: usize,
    rank_one: usize,
    rank_counts: BTreeMap<usize, usize>,
    changes: ChangeCounts,
    rank_delta: Option<Distribution<i64>>,
}

fn aggregate_view<'a>(
    records: impl Iterator<Item = &'a ContextCaseRecord>,
    view: ContextView,
) -> ViewAggregate {
    let mut available = 0;
    let mut rank_one = 0;
    let mut rank_counts = BTreeMap::new();
    let mut changes = ChangeCounts::default();
    let mut deltas = Vec::new();
    for record in records {
        if let Some(rank) = view.rank(record) {
            available += 1;
            rank_one += usize::from(rank == 1);
            *rank_counts.entry(rank).or_insert(0) += 1;
        }
        let change = view.change(record);
        changes.add(change.classification);
        if let Some(delta) = change.rank_delta {
            deltas.push(delta);
        }
    }
    ViewAggregate {
        view: view.name(),
        available,
        rank_one,
        rank_counts,
        changes,
        rank_delta: distribution(&deltas),
    }
}

#[derive(Serialize)]
struct SongViewSummary {
    view: &'static str,
    base_rank_one: usize,
    context: ViewAggregate,
}

#[derive(Serialize)]
struct SongContextSummary {
    song: String,
    cases: usize,
    views: Vec<SongViewSummary>,
}

#[derive(Serialize)]
struct LooContextSummary {
    omitted_song: String,
    remaining_cases: usize,
    view: &'static str,
    base_rank_one: usize,
    context_rank_one: usize,
    rank_one_gain: i64,
    improved: usize,
    unchanged: usize,
    worsened: usize,
    median_rank_delta: Option<i64>,
}

#[derive(Serialize)]
struct ContextHumanSummary {
    complete: usize,
    pareto_member: usize,
    origin_excess: Option<Distribution<i64>>,
    anchor_excess: Option<Distribution<i64>>,
}

#[derive(Serialize)]
struct ContextSummary {
    schema: &'static str,
    version: u32,
    population: usize,
    baseline_legal_conditions: usize,
    baseline_feasible_conditions: usize,
    baseline_rank_counts: BTreeMap<usize, usize>,
    baseline_outcomes: OutcomeCounts,
    anchors_present: usize,
    observed_pareto_members: usize,
    views: Vec<ViewAggregate>,
    costly_views: Vec<ViewAggregate>,
    human: ContextHumanSummary,
    songs: usize,
    corpus: CorpusFacts,
}

fn context_case(
    corpus: &Corpus,
    line: &Line,
    edge: &griff_constraint_lab::fingering::CrossLineTechniqueEdge,
    policy: &ChordCostPolicy,
) -> Result<ContextCaseRecord, std::io::Error> {
    let file = corpus.names[line.file].clone();
    let atoms: Vec<ChordAtom> = edge
        .target_chord
        .iter()
        .copied()
        .map(imported_chord_atom)
        .collect();
    let origin_position = line.tab.original_positions[edge.from];
    let analysis: ChordContextAnalysis = analyze_chord_context(
        &atoms,
        &line.tab.original_tuning,
        STANDARD_MAX_FRET,
        edge.target.note_id,
        edge.target.original_position.string,
        ChordContext {
            origin_fret: origin_position.fret,
            anchor_fret: edge.target_anchor_fret,
        },
        policy,
    )
    .map_err(std::io::Error::other)?;
    let outcome = chord_outcome(&analysis.baseline);
    let delta_cost = analysis
        .baseline
        .observed
        .as_ref()
        .map(|observed| observed.optimum - analysis.baseline.unconstrained.optimum);
    for (baseline, context) in analysis
        .baseline
        .target_strings
        .iter()
        .zip(&analysis.target_strings)
    {
        let context_base = context
            .base
            .as_ref()
            .map(|metric| (metric.optimum, metric.optimum_count));
        let baseline_base = baseline
            .result
            .as_ref()
            .map(|metric| (metric.optimum, metric.optimum_count));
        if baseline.string != context.string || baseline_base != context_base {
            return Err(std::io::Error::other(format!(
                "B0 drift in {} target {} string {}",
                line.id, edge.target.note_id, baseline.string
            )));
        }
    }
    let chord = edge
        .target_chord
        .iter()
        .map(|atom| ChordAtomRecord {
            voice_note_id: atom.note_id,
            duration: atom.duration,
            pitch: atom.pitch.0,
            imported_position: atom.original_position.map(Into::into),
            tapped: atom.tapped,
            legato_target: atom.note_id == edge.target.note_id,
        })
        .collect();
    Ok(ContextCaseRecord {
        schema: "griff.constraint-lab-legato-chord-context",
        version: 1,
        id: line.id.clone(),
        file: file.clone(),
        song: song_key(&file),
        family: format_family(&file),
        test: line.test,
        track: line.tab.track,
        voice: line.tab.voice,
        origin_line_start_tick: line.tab.start_tick,
        origin_note_id: edge.origin_note_id,
        origin_onset_tick: line.tab.onsets[edge.from],
        origin_pitch: line.tab.pitches[edge.from].0,
        origin_position: origin_position.into(),
        origin_tapped: line.tab.tapped[edge.from],
        anchor_fret: edge.target_anchor_fret,
        target_note_id: edge.target.note_id,
        target_pitch: edge.target.pitch.0,
        chord_onset_tick: edge.target.onset,
        observed_string: edge.target.original_position.string,
        chord,
        baseline_outcome: outcome,
        baseline_delta_cost: delta_cost,
        base_rank: analysis.observed.base_rank,
        origin_rank: analysis.observed.origin_rank,
        anchor_rank: analysis.observed.anchor_rank,
        origin_anchor_rank: analysis.observed.origin_anchor_rank,
        anchor_origin_rank: analysis.observed.anchor_origin_rank,
        pareto_member: analysis.observed.pareto_member,
        origin_change: analysis.observed.origin_change.into(),
        anchor_change: analysis.observed.anchor_change.into(),
        origin_anchor_change: analysis.observed.origin_anchor_change.into(),
        anchor_origin_change: analysis.observed.anchor_origin_change.into(),
        target_strings: analysis.target_strings.iter().map(Into::into).collect(),
        human: analysis.human.as_ref().map(Into::into),
    })
}

fn median_i64(values: impl Iterator<Item = i64>) -> Option<i64> {
    let mut values: Vec<i64> = values.collect();
    values.sort_unstable();
    values.get(values.len().checked_sub(1)? / 2).copied()
}

fn legato_chord_context(corpus: Corpus, out: &Path) -> std::io::Result<()> {
    let policy = ChordCostPolicy::v1_unary();
    let mut records = Vec::new();
    for line in &corpus.lines {
        for edge in &line.tab.cross_line_edges {
            if edge.boundary.target_disposition == TargetDisposition::Excluded
                && edge.target_chord.len() > 1
                && edge
                    .target_chord
                    .iter()
                    .any(|atom| atom.note_id == edge.target.note_id)
            {
                records.push(context_case(&corpus, line, edge, &policy)?);
            }
        }
    }
    records.sort_by(|a, b| {
        (
            &a.file,
            a.track,
            a.voice,
            a.chord_onset_tick,
            a.target_note_id,
        )
            .cmp(&(
                &b.file,
                b.track,
                b.voice,
                b.chord_onset_tick,
                b.target_note_id,
            ))
    });

    let context_path = out.join("legato-chord-context.jsonl");
    let mut writer = BufWriter::new(fs::File::create(&context_path)?);
    for record in &records {
        serde_json::to_writer(&mut writer, record).map_err(std::io::Error::other)?;
        writer.write_all(b"\n")?;
    }
    writer.flush()?;

    let mut baseline_rank_counts = BTreeMap::new();
    let mut baseline_outcomes = OutcomeCounts::default();
    let mut legal = 0;
    let mut feasible = 0;
    let mut anchors_present = 0;
    let mut pareto = 0;
    let mut human_complete = 0;
    let mut human_pareto = 0;
    let mut human_origin_excess = Vec::new();
    let mut human_anchor_excess = Vec::new();
    let mut songs: BTreeMap<String, Vec<&ContextCaseRecord>> = BTreeMap::new();
    for record in &records {
        if let Some(rank) = record.base_rank {
            *baseline_rank_counts.entry(rank).or_insert(0) += 1;
        }
        baseline_outcomes.add(record.baseline_outcome);
        legal += record.target_strings.len();
        feasible += record
            .target_strings
            .iter()
            .filter(|condition| condition.base.is_some())
            .count();
        anchors_present += usize::from(record.anchor_fret.is_some());
        pareto += usize::from(record.pareto_member == Some(true));
        if let Some(human) = &record.human {
            human_complete += 1;
            human_pareto += usize::from(human.pareto_member == Some(true));
            if let Some(excess) = human.origin_excess {
                human_origin_excess.push(excess);
            }
            if let Some(excess) = human.anchor_excess {
                human_anchor_excess.push(excess);
            }
        }
        songs.entry(record.song.clone()).or_default().push(record);
    }
    if records.len() != 38
        || legal != 176
        || feasible != 174
        || baseline_rank_counts.get(&1) != Some(&11)
        || baseline_rank_counts.get(&2) != Some(&24)
        || baseline_rank_counts.get(&3) != Some(&3)
        || baseline_outcomes.free != 11
        || baseline_outcomes.costly != 27
        || baseline_outcomes.infeasible != 0
    {
        return Err(std::io::Error::other(format!(
            "#209 B0 drift: cases={}, legal={}, feasible={}, ranks={baseline_rank_counts:?}, outcomes={}/{}/{}",
            records.len(), legal, feasible, baseline_outcomes.free, baseline_outcomes.costly, baseline_outcomes.infeasible
        )));
    }

    let views: Vec<ViewAggregate> = ContextView::ALL
        .into_iter()
        .map(|view| aggregate_view(records.iter(), view))
        .collect();
    let costly_views: Vec<ViewAggregate> = ContextView::ALL
        .into_iter()
        .map(|view| {
            aggregate_view(
                records
                    .iter()
                    .filter(|record| record.baseline_outcome == "feasible_costly"),
                view,
            )
        })
        .collect();
    let summary = ContextSummary {
        schema: "griff.constraint-lab-legato-chord-context-summary",
        version: 1,
        population: records.len(),
        baseline_legal_conditions: legal,
        baseline_feasible_conditions: feasible,
        baseline_rank_counts,
        baseline_outcomes,
        anchors_present,
        observed_pareto_members: pareto,
        views,
        costly_views,
        human: ContextHumanSummary {
            complete: human_complete,
            pareto_member: human_pareto,
            origin_excess: distribution(&human_origin_excess),
            anchor_excess: distribution(&human_anchor_excess),
        },
        songs: songs.len(),
        corpus: corpus.facts,
    };
    write_json(&out.join("legato-chord-context-summary.json"), &summary)?;

    let song_summary: Vec<SongContextSummary> = songs
        .iter()
        .map(|(song, cases)| SongContextSummary {
            song: song.clone(),
            cases: cases.len(),
            views: ContextView::ALL
                .into_iter()
                .map(|view| SongViewSummary {
                    view: view.name(),
                    base_rank_one: cases
                        .iter()
                        .filter(|record| record.base_rank == Some(1))
                        .count(),
                    context: aggregate_view(cases.iter().copied(), view),
                })
                .collect(),
        })
        .collect();
    write_json(
        &out.join("legato-chord-context-song-summary.json"),
        &song_summary,
    )?;

    let mut loo = Vec::new();
    for omitted_song in songs.keys() {
        let kept: Vec<&ContextCaseRecord> = records
            .iter()
            .filter(|record| &record.song != omitted_song)
            .collect();
        for view in ContextView::ALL {
            let aggregate = aggregate_view(kept.iter().copied(), view);
            let base_rank_one = kept
                .iter()
                .filter(|record| record.base_rank == Some(1))
                .count();
            loo.push(LooContextSummary {
                omitted_song: omitted_song.clone(),
                remaining_cases: kept.len(),
                view: view.name(),
                base_rank_one,
                context_rank_one: aggregate.rank_one,
                rank_one_gain: i64::try_from(aggregate.rank_one).unwrap_or(i64::MAX)
                    - i64::try_from(base_rank_one).unwrap_or(i64::MAX),
                improved: aggregate.changes.improved,
                unchanged: aggregate.changes.unchanged,
                worsened: aggregate.changes.worsened,
                median_rank_delta: median_i64(
                    kept.iter()
                        .filter_map(|record| view.change(record).rank_delta),
                ),
            });
        }
    }
    write_json(&out.join("legato-chord-context-loo.json"), &loo)?;
    eprintln!("wrote {}", context_path.display());
    println!(
        "legato chord context: {} cases, {} anchors, {} observed Pareto members",
        summary.population, summary.anchors_present, summary.observed_pareto_members
    );
    Ok(())
}

#[derive(Serialize)]
struct BoundaryRegimeRecord {
    feasible: bool,
    human_path_feasible: bool,
    optimum: Option<i64>,
    optimum_count: Option<u64>,
    human_in_optimum: Option<bool>,
    agreement_floor: Option<usize>,
    agreement_uniform: Option<f64>,
    agreement_ceiling: Option<usize>,
    deterministic_matches: Option<usize>,
    deterministic_non_target_matches: Option<usize>,
    deterministic_target_match: Option<bool>,
}

#[derive(Serialize)]
struct BoundaryReplayRecord {
    schema: &'static str,
    source: String,
    song: String,
    track: usize,
    voice: u8,
    origin_note_id: usize,
    target_note_id: usize,
    origin_line_start: u32,
    target_line_start: u32,
    target_index: usize,
    origin_string: u8,
    anchor_fret: Option<u8>,
    independent: BoundaryRegimeRecord,
    hand: Option<BoundaryRegimeRecord>,
    technique: BoundaryRegimeRecord,
    both: Option<BoundaryRegimeRecord>,
    causal: BoundaryRegimeRecord,
    causal_required_string: u8,
    causal_anchor_fret: Option<u8>,
    causal_matches_oracle_context: bool,
    causal_transport_equal: bool,
}

struct CausalReplay {
    regime: BoundaryRegimeRecord,
    required_string: u8,
    anchor_fret: Option<u8>,
    transport_equal: bool,
}

type CausalReplayKey = (usize, usize, u8, usize);
type CausalReplayMap = BTreeMap<CausalReplayKey, CausalReplay>;

fn boundary_regime(
    chain: Option<Chain>,
    human: &[FretboardPosition],
    target: usize,
    secondary: Option<&Features>,
) -> BoundaryRegimeRecord {
    let Some(chain) = chain else {
        return BoundaryRegimeRecord {
            feasible: false,
            human_path_feasible: false,
            optimum: None,
            optimum_count: None,
            human_in_optimum: None,
            agreement_floor: None,
            agreement_uniform: None,
            agreement_ceiling: None,
            deterministic_matches: None,
            deterministic_non_target_matches: None,
            deterministic_target_match: None,
        };
    };
    let set = optimum_set(&chain, Some(human));
    let human_path_feasible = human
        .iter()
        .enumerate()
        .all(|(note, position)| chain.candidates(note).contains(position));
    let agreement = set.agreement.as_ref();
    let zero = [0; FEATURES];
    let path = lexicographic_path(&chain, secondary.unwrap_or(&zero), None);
    let positions = chain.positions_of(&path).unwrap_or_default();
    let deterministic_matches = positions
        .iter()
        .zip(human)
        .filter(|(actual, expected)| actual == expected)
        .count();
    let deterministic_non_target_matches = positions
        .iter()
        .zip(human)
        .enumerate()
        .filter(|(index, (actual, expected))| *index != target && actual == expected)
        .count();
    BoundaryRegimeRecord {
        feasible: true,
        human_path_feasible,
        optimum: Some(set.optimum),
        optimum_count: Some(set.count.exact),
        human_in_optimum: agreement.map(|value| value.max == human.len()),
        agreement_floor: agreement.map(|value| value.min),
        agreement_uniform: agreement.map(|value| value.expected),
        agreement_ceiling: agreement.map(|value| value.max),
        deterministic_matches: Some(deterministic_matches),
        deterministic_non_target_matches: Some(deterministic_non_target_matches),
        deterministic_target_match: positions
            .get(target)
            .zip(human.get(target))
            .map(|(actual, expected)| actual == expected),
    }
}

#[derive(Serialize)]
struct BoundaryReplaySummary {
    schema: &'static str,
    cases: usize,
    anchors_present: usize,
    technique_feasible: usize,
    technique_human_feasible: usize,
    hand_better_equal_worse: [usize; 3],
    technique_better_equal_worse: [usize; 3],
    both_better_equal_worse: [usize; 3],
    hand_whole_better_equal_worse: [usize; 3],
    technique_whole_better_equal_worse: [usize; 3],
    both_whole_better_equal_worse: [usize; 3],
    target_matches: [usize; 4],
    imported_path_primary_optimum: [usize; 4],
    causal_feasible: usize,
    causal_target_matches: usize,
    causal_string_matches_oracle: usize,
    causal_anchor_matches_oracle: usize,
    causal_context_matches_oracle: usize,
    causal_transport_equal: usize,
    causal_whole_better_equal_worse: [usize; 3],
    causal_non_target_better_equal_worse: [usize; 3],
    within_line_relations: u64,
    cross_line_relations: u64,
}

fn boundary_kind(kind: TechniqueKind) -> griff_constraint_lab::boundary_context::TechniqueKind {
    match kind {
        TechniqueKind::HammerOn => griff_constraint_lab::boundary_context::TechniqueKind::HammerOn,
        TechniqueKind::PullOff => griff_constraint_lab::boundary_context::TechniqueKind::PullOff,
        TechniqueKind::Legato => griff_constraint_lab::boundary_context::TechniqueKind::Legato,
    }
}

fn causal_boundary_replay(
    corpus: &Corpus,
    weights: &FingeringWeights,
) -> std::io::Result<CausalReplayMap> {
    let mut order: Vec<usize> = (0..corpus.lines.len()).collect();
    order.sort_by_key(|&index| {
        let line = &corpus.lines[index];
        (
            line.file,
            line.tab.track,
            line.tab.voice,
            line.tab.start_tick,
        )
    });
    let mut contexts: BTreeMap<(usize, usize, u8), BoundaryContext> = BTreeMap::new();
    let mut output = BTreeMap::new();
    let mut anchor_features = [0; FEATURES];
    anchor_features[FEATURES - 1] = 1;
    for index in order {
        let line = &corpus.lines[index];
        let key = (line.file, line.tab.track, line.tab.voice);
        let voice = VoiceIdentity::new(
            corpus.names[line.file].clone(),
            line.tab.track,
            line.tab.voice,
        );
        let context = contexts
            .remove(&key)
            .unwrap_or_else(|| BoundaryContext::unknown(voice.clone()));
        let direct = consume_for_line(context.clone(), &voice, &line.tab.note_ids)
            .map_err(std::io::Error::other)?;
        let bytes = encode_context(&context).map_err(std::io::Error::other)?;
        let transported = decode_context(&bytes).map_err(std::io::Error::other)?;
        let consumed = consume_for_line(transported, &voice, &line.tab.note_ids)
            .map_err(std::io::Error::other)?;
        let transport_equal = direct == consumed;
        if !transport_equal {
            return Err(std::io::Error::other(
                "serialized transport changed consumption",
            ));
        }
        let base = Chain::v1(
            &line.tab.pitches,
            &line.tab.tuning,
            weights,
            STANDARD_MAX_FRET,
        )
        .map_err(std::io::Error::other)?;
        let conditioned = condition_consumer_chain(base, &line.tab.note_ids, &consumed)
            .map_err(std::io::Error::other)?;
        let anchor = match consumed.anchor_fret() {
            Ok(value) => value,
            Err(griff_constraint_lab::boundary_context::BoundaryContextError::UnknownHand) => None,
            Err(error) => return Err(std::io::Error::other(error)),
        };
        let chain = conditioned.with_anchor(anchor);
        let secondary = anchor.map(|_| &anchor_features);
        for obligation in consumed.consumed() {
            let target = line
                .tab
                .note_ids
                .iter()
                .position(|note_id| *note_id == obligation.target_note_id())
                .ok_or_else(|| std::io::Error::other("consumed target disappeared"))?;
            output.insert(
                (
                    line.file,
                    line.tab.track,
                    line.tab.voice,
                    obligation.target_note_id(),
                ),
                CausalReplay {
                    regime: boundary_regime(
                        Some(chain.clone()),
                        &line.tab.human,
                        target,
                        secondary,
                    ),
                    required_string: obligation.required_string(),
                    anchor_fret: anchor,
                    transport_equal,
                },
            );
        }
        let zero = [0; FEATURES];
        let path = lexicographic_path(&chain, secondary.unwrap_or(&zero), None);
        let positions = chain
            .positions_of(&path)
            .ok_or_else(|| std::io::Error::other("causal path is ragged"))?;
        let solved = SolvedPartition::new(
            voice,
            line.tab
                .note_ids
                .iter()
                .zip(&line.tab.onsets)
                .zip(&positions)
                .zip(&line.tab.tapped)
                .map(|(((note_id, onset), position), tapped)| {
                    SolvedNote::new(*note_id, *onset, *position, *tapped)
                })
                .collect(),
        )
        .map_err(std::io::Error::other)?;
        let relations: Vec<_> = line
            .tab
            .cross_line_edges
            .iter()
            .filter(|edge| edge.boundary.target_disposition == TargetDisposition::KeptLine)
            .map(|edge| {
                ProjectedTechnique::new(
                    edge.origin_note_id,
                    line.tab.onsets[edge.from],
                    edge.target.note_id,
                    boundary_kind(edge.kind),
                )
            })
            .collect();
        let outgoing = produce_context(consumed.remaining(), &solved, &relations)
            .map_err(std::io::Error::other)?;
        contexts.insert(
            key,
            decode_context(&encode_context(&outgoing).map_err(std::io::Error::other)?)
                .map_err(std::io::Error::other)?,
        );
    }
    Ok(output)
}

fn compare_non_target(
    baseline: &BoundaryRegimeRecord,
    context: &BoundaryRegimeRecord,
    counts: &mut [usize; 3],
) {
    let Some(delta) = baseline
        .deterministic_non_target_matches
        .zip(context.deterministic_non_target_matches)
        .map(|(base, value)| value.cmp(&base))
    else {
        return;
    };
    counts[match delta {
        std::cmp::Ordering::Greater => 0,
        std::cmp::Ordering::Equal => 1,
        std::cmp::Ordering::Less => 2,
    }] += 1;
}

fn compare_whole(
    baseline: &BoundaryRegimeRecord,
    context: &BoundaryRegimeRecord,
    counts: &mut [usize; 3],
) {
    let Some(ordering) = baseline
        .deterministic_matches
        .zip(context.deterministic_matches)
        .map(|(base, value)| value.cmp(&base))
    else {
        return;
    };
    counts[match ordering {
        std::cmp::Ordering::Greater => 0,
        std::cmp::Ordering::Equal => 1,
        std::cmp::Ordering::Less => 2,
    }] += 1;
}

fn boundary_context_replay(corpus: &Corpus, out: &Path) -> std::io::Result<()> {
    let weights = FingeringWeights {
        fret: 0,
        open_string: -3,
        position_shift: 1,
        string_change: 0,
    };
    let mut causal = causal_boundary_replay(corpus, &weights)?;
    let mut records = Vec::new();
    for origin in &corpus.lines {
        for edge in &origin.tab.cross_line_edges {
            if edge.boundary.target_disposition != TargetDisposition::KeptLine {
                continue;
            }
            let target_start = edge
                .boundary
                .target_line_start_tick
                .ok_or_else(|| std::io::Error::other("kept target has no line start"))?;
            let candidates: Vec<_> = corpus
                .lines
                .iter()
                .filter(|line| {
                    line.file == origin.file
                        && line.tab.track == origin.tab.track
                        && line.tab.voice == origin.tab.voice
                        && line.tab.start_tick == target_start
                })
                .collect();
            if candidates.len() != 1 {
                return Err(std::io::Error::other("kept target line is not unique"));
            }
            let target_line = candidates[0];
            let target_index = target_line
                .tab
                .note_ids
                .iter()
                .position(|note_id| *note_id == edge.target.note_id)
                .ok_or_else(|| std::io::Error::other("target stable id missing from kept line"))?;
            let origin_string = origin
                .tab
                .original_positions
                .get(edge.from)
                .ok_or_else(|| std::io::Error::other("origin position missing"))?
                .string;
            if target_line.tab.human[target_index].string != origin_string {
                return Err(std::io::Error::other(
                    "corrected same-string projection drift",
                ));
            }
            let base = Chain::v1(
                &target_line.tab.pitches,
                &target_line.tab.tuning,
                &weights,
                STANDARD_MAX_FRET,
            )
            .map_err(std::io::Error::other)?;
            let conditioned = base.clone().condition_string(target_index, origin_string);
            let mut anchor_features = [0; FEATURES];
            anchor_features[FEATURES - 1] = 1;
            let independent = boundary_regime(
                Some(base.clone()),
                &target_line.tab.human,
                target_index,
                None,
            );
            let hand = target_line.tab.anchor_fret.map(|anchor| {
                boundary_regime(
                    Some(base.clone().with_anchor(Some(anchor))),
                    &target_line.tab.human,
                    target_index,
                    Some(&anchor_features),
                )
            });
            let technique = boundary_regime(
                conditioned.clone(),
                &target_line.tab.human,
                target_index,
                None,
            );
            let both = target_line.tab.anchor_fret.map(|anchor| {
                boundary_regime(
                    conditioned.map(|chain| chain.with_anchor(Some(anchor))),
                    &target_line.tab.human,
                    target_index,
                    Some(&anchor_features),
                )
            });
            let causal_replay = causal
                .remove(&(
                    origin.file,
                    origin.tab.track,
                    origin.tab.voice,
                    edge.target.note_id,
                ))
                .ok_or_else(|| std::io::Error::other("causal obligation was not consumed"))?;
            let causal_matches_oracle_context = causal_replay.required_string == origin_string
                && causal_replay.anchor_fret == target_line.tab.anchor_fret;
            records.push(BoundaryReplayRecord {
                schema: "griff.constraint-lab-boundary-context-replay.v1",
                source: corpus.names[origin.file].clone(),
                song: song_key(&corpus.names[origin.file]),
                track: origin.tab.track,
                voice: origin.tab.voice,
                origin_note_id: edge.origin_note_id,
                target_note_id: edge.target.note_id,
                origin_line_start: origin.tab.start_tick,
                target_line_start: target_start,
                target_index,
                origin_string,
                anchor_fret: target_line.tab.anchor_fret,
                independent,
                hand,
                technique,
                both,
                causal: causal_replay.regime,
                causal_required_string: causal_replay.required_string,
                causal_anchor_fret: causal_replay.anchor_fret,
                causal_matches_oracle_context,
                causal_transport_equal: causal_replay.transport_equal,
            });
        }
    }
    records.sort_by(|left, right| {
        (
            &left.source,
            left.track,
            left.voice,
            left.origin_note_id,
            left.target_note_id,
        )
            .cmp(&(
                &right.source,
                right.track,
                right.voice,
                right.origin_note_id,
                right.target_note_id,
            ))
    });
    if records.len() != 8 {
        return Err(std::io::Error::other(format!(
            "boundary replay population drift: {} != 8",
            records.len()
        )));
    }
    let mut hand_counts = [0; 3];
    let mut technique_counts = [0; 3];
    let mut both_counts = [0; 3];
    let mut hand_whole = [0; 3];
    let mut technique_whole = [0; 3];
    let mut both_whole = [0; 3];
    let mut causal_whole = [0; 3];
    let mut causal_non_target = [0; 3];
    for record in &records {
        if let Some(hand) = &record.hand {
            compare_non_target(&record.independent, hand, &mut hand_counts);
            compare_whole(&record.independent, hand, &mut hand_whole);
        }
        compare_non_target(
            &record.independent,
            &record.technique,
            &mut technique_counts,
        );
        compare_whole(&record.independent, &record.technique, &mut technique_whole);
        if let Some(both) = &record.both {
            compare_non_target(&record.independent, both, &mut both_counts);
            compare_whole(&record.independent, both, &mut both_whole);
        }
        compare_whole(&record.independent, &record.causal, &mut causal_whole);
        compare_non_target(&record.independent, &record.causal, &mut causal_non_target);
    }
    let summary = BoundaryReplaySummary {
        schema: "griff.constraint-lab-boundary-context-replay-summary.v1",
        cases: records.len(),
        anchors_present: records
            .iter()
            .filter(|record| record.hand.is_some())
            .count(),
        technique_feasible: records
            .iter()
            .filter(|record| record.technique.feasible)
            .count(),
        technique_human_feasible: records
            .iter()
            .filter(|record| record.technique.human_path_feasible)
            .count(),
        hand_better_equal_worse: hand_counts,
        technique_better_equal_worse: technique_counts,
        both_better_equal_worse: both_counts,
        hand_whole_better_equal_worse: hand_whole,
        technique_whole_better_equal_worse: technique_whole,
        both_whole_better_equal_worse: both_whole,
        target_matches: [
            records
                .iter()
                .filter(|record| record.independent.deterministic_target_match == Some(true))
                .count(),
            records
                .iter()
                .filter(|record| {
                    record
                        .hand
                        .as_ref()
                        .is_some_and(|value| value.deterministic_target_match == Some(true))
                })
                .count(),
            records
                .iter()
                .filter(|record| record.technique.deterministic_target_match == Some(true))
                .count(),
            records
                .iter()
                .filter(|record| {
                    record
                        .both
                        .as_ref()
                        .is_some_and(|value| value.deterministic_target_match == Some(true))
                })
                .count(),
        ],
        imported_path_primary_optimum: [
            records
                .iter()
                .filter(|record| record.independent.human_in_optimum == Some(true))
                .count(),
            records
                .iter()
                .filter(|record| {
                    record
                        .hand
                        .as_ref()
                        .is_some_and(|value| value.human_in_optimum == Some(true))
                })
                .count(),
            records
                .iter()
                .filter(|record| record.technique.human_in_optimum == Some(true))
                .count(),
            records
                .iter()
                .filter(|record| {
                    record
                        .both
                        .as_ref()
                        .is_some_and(|value| value.human_in_optimum == Some(true))
                })
                .count(),
        ],
        causal_feasible: records
            .iter()
            .filter(|record| record.causal.feasible)
            .count(),
        causal_target_matches: records
            .iter()
            .filter(|record| record.causal.deterministic_target_match == Some(true))
            .count(),
        causal_string_matches_oracle: records
            .iter()
            .filter(|record| record.causal_required_string == record.origin_string)
            .count(),
        causal_anchor_matches_oracle: records
            .iter()
            .filter(|record| record.causal_anchor_fret == record.anchor_fret)
            .count(),
        causal_context_matches_oracle: records
            .iter()
            .filter(|record| record.causal_matches_oracle_context)
            .count(),
        causal_transport_equal: records
            .iter()
            .filter(|record| record.causal_transport_equal)
            .count(),
        causal_whole_better_equal_worse: causal_whole,
        causal_non_target_better_equal_worse: causal_non_target,
        within_line_relations: 29_758,
        cross_line_relations: corpus.facts.cut_stats.cross_line_legato,
    };
    let mut writer = BufWriter::new(fs::File::create(out.join("boundary-context-replay.jsonl"))?);
    for record in &records {
        serde_json::to_writer(&mut writer, record).map_err(std::io::Error::other)?;
        writer.write_all(b"\n")?;
    }
    writer.flush()?;
    write_json(&out.join("boundary-context-replay.json"), &summary)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&summary).map_err(std::io::Error::other)?
    );
    Ok(())
}

// ── entry ─────────────────────────────────────────────────────────────────────

struct Args {
    command: String,
    tabs: PathBuf,
    out: PathBuf,
    models: Vec<Model>,
}

fn parse_args() -> Result<Args, String> {
    let mut it = std::env::args().skip(1);
    let command = it.next().ok_or("missing command (fit | export | report)")?;
    let (mut tabs, mut out, mut models) = (None, None, Vec::new());
    while let Some(flag) = it.next() {
        let value = it.next().ok_or_else(|| format!("{flag} needs a value"))?;
        match flag.as_str() {
            "--tabs" => tabs = Some(PathBuf::from(value)),
            "--out" => out = Some(PathBuf::from(value)),
            "--v1" | "--hand" => models.push(parse_model(&flag, &value)?),
            _ => return Err(format!("unknown flag {flag}")),
        }
    }
    if models.is_empty() {
        models.push(parse_model("--v1", "v1=1,1,2,1")?);
    }
    Ok(Args {
        command,
        tabs: tabs.ok_or("--tabs DIR is required")?,
        out: out.ok_or("--out DIR is required")?,
        models,
    })
}

fn run() -> Result<(), String> {
    let args = parse_args()?;
    fs::create_dir_all(&args.out).map_err(|e| e.to_string())?;
    let started = Instant::now();
    let corpus = load(&args.tabs, &LineCut::v1()).map_err(|e| e.to_string())?;
    eprintln!(
        "corpus: {} files ({} failed), {} guitar tracks, {} + {} lines, {} + {} notes (train + test), {:.1}s",
        corpus.facts.files,
        corpus.facts.import_failures,
        corpus.facts.guitar_tracks,
        corpus.facts.lines_train,
        corpus.facts.lines_test,
        corpus.facts.notes_train,
        corpus.facts.notes_test,
        started.elapsed().as_secs_f64()
    );
    let result = match args.command.as_str() {
        "fit" => fit(corpus, &args.out),
        "export" => export(&corpus, &args.models, &args.out),
        "report" => report(corpus, &args.models, &args.out),
        "repeat-export" => repeat_export(&corpus, &args.models, &args.out),
        "repeat-report" => repeat_report(&corpus, &args.models, &args.out),
        "ties-check" => ties_check(&corpus, &args.models, &args.out),
        "tiebreak" => tiebreak(corpus, &args.models, &args.out),
        "taps" => taps(corpus, &args.out),
        "legato-census" => legato_census(corpus, &args.out),
        "legato" => legato(corpus, &args.out),
        "legato-chords" => legato_chords(corpus, &args.out),
        "legato-chord-context" => legato_chord_context(corpus, &args.out),
        "boundary-context" => boundary_context_replay(&corpus, &args.out),
        other => return Err(format!("unknown command {other}")),
    };
    result.map_err(|e| e.to_string())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("fingering_gap: {e}");
            ExitCode::FAILURE
        }
    }
}
