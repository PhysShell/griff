//! Oracle spike runner: builds the two Constraint Inventory fixture problems,
//! emits `MiniZinc` models, solves with the exact reference solver, optionally
//! cross-checks against an external `minizinc` (path in `MINIZINC_BIN`, or
//! `minizinc` on `PATH`), and archives run manifests.
//!
//! Usage (from `lab/`):
//!
//! ```text
//! cargo run --bin oracle            # reference solver only
//! MINIZINC_BIN=/path/to/minizinc cargo run --bin oracle
//! ```
//!
//! Outputs: `fixtures/*.mzn` (emitted models) and `runs/*.json` (manifests),
//! relative to the current directory. Exits non-zero if the external solver
//! disagrees with the reference solver on any fixture.

use std::fs;
use std::path::Path;
use std::process::Command;

use griff_constraint_lab::emit::to_minizinc;
use griff_constraint_lab::ir::OracleProblem;
use griff_constraint_lab::manifest::{
    OutcomeRecord, RunManifest, SolverIdentity, Status, SCHEMA, SCHEMA_VERSION,
};
use griff_constraint_lab::problems::{bounded_travel_problem, pair_cleanliness_problem, PairSpec};
use griff_constraint_lab::solve::{solve_exact, verify_witness, Outcome};
use griff_core::event::{NoteMarks, Pitch, Tempo, Ticks, TimeSignature, Tuning, Velocity};
use griff_core::fretboard::STANDARD_MAX_FRET;
use griff_core::score::{
    AtomEvent, AtomNote, EventGroup, EventGroupKind, LossReport, MasterBar, RepeatMarker, Score,
    Track, Voice,
};
use griff_core::slice::TickRange;

const QUARTER: u32 = 480;

fn pitch(p: u8) -> Pitch {
    Pitch::new(p).expect("fixture pitch is valid MIDI")
}

/// The Problem A fixture line: a swancore-flavoured E-minor run that spans
/// registers, so tight travel bounds actually bite.
fn fixture_line() -> Vec<Pitch> {
    [40u8, 47, 52, 55, 59, 64, 67, 71]
        .iter()
        .map(|&p| pitch(p))
        .collect()
}

/// Builds a one-bar 4/4 canonical `Score` whose single Standard-E track
/// plays the given `(onset, pitch)` quarter notes — every Problem B fixture
/// starts from the canonical model, per the `PairSpec` contract.
fn part_a_score(notes: &[(u32, u8)]) -> Score {
    const BAR: u32 = 4 * QUARTER;
    Score {
        ticks_per_quarter: 480,
        master_bars: vec![MasterBar {
            index: 0,
            tick_range: TickRange::new(Ticks(0), Ticks(BAR)).expect("ordered"),
            time_signature: TimeSignature {
                numerator: 4,
                denominator: 4,
            },
            tempo: Tempo::from_bpm_integer(120).expect("120 BPM"),
            repeat: RepeatMarker::default(),
        }],
        tracks: vec![Track {
            name: Some("A".to_string()),
            channel: 0,
            voices: vec![Voice {
                id: 0,
                event_groups: notes
                    .iter()
                    .map(|&(onset, p)| EventGroup {
                        kind: EventGroupKind::Single,
                        atoms: vec![AtomEvent::Note(AtomNote {
                            absolute_start: Ticks(onset),
                            duration: Ticks(QUARTER),
                            pitch: pitch(p),
                            velocity: Velocity::new(90).expect("valid velocity"),
                            marks: NoteMarks::empty(),
                            position: None,
                        })],
                        technique_spans: Vec::new(),
                    })
                    .collect(),
            }],
            tuning: Tuning::standard_e(),
        }],
        source_meta: None,
        loss: LossReport::new(),
    }
}

/// The Problem B SAT fixture: A in the low-mid register, B constrained to a
/// consonant upper-register domain.
fn fixture_pair_sat() -> PairSpec {
    let score = part_a_score(&[(0, 52), (QUARTER, 55), (2 * QUARTER, 52), (3 * QUARTER, 57)]);
    PairSpec::from_score_part_a(
        &score,
        0,
        vec![0, QUARTER, 2 * QUARTER, 3 * QUARTER],
        [64u8, 66, 67, 69, 71, 72, 74, 76]
            .iter()
            .map(|&p| pitch(p))
            .collect(),
        Tuning::standard_e(),
    )
    .expect("SAT fixture spec builds")
}

/// The Problem B UNSAT fixture: no coincident onsets, but the only available
/// B band sits inside A's — register mud alone forces UNSAT.
fn fixture_pair_mud() -> PairSpec {
    let score = part_a_score(&[(0, 45), (QUARTER, 57)]);
    PairSpec::from_score_part_a(
        &score,
        0,
        vec![2 * QUARTER, 3 * QUARTER],
        [48u8, 50, 52].iter().map(|&p| pitch(p)).collect(),
        Tuning::standard_e(),
    )
    .expect("mud fixture spec builds")
}

