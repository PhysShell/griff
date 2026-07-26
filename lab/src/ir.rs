//! Solver-neutral typed IR for the oracle spike.
//!
//! Deliberately tiny: exactly the constraint vocabulary Problems A and B need
//! (`docs/proposals/constraint-inventory.md`). Every domain is finite, sorted,
//! and deduplicated at construction, so a problem value has one canonical form
//! and a stable fingerprint.

use serde::Serialize;
use thiserror::Error;

/// Typed IR-invariant violations, refused at construction.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum IrError {
    /// A constraint references a variable that does not exist.
    #[error("constraint references dangling VarId {id} (only {vars} vars)")]
    DanglingVarId {
        /// The dangling index.
        id: usize,
        /// Number of declared variables.
        vars: usize,
    },
    /// A variable's domain is empty.
    #[error("variable \"{name}\" has an empty domain")]
    EmptyDomain {
        /// The offending variable name.
        name: String,
    },
    /// Two variables share a name (witness keys must be unique).
    #[error("duplicate variable name \"{name}\"")]
    DuplicateName {
        /// The duplicated name.
        name: String,
    },
    /// A name is not `MiniZinc`-safe (`[A-Za-z_][A-Za-z0-9_]*`).
    #[error("variable name \"{name}\" is not MiniZinc-safe")]
    UnsafeName {
        /// The offending name.
        name: String,
    },
    /// A band threshold denominator is not positive.
    #[error("band threshold denominator {den} is not positive")]
    NonPositiveDenominator {
        /// The offending denominator.
        den: i64,
    },
    /// A band constraint has no member variables (`min([])` is undefined).
    #[error("band constraint has no member variables")]
    EmptyBand {},
    /// A fixed band with `lo > hi` describes nothing.
    #[error("fixed band [{lo}, {hi}] is reversed")]
    ReversedFixedBand {
        /// The claimed low bound.
        lo: i64,
        /// The claimed high bound.
        hi: i64,
    },
}

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

/// A complete oracle problem: named, finite, canonical, **opaque** — the
/// fields are private so the construction-time invariants cannot be broken
/// afterwards; read access goes through the immutable accessors.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OracleProblem {
    name: String,
    vars: Vec<IntVar>,
    constraints: Vec<Constraint>,
}

impl OracleProblem {
    /// Builds a problem, panicking on an IR-invariant violation.
    ///
    /// # Panics
    ///
    /// Panics with the [`IrError`] message when [`OracleProblem::try_new`]
    /// would refuse — the infallible form for statically-known-valid
    /// problems (tests, fixtures).
    #[must_use]
    #[allow(clippy::panic)] // documented: the typed form is try_new
    pub fn new(name: impl Into<String>, vars: Vec<IntVar>, constraints: Vec<Constraint>) -> Self {
        match Self::try_new(name, vars, constraints) {
            Ok(problem) => problem,
            Err(e) => panic!("invalid oracle problem: {e}"),
        }
    }

    /// Builds a problem after validating every IR invariant, so the emitter
    /// and solver never see malformed input.
    ///
    /// # Errors
    ///
    /// See [`IrError`] — dangling `VarId`s, empty domains, duplicate or
    /// MiniZinc-unsafe names, non-positive band denominators.
    pub fn try_new(
        name: impl Into<String>,
        vars: Vec<IntVar>,
        constraints: Vec<Constraint>,
    ) -> Result<Self, IrError> {
        for var in &vars {
            if var.domain.is_empty() {
                return Err(IrError::EmptyDomain {
                    name: var.name.clone(),
                });
            }
            if !minizinc_safe(&var.name) {
                return Err(IrError::UnsafeName {
                    name: var.name.clone(),
                });
            }
        }
        for (i, var) in vars.iter().enumerate() {
            if vars.iter().skip(i + 1).any(|other| other.name == var.name) {
                return Err(IrError::DuplicateName {
                    name: var.name.clone(),
                });
            }
        }
        let check_id = |id: &VarId| -> Result<(), IrError> {
            if id.0 >= vars.len() {
                return Err(IrError::DanglingVarId {
                    id: id.0,
                    vars: vars.len(),
                });
            }
            Ok(())
        };
        for constraint in &constraints {
            match constraint {
                Constraint::AbsDiffLe { a, b, .. } => {
                    check_id(a)?;
                    check_id(b)?;
                }
                Constraint::ForbiddenIntervalClasses { var, .. } => check_id(var)?,
                Constraint::BandOverlapAtMost {
                    vars: band, den, ..
                } => {
                    for id in band {
                        check_id(id)?;
                    }
                    if *den <= 0 {
                        return Err(IrError::NonPositiveDenominator { den: *den });
                    }
                }
            }
        }
        Ok(Self {
            name: name.into(),
            vars,
            constraints,
        })
    }

    /// Problem name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Variables, in emission order.
    #[must_use]
    pub fn vars(&self) -> &[IntVar] {
        &self.vars
    }

    /// Constraints, in emission order.
    #[must_use]
    pub fn constraints(&self) -> &[Constraint] {
        &self.constraints
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

/// `[A-Za-z_][A-Za-z0-9_]*` — safe as a `MiniZinc` identifier.
fn minizinc_safe(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}
