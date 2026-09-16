//! Solver-neutral **optimization** IR — the Constraint Lab's second phase.
//!
//! The SAT/UNSAT IR ([`crate::ir`]) answers "does an admissible realization
//! exist?". This IR adds an objective: finite integer variables, binary hard
//! tables, and an integer-weighted sum of cost terms, so an external solver
//! can answer "what does the *best* admissible realization cost?".
//!
//! Authority stays in-repo: every witness — from any solver — is re-scored by
//! [`OptProblem::evaluate`], and [`verify_record`] accepts an optimality claim
//! only when the solver proved it, the witness is admissible, and the
//! re-scored objective equals the claimed one. Research tooling only: nothing
//! here is a production dependency.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::ir::{IntVar, IrError, VarId};
use crate::manifest::SolverIdentity;

/// Wire schema identity of an exported optimization problem.
pub const OPT_SCHEMA: &str = "griff.constraint-lab-opt";
/// Wire schema version of an exported optimization problem.
pub const OPT_SCHEMA_VERSION: u32 = 1;

/// A hard (admissibility) constraint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Hard {
    /// `(a, b)` must equal one of `tuples` (canonical: sorted, deduplicated).
    Allowed {
        /// First variable.
        a: VarId,
        /// Second variable.
        b: VarId,
        /// The admissible value pairs.
        tuples: Vec<(i64, i64)>,
    },
}

/// One integer-weighted objective term; the objective is their sum.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Term {
    /// `cost(var)` from a table; a value absent from the table costs `0`.
    Unary {
        /// The scored variable.
        var: VarId,
        /// `(value, cost)` entries (canonical: sorted by value, unique keys).
        costs: Vec<(i64, i64)>,
    },
    /// `cost(a, b)` from a table; an absent pair costs `0`.
    Pair {
        /// First variable.
        a: VarId,
        /// Second variable.
        b: VarId,
        /// `(value_a, value_b, cost)` entries (canonical: sorted, unique keys).
        costs: Vec<(i64, i64, i64)>,
    },
    /// `weight · |a − b|`.
    AbsDiff {
        /// First variable.
        a: VarId,
        /// Second variable.
        b: VarId,
        /// Integer weight.
        weight: i64,
    },
    /// `weight · [a ≠ b]`.
    NotEqual {
        /// First variable.
        a: VarId,
        /// Second variable.
        b: VarId,
        /// Integer weight.
        weight: i64,
    },
}

/// Typed refusals at problem construction.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum OptIrError {
    /// A shared IR invariant (domains, names, variable ids) is violated.
    #[error(transparent)]
    Ir(#[from] IrError),
    /// A cost table maps one key to two costs — ambiguous, refused.
    #[error("cost table of objective term {term} repeats a key")]
    DuplicateCostKey {
        /// Index of the offending term.
        term: usize,
    },
    /// A hard table admits nothing — an unconditional UNSAT is refused at
    /// construction rather than smuggled to a solver.
    #[error("hard constraint {index} admits no tuple")]
    EmptyAllowedTable {
        /// Index of the offending hard constraint.
        index: usize,
    },
}

/// Why a witness is not an admissible, scorable assignment.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum WitnessError {
    /// Wrong number of values.
    #[error("witness has {got} values, the problem has {expected} variables")]
    Length {
        /// Variables in the problem.
        expected: usize,
        /// Values in the witness.
        got: usize,
    },
    /// A value lies outside its variable's domain.
    #[error("value {value} of \"{name}\" lies outside its domain")]
    OutOfDomain {
        /// The variable name.
        name: String,
        /// The offending value.
        value: i64,
    },
    /// A hard constraint is violated.
    #[error("hard constraint {index} is violated")]
    HardViolated {
        /// Index of the violated hard constraint.
        index: usize,
    },
    /// The objective does not fit `i64`.
    #[error("the objective overflows i64")]
    Overflow,
}

/// A complete optimization problem: named, finite, canonical, **opaque** —
/// construction validates every invariant and canonicalizes every table, so
/// equal problems have equal fingerprints.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OptProblem {
    name: String,
    vars: Vec<IntVar>,
    hard: Vec<Hard>,
    objective: Vec<Term>,
}

impl OptProblem {
    /// Builds a problem after validating every invariant and canonicalizing
    /// every table (sorted; hard tables deduplicated).
    ///
    /// # Errors
    ///
    /// [`OptIrError::Ir`] for dangling variable ids, empty domains, duplicate
    /// or MiniZinc-unsafe names; [`OptIrError::DuplicateCostKey`] for an
    /// ambiguous cost table; [`OptIrError::EmptyAllowedTable`] for a hard
    /// table that admits nothing.
    pub fn try_new(
        name: impl Into<String>,
        vars: Vec<IntVar>,
        hard: Vec<Hard>,
        objective: Vec<Term>,
    ) -> Result<Self, OptIrError> {
        let _ = (name.into(), vars, hard, objective);
        todo!("optimization IR construction — green step")
    }

    /// Problem name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Variables, in declaration order.
    #[must_use]
    pub fn vars(&self) -> &[IntVar] {
        &self.vars
    }

    /// Hard constraints.
    #[must_use]
    pub fn hard(&self) -> &[Hard] {
        &self.hard
    }

    /// Objective terms.
    #[must_use]
    pub fn objective(&self) -> &[Term] {
        &self.objective
    }

    /// FNV-1a 64 over the canonical serialization — the same within-context
    /// identity discipline as [`crate::ir::OracleProblem::fingerprint`].
    #[must_use]
    pub fn fingerprint(&self) -> u64 {
        todo!("optimization IR fingerprint — green step")
    }