fn outcome_record(problem: &OracleProblem, outcome: &Outcome) -> OutcomeRecord {
    match outcome {
        Outcome::Sat { witness, nodes } => OutcomeRecord {
            status: Status::Sat,
            witness: Some(
                problem
                    .vars()
                    .iter()
                    .zip(witness)
                    .map(|(v, &value)| (v.name.clone(), value))
                    .collect(),
            ),
            nodes: *nodes,
        },
        Outcome::Unsat { nodes } => OutcomeRecord {
            status: Status::Unsat,
            witness: None,
            nodes: *nodes,
        },
    }
}

/// Locates an external `MiniZinc` binary, if any.
fn minizinc_bin() -> Option<String> {
    if let Ok(explicit) = std::env::var("MINIZINC_BIN") {
        return Some(explicit);
    }
    let probe = Command::new("minizinc").arg("--version").output().ok()?;
    probe.status.success().then(|| "minizinc".to_string())
}

/// Frontend + backend identity: the `MiniZinc` converter version *and* the
/// `Chuffed` backend version, recorded separately for reproducibility.
fn minizinc_version(bin: &str) -> String {
    let frontend = Command::new(bin)
        .arg("--version")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.lines().next().map(str::to_string))
        .unwrap_or_else(|| "unknown".to_string());
    let backend = Command::new(bin)
        .arg("--solvers")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| {
            s.lines()
                .find(|l| l.to_ascii_lowercase().contains("chuffed"))
                .map(|l| l.trim().to_string())
        })
        .unwrap_or_else(|| "chuffed version unknown".to_string());
    format!("frontend: {frontend}; backend: {backend}")
}

