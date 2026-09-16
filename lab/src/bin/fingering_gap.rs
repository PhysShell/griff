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

use griff_constraint_lab::fingering::{
    best_hands, decode_positions, hand_problem, holdout_bucket, repeat_pairs, solve_hand, song_key,
    tab_lines, v1_cost, v1_problem, with_repeat_consistency, with_string_tiebreak, CutStats,
    HandModel, HandWeights, LineCut, TabLine, HAND_VARS_PER_NOTE, V1_VARS_PER_NOTE,
};
use griff_constraint_lab::ir::VarId;
use griff_constraint_lab::optir::{
    verify_agreement, verify_record, OptProblem, ProblemRecord, SolveRecord, Verdict,
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
        let test = holdout_bucket(&key, HOLDOUT_BUCKETS) == 0;
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
        let rate = |x: u64| 100.0 * x as f64 / e.ceiling_notes.max(1) as f64;
        println!(
            "| {name} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {:.1}% | {:.1}% | {:.1} / {:.1} / {:.1} | {:.1} |",
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
