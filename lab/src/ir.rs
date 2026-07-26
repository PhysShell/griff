//! Solver-neutral typed IR for the oracle spike.
//!
//! Deliberately tiny: exactly the constraint vocabulary Problems A and B need
//! (`docs/proposals/constraint-inventory.md`). Every domain is finite, sorted,
//! and deduplicated at construction, so a problem value has one canonical form
//! and a stable fingerprint.

use serde::Serialize;

/// Index of a variable within its [`OracleProblem`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub struct VarId(pub usize);

/// A finite-domain integer variable. The domain is sorted ascending and
/// deduplicated by [`IntVar::new`] — the canonical form the fingerprint and
/// the emitters rely on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IntVar {
    /// Emission name (also the witness key).
    pub name: String,
    /// Canonical (sorted, deduplicated) finite domain.
    pub domain: Vec<i64>,
}

impl IntVar {
    /// Builds a variable, canonicalizing the domain (sort + dedup).
    #[must_use]
    pub fn new(name: impl Into<String>, mut domain: Vec<i64>) -> Self {
        domain.sort_unstable();
        domain.dedup();
        Self {
            name: name.into(),
            domain,
        }
    }
}

/// The spike's constraint vocabulary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum Constraint {
    /// `|a - b| <= bound` — the experimental travel bound (Problem A).
    AbsDiffLe {
        /// First variable.
        a: VarId,
        /// Second variable.
        b: VarId,
        /// Inclusive bound.
        bound: i64,
    },
    /// `(|var - fixed|) mod 12 ∉ classes` — the coincident-dissonance law
    /// (`DISSONANT_CLASSES = [1, 6, 11]` in production).
    ForbiddenIntervalClasses {
        /// The constrained variable.
        var: VarId,
        /// The fixed counterpart pitch.
        fixed: i64,
        /// Forbidden interval classes mod 12.
        classes: Vec<i64>,
    },
    /// The register-mud law over whole bands: with the variable band being
    /// the min/max over `vars` and the fixed band `[fixed_lo, fixed_hi]`,
    /// overlap relative to the **narrower** band must stay `<= num/den`.
    /// Degenerate rule (either band a single point): the intersection must be
    /// empty — mirroring production's `band_overlap`.
    BandOverlapAtMost {
        /// Variables whose assignment forms the variable band.
        vars: Vec<VarId>,
        /// Fixed band low bound.
        fixed_lo: i64,
        /// Fixed band high bound.
        fixed_hi: i64,
        /// Threshold numerator.
        num: i64,
        /// Threshold denominator.
        den: i64,
    },
}

/// A complete oracle problem: named, finite, canonical.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OracleProblem {
    /// Problem name (manifest and emission header identity).
    pub name: String,
    /// Variables, in emission order.
    pub vars: Vec<IntVar>,
    /// Constraints, in emission order.
    pub constraints: Vec<Constraint>,
}

impl OracleProblem {
    /// Builds a problem from already-canonical parts.
    #[must_use]
    pub fn new(name: impl Into<String>, vars: Vec<IntVar>, constraints: Vec<Constraint>) -> Self {
        Self {
            name: name.into(),
            vars,
            constraints,
        }
    }

    /// FNV-1a 64 over the canonical serialization: stable across runs,
    /// sensitive to any content change. A within-context identity for
    /// manifests and emission headers — not a durable content address.
    #[must_use]
    pub fn fingerprint(&self) -> u64 {
        let canonical =
            serde_json::to_string(self).unwrap_or_else(|_| format!("unserializable:{}", self.name));
        fnv1a64(canonical.as_bytes())
    }
}

/// FNV-1a 64-bit.
fn fnv1a64(bytes: &[u8]) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    bytes
        .iter()
        .fold(OFFSET, |acc, &b| (acc ^ u64::from(b)).wrapping_mul(PRIME))
}
