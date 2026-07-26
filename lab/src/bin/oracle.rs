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
use griff_constraint_lab::manifest::{OutcomeRecord, RunManifest, SolverIdentity};
use griff_constraint_lab::problems::{bounded_travel_problem, pair_cleanliness_problem, PairSpec};
use griff_constraint_lab::solve::{solve_exact, Outcome};
use griff_core::event::{Pitch, Tuning};
use griff_core::fretboard::STANDARD_MAX_FRET;

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

/// The Problem B SAT fixture: A in the low-mid register, B constrained to a
/// consonant upper-register domain.
fn fixture_pair_sat() -> PairSpec {
    PairSpec {
        a_line: vec![
            (0, pitch(52)),
            (QUARTER, pitch(55)),
            (2 * QUARTER, pitch(52)),
            (3 * QUARTER, pitch(57)),
        ],
        b_onsets: vec![0, QUARTER, 2 * QUARTER, 3 * QUARTER],
        b_domain: [64u8, 66, 67, 69, 71, 72, 74, 76]
            .iter()
            .map(|&p| pitch(p))
            .collect(),
        tuning: Tuning::standard_e(),
        max_fret: STANDARD_MAX_FRET,
    }
}

/// The Problem B UNSAT fixture: no coincident onsets, but the only available
/// B band sits inside A's — register mud alone forces UNSAT.
fn fixture_pair_mud() -> PairSpec {
    PairSpec {
        a_line: vec![(0, pitch(45)), (QUARTER, pitch(57))],
        b_onsets: vec![2 * QUARTER, 3 * QUARTER],
        b_domain: [48u8, 50, 52].iter().map(|&p| pitch(p)).collect(),
        tuning: Tuning::standard_e(),
        max_fret: STANDARD_MAX_FRET,
    }
}

fn outcome_record(problem: &OracleProblem, outcome: &Outcome) -> OutcomeRecord {
    match outcome {
        Outcome::Sat { witness, nodes } => OutcomeRecord {
            status: "sat".to_string(),
            witness: Some(
                problem
                    .vars
                    .iter()
                    .zip(witness)
                    .map(|(v, &value)| (v.name.clone(), value))
                    .collect(),
            ),
            nodes: *nodes,
        },
        Outcome::Unsat { nodes } => OutcomeRecord {
            status: "unsat".to_string(),
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

fn minizinc_version(bin: &str) -> String {
    Command::new(bin)
        .arg("--version")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.lines().next().map(str::to_string))
        .unwrap_or_else(|| "unknown".to_string())
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
    if stdout.contains("=====UNSATISFIABLE=====") {
        return Ok(OutcomeRecord {
            status: "unsat".to_string(),
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
        status: "sat".to_string(),
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
    println!("== {slug} ({})", problem.name);
    let model = to_minizinc(problem);
    let model_path = fixtures.join(format!("{slug}.mzn"));
    fs::write(&model_path, &model).expect("model writes");
    println!("  emitted  {}", model_path.display());

    let reference = solve_exact(problem);
    let reference_record = outcome_record(problem, &reference);
    println!(
        "  reference: {} ({} nodes)",
        reference_record.status, reference_record.nodes
    );
    let fingerprint_hex = format!("{:016x}", problem.fingerprint());
    write_manifest(
        runs,
        &format!("{slug}.griff-lab-exact"),
        &RunManifest {
            schema: "griff.constraint-lab-run".to_string(),
            version: 1,
            problem: problem.name.clone(),
            fingerprint_hex: fingerprint_hex.clone(),
            solver: SolverIdentity {
                name: "griff-lab-exact".to_string(),
                version: "1".to_string(),
            },
            outcome: reference_record.clone(),
        },
    );

    let Some((bin, version)) = minizinc else {
        return true;
    };
    match run_minizinc(bin, &model_path) {
        Ok(external_record) => {
            println!("  minizinc:  {} ({version})", external_record.status);
            let agree = external_record.status == reference_record.status;
            write_manifest(
                runs,
                &format!("{slug}.minizinc-chuffed"),
                &RunManifest {
                    schema: "griff.constraint-lab-run".to_string(),
                    version: 1,
                    problem: problem.name.clone(),
                    fingerprint_hex,
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

    if all_agree {
        println!("all fixtures: solvers agree");
    } else {
        eprintln!("solver disagreement or failure — see above");
        std::process::exit(1);
    }
}
