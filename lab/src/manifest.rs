//! Archived run manifests: exact inputs, solver identity, outcome.
//!
//! Every oracle run — reference solver or external `MiniZinc` — records the
//! problem identity (name + fingerprint), the solver identity, and the typed
//! outcome, per the Lab manifest discipline in the hard-constraint-contract
//! proposal.

use serde::{Deserialize, Serialize};

/// Which solver produced an outcome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SolverIdentity {
    /// Solver name (e.g. `griff-lab-exact`, `minizinc/gecode`).
    pub name: String,
    /// Solver version string.
    pub version: String,
}

/// The recorded outcome of one run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutcomeRecord {
    /// `"sat"` or `"unsat"`.
    pub status: String,
    /// Witness as `(variable name, value)` pairs when SAT.
    pub witness: Option<Vec<(String, i64)>>,
    /// Search nodes explored (0 when the solver does not report it).
    pub nodes: u64,
}

/// One archived oracle run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunManifest {
    /// Schema identity: `griff.constraint-lab-run`.
    pub schema: String,
    /// Schema version.
    pub version: u32,
    /// Problem name.
    pub problem: String,
    /// Problem fingerprint, zero-padded hex.
    pub fingerprint_hex: String,
    /// The solver that produced this outcome.
    pub solver: SolverIdentity,
    /// The outcome.
    pub outcome: OutcomeRecord,
}