    /// Re-scores a complete assignment: full length, every value inside its
    /// domain, every hard constraint satisfied, then the exact objective.
    ///
    /// # Errors
    ///
    /// See [`WitnessError`].
    pub fn evaluate(&self, witness: &[i64]) -> Result<i64, WitnessError> {
        let _ = witness;
        todo!("optimization IR evaluation — green step")
    }
}

/// One exported problem on the wire (one JSON line), consumed by external
/// solver adapters.
#[derive(Debug, Clone, Serialize)]
pub struct ProblemRecord {
    /// Schema identity: [`OPT_SCHEMA`].
    pub schema: &'static str,
    /// Schema version: [`OPT_SCHEMA_VERSION`].
    pub version: u32,
    /// Caller-assigned identity, unique within one export.
    pub id: String,
    /// The problem fingerprint, zero-padded hex.
    pub fingerprint_hex: String,
    /// The problem.
    pub problem: OptProblem,
    /// Optional reference values for the agreement pass: among optimal
    /// assignments, maximize how many of these `(variable, value)` pairs hold.
    /// Never part of the objective.
    pub reference: Vec<(VarId, i64)>,
}

impl ProblemRecord {
    /// Wraps a problem with its identity and fingerprint.
    #[must_use]
    pub fn new(id: impl Into<String>, problem: OptProblem, reference: Vec<(VarId, i64)>) -> Self {
        let _ = (id.into(), problem, reference);
        todo!("problem record — green step")
    }
}

/// A solver's status for one solve, on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SolveStatus {
    /// Optimum proven.
    Optimal,
    /// A feasible assignment without an optimality proof.
    Feasible,
    /// Proven to admit no assignment.
    Infeasible,
    /// No conclusion (limit reached).
    Unknown,
    /// The solver rejected the model.
    ModelInvalid,
}

/// The lexicographic agreement pass: among assignments at the proven
/// optimum, the maximum number of reference pairs that can hold.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgreementRecord {
    /// Status of the agreement maximization.
    pub status: SolveStatus,
    /// Claimed number of matched reference pairs.
    pub matched: Option<u64>,
    /// The witness achieving it.
    pub witness: Option<Vec<i64>>,
}

/// One solver result on the wire (one JSON line).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SolveRecord {
    /// The problem id it answers.
    pub id: String,
    /// The fingerprint the solver read.
    pub fingerprint_hex: String,
    /// Solver identity.
    pub solver: SolverIdentity,
    /// Solve status.
    pub status: SolveStatus,
    /// Claimed objective of the witness.
    pub objective: Option<i64>,
    /// Best proven lower bound.
    pub bound: Option<i64>,
    /// The witness, one value per variable.
    pub witness: Option<Vec<i64>>,
    /// Wall time in microseconds.
    pub wall_us: u64,
    /// Optional agreement pass.
    pub agreement: Option<AgreementRecord>,
}

/// The in-repo judgement of a [`SolveRecord`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Optimality proven by the solver, witness admissible, objective
    /// re-scored exactly.
    Proven {
        /// The verified optimum.
        optimum: i64,
    },
    /// The record answers a different problem.
    FingerprintMismatch,
    /// The solver did not prove optimality.
    NotProven {
        /// The reported status.
        status: SolveStatus,
    },
    /// The solver claims optimality but supplied no witness or objective.
    MissingWitness,
    /// The witness is not admissible.
    WitnessInvalid(WitnessError),
    /// The claimed objective differs from the re-scored one, or the proven
    /// bound differs from the objective.
    ObjectiveMismatch {
        /// Solver-claimed objective.
        claimed: i64,
        /// In-repo re-scored objective.
        rescored: i64,
        /// Solver-claimed bound.
        bound: Option<i64>,
    },
}

/// Judges a solver record against the problem it claims to answer.
#[must_use]
pub fn verify_record(problem: &OptProblem, record: &SolveRecord) -> Verdict {
    let _ = (problem, record);
    todo!("record verification — green step")
}

/// Why an agreement-pass claim is refused.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AgreementError {
    /// The pass was not solved to optimality.
    #[error("agreement pass not proven: {status:?}")]
    NotProven {
        /// Reported status.
        status: SolveStatus,
    },
    /// No witness or count supplied.
    #[error("agreement pass has no witness or count")]
    MissingWitness,
    /// The witness is not admissible.
    #[error("agreement witness invalid: {0}")]
    WitnessInvalid(WitnessError),
    /// The witness does not sit at the proven optimum.
    #[error("agreement witness costs {rescored}, optimum is {optimum}")]
    OffOptimum {
        /// Verified optimum.
        optimum: i64,
        /// Re-scored cost of the agreement witness.
        rescored: i64,
    },
    /// The claimed match count differs from the recount.
    #[error("agreement claims {claimed} matches, recount is {recounted}")]
    CountMismatch {
        /// Claimed matches.
        claimed: u64,
        /// In-repo recount.
        recounted: u64,
    },
}

/// Verifies an agreement pass against a verified optimum and the reference
/// pairs; returns the recounted number of matches.
///
/// # Errors
///
/// See [`AgreementError`].
pub fn verify_agreement(
    problem: &OptProblem,
    reference: &[(VarId, i64)],
    optimum: i64,
    record: &AgreementRecord,
) -> Result<u64, AgreementError> {
    let _ = (problem, reference, optimum, record);
    todo!("agreement verification — green step")
}
