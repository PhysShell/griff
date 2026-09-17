//! Offline Constraint Lab spike — the first increment of the
//! hard-constraint-contract proposal (`docs/proposals/hard-constraint-contract.md`),
//! modelling the two problems selected by the Constraint Inventory
//! (`docs/proposals/constraint-inventory.md`):
//!
//! - **Problem A** — bounded-travel fretboard realization (an *experimental*
//!   rule: calibration evidence only, no authority over production);
//! - **Problem B** — complement pair cleanliness (the existing
//!   `PairValidation` rule set, pinned to production semantics).
//!
//! The optimization phase ([`optir`], [`fingering`]) adds an objective to
//! the IR and measures the production fingering DP and a hand-position model
//! against an external optimum and against human tablature; [`ties`]
//! analyses the optimum set exactly and learns a secondary tie-break.
//!
//! Shape: typed problem → solver-neutral IR → `MiniZinc` emission + an exact
//! in-repo reference solver → archived manifests. Research tooling only:
//! nothing here is a production dependency, and no production path calls it.

pub mod emit;
pub mod fingering;
pub mod ir;
pub mod manifest;
pub mod optir;
pub mod problems;
pub mod solve;
pub mod technique;
pub mod ties;