/// Runs the external solver on an emitted model and parses the outcome.
fn run_minizinc(bin: &str, model_path: &Path) -> Result<OutcomeRecord, String> {
    let output = Command::new(bin)
        .arg("--solver")
        .arg("chuffed")
        .arg(model_path)
        .output()
        .map_err(|e| format!("failed to spawn {bin}: {e}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    // A failed run must never be archived as evidence — gate on exit status
    // before interpreting any marker in the output.
    if !output.status.success() {
        return Err(format!(
            "{bin} exited with {}:\n{stdout}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    if stdout.contains("=====UNSATISFIABLE=====") {
        return Ok(OutcomeRecord {
            status: Status::Unsat,
            witness: None,
            nodes: 0,
        });
    }
    let mut witness: Vec<(String, i64)> = Vec::new();
    for line in stdout.lines() {
        if let Some((name, value)) = line.split_once('=') {
            if let Ok(value) = value.trim().parse::<i64>() {
                witness.push((name.trim().to_string(), value));
            }
        }
    }
    if witness.is_empty() {
        return Err(format!(
            "no witness parsed from {bin} output:\n{stdout}\n{}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(OutcomeRecord {
        status: Status::Sat,
        witness: Some(witness),
        nodes: 0,
    })
}

fn write_manifest(dir: &Path, slug: &str, manifest: &RunManifest) {
    let path = dir.join(format!("{slug}.json"));
    let json = serde_json::to_string_pretty(manifest).expect("manifest serializes");
    fs::write(&path, json + "\n").expect("manifest writes");
    println!("  archived {}", path.display());
}

/// One fixture through the whole pipeline. Returns `true` when every solver
/// agreed on the SAT/UNSAT status.
fn run_fixture(
    slug: &str,
    problem: &OracleProblem,
    fixtures: &Path,
    runs: &Path,
    minizinc: Option<&(String, String)>,
) -> bool {
    println!("== {slug} ({})", problem.name());
    let model = to_minizinc(problem);
    let model_path = fixtures.join(format!("{slug}.mzn"));
    fs::write(&model_path, &model).expect("model writes");
    println!("  emitted  {}", model_path.display());

    let reference = solve_exact(problem);
    let reference_record = outcome_record(problem, &reference);
    println!(
        "  reference: {:?} ({} nodes)",
        reference_record.status, reference_record.nodes
    );
    let fingerprint_hex = format!("{:016x}", problem.fingerprint());
    write_manifest(
        runs,
        &format!("{slug}.griff-lab-exact"),
        &RunManifest {
            schema: SCHEMA.to_string(),
            version: SCHEMA_VERSION,
            problem: problem.name().to_string(),
            fingerprint_hex: fingerprint_hex.clone(),
            solver: SolverIdentity {
                name: "griff-lab-exact".to_string(),
                version: "1".to_string(),
            },
            outcome: reference_record.clone(),
        },
    );

    external_cross_check(
        slug,
        problem,
        &model_path,
        runs,
        minizinc,
        &fingerprint_hex,
        reference_record.status,
    )
}

/// Removes a stale external manifest, if present.
fn remove_stale(external_manifest: &Path) {
    if external_manifest.exists() {
        fs::remove_file(external_manifest).expect("stale external manifest removed");
        println!("  removed stale {}", external_manifest.display());
    }
}

/// The external half of a fixture run. Stale-evidence rule: an external
/// manifest may exist on disk only when *this* run produced it —
/// reference-only runs and failed external runs remove it, so the archive
/// can never pair fresh reference evidence with a previous run's external
/// verdict.
#[allow(clippy::too_many_arguments)] // a linear pipeline stage, not an API
fn external_cross_check(
    slug: &str,
    problem: &OracleProblem,
    model_path: &Path,
    runs: &Path,
    minizinc: Option<&(String, String)>,
    fingerprint_hex: &str,
    reference_status: Status,
) -> bool {
    let external_manifest = runs.join(format!("{slug}.minizinc-chuffed.json"));
    let Some((bin, version)) = minizinc else {
        remove_stale(&external_manifest);
        return true;
    };
    match run_minizinc(bin, model_path) {
        Ok(external_record) => {
            println!("  minizinc:  {:?} ({version})", external_record.status);
            // Status agreement alone would trust an unvalidated external
            // witness; re-check it against the IR so this is a model
            // oracle, not a status oracle.
            let witness_valid = match &external_record.witness {
                Some(pairs) => {
                    let by_name: std::collections::HashMap<&str, i64> = pairs
                        .iter()
                        .map(|(name, value)| (name.as_str(), *value))
                        .collect();
                    let ordered: Option<Vec<i64>> = problem
                        .vars()
                        .iter()
                        .map(|v| by_name.get(v.name.as_str()).copied())
                        .collect();
                    ordered.is_some_and(|w| verify_witness(problem, &w))
                }
                None => true, // UNSAT carries no witness to validate
            };
            if !witness_valid {
                eprintln!("  external witness violates the IR on {slug}");
            }
            let agree = witness_valid && external_record.status == reference_status;
            write_manifest(
                runs,
                &format!("{slug}.minizinc-chuffed"),
                &RunManifest {
                    schema: SCHEMA.to_string(),
                    version: SCHEMA_VERSION,
                    problem: problem.name().to_string(),
                    fingerprint_hex: fingerprint_hex.to_string(),
                    solver: SolverIdentity {
                        name: "minizinc/chuffed".to_string(),
                        version: version.clone(),
                    },
                    outcome: external_record,
                },
            );
            if !agree {
                eprintln!("  DISAGREEMENT on {slug}");
            }
            agree
        }
        Err(e) => {
            eprintln!("  minizinc failed on {slug}: {e}");
            remove_stale(&external_manifest);
            false
        }
    }
}

fn main() {
    let fixtures = Path::new("fixtures");
    let runs = Path::new("runs");
    fs::create_dir_all(fixtures).expect("fixtures dir");
    fs::create_dir_all(runs).expect("runs dir");

    let minizinc = minizinc_bin().map(|bin| {
        let version = minizinc_version(&bin);
        println!("external solver: {bin} ({version})");
        (bin, version)
    });
    if minizinc.is_none() {
        println!("external solver: none found (reference solver only)");
    }

    let mut all_agree = true;
    let line = fixture_line();
    let tuning = Tuning::standard_e();

    // Problem A: sweep the travel bound and report the SAT frontier.
    let mut frontier: Option<u8> = None;
    for bound in 0..=24u8 {
        let problem = bounded_travel_problem(&line, &tuning, STANDARD_MAX_FRET, bound)
            .expect("fixture line is playable");
        if matches!(solve_exact(&problem), Outcome::Sat { .. }) {
            frontier = Some(bound);
            break;
        }
    }
    let frontier = frontier.expect("bound 24 always admits the line");
    // A frontier at bound 0 would make the "frontier-unsat" fixture SAT
    // under an UNSAT slug — refuse to archive a lie about the fixture line.
    assert!(
        frontier > 0,
        "fixture line is SAT at bound 0; there is no UNSAT frontier to archive"
    );
    println!(
        "Problem A frontier: bound {} UNSAT, bound {} SAT",
        frontier.saturating_sub(1),
        frontier
    );

    for (slug, bound) in [
        ("bounded-travel-frontier-unsat", frontier.saturating_sub(1)),
        ("bounded-travel-frontier-sat", frontier),
        ("bounded-travel-loose", 24),
    ] {
        let problem = bounded_travel_problem(&line, &tuning, STANDARD_MAX_FRET, bound)
            .expect("fixture line is playable");
        all_agree &= run_fixture(slug, &problem, fixtures, runs, minizinc.as_ref());
    }

    // Problem B: one SAT and one mud-forced UNSAT fixture.
    let sat_problem = pair_cleanliness_problem(&fixture_pair_sat()).expect("SAT fixture builds");
    all_agree &= run_fixture(
        "pair-cleanliness-sat",
        &sat_problem,
        fixtures,
        runs,
        minizinc.as_ref(),
    );

    let mud_problem = pair_cleanliness_problem(&fixture_pair_mud()).expect("mud fixture builds");
    all_agree &= run_fixture(
        "pair-cleanliness-mud-unsat",
        &mud_problem,
        fixtures,
        runs,
        minizinc.as_ref(),
    );

    if !all_agree {
        eprintln!("solver disagreement or failure — see above");
        std::process::exit(1);
    }
    if minizinc.is_some() {
        println!("all fixtures: solvers agree");
    } else {
        println!(
            "reference-only run: no external solver, no agreement claim; \
             any stale external manifests were removed"
        );
    }
}
