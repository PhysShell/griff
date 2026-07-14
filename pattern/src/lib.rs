//! Pure structural pattern algebra — the `griff-pattern` core (ADR-0029 §2).
//!
//! A [`Kernel`] is a rectangular grid of active/inactive cells with **no
//! implicit musical meaning**: rows are not voices, columns are not time.
//! [`fractalize`] expands active cells into scaled kernel copies and inactive
//! cells into empty subtrees, bounded by a required [`ExpansionBudget`] and
//! optionally thinned by the deterministic, path-addressed
//! [`swang-prune-hash-v1`](prune_hash_v1) test. [`linearize`] assigns meaning's
//! first half — an explicit [`Traversal`] — producing an [`ActivitySequence`]
//! that preserves every cell; the second half (time) belongs to `griff-swang`.
//!
//! This crate is **std-only by contract**: no serde, no `griff-core`, no time
//! types, no floats, no platform-sized integers in hashed state. The normative
//! semantics live in `docs/swang/spec.md`; the golden vectors in this module's
//! tests were computed by an independent implementation of that spec.

use std::error::Error;
use std::fmt;

/// `u64::from_le_bytes(*b"swangpr1")` — the domain separator of
/// `swang-prune-hash-v1` (spec §1.8).
const DOMAIN: u64 = u64::from_le_bytes(*b"swangpr1");

/// The golden gamma, folded onto each child index before mixing.
const GAMMA: u64 = 0x9e37_79b9_7f4a_7c15;

/// Basis points in a whole: densities live in `0..=BPS_SCALE`.
const BPS_SCALE: u16 = 10_000;

/// The splitmix64 finalizer (Stafford Mix13) — the mixer of
/// `swang-prune-hash-v1`. Non-cryptographic on purpose.
const fn mix64(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// Flat index of (`row`, `col`) in a `width`-wide grid, `None` on overflow.
fn cell_index(width: usize, row: usize, col: usize) -> Option<usize> {
    row.checked_mul(width)?.checked_add(col)
}

/// A rectangular occupancy grid: the structural seed of an expansion.
///
/// `X` marks an active cell, `.` an inactive one; nothing here is a note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Kernel {
    width: usize,
    height: usize,
    cells: Vec<bool>,
}

impl Kernel {
    /// Parses a kernel from rows of `X` / `.` characters.
    ///
    /// # Errors
    /// [`PatternError::EmptyKernel`] for no rows or an empty first row,
    /// [`PatternError::RaggedKernel`] when row lengths differ, and
    /// [`PatternError::InvalidCell`] for any character other than `X` / `.`.
    pub fn from_rows(rows: &[&str]) -> Result<Self, PatternError> {
        let width = rows.first().map_or(0, |first| first.chars().count());
        if width == 0 {
            return Err(PatternError::EmptyKernel);
        }
        let mut cells = Vec::with_capacity(rows.len().saturating_mul(width));
        for (row, text) in rows.iter().enumerate() {
            let mut got = 0_usize;
            for (col, cell) in text.chars().enumerate() {
                match cell {
                    'X' => cells.push(true),
                    '.' => cells.push(false),
                    other => {
                        return Err(PatternError::InvalidCell {
                            row,
                            col,
                            cell: other,
                        })
                    }
                }
                got = got.saturating_add(1);
            }
            if got != width {
                return Err(PatternError::RaggedKernel {
                    row,
                    expected: width,
                    got,
                });
            }
        }
        Ok(Self {
            width,
            height: rows.len(),
            cells,
        })
    }

    /// Grid width in cells.
    #[must_use]
    pub const fn width(&self) -> usize {
        self.width
    }

    /// Grid height in cells.
    #[must_use]
    pub const fn height(&self) -> usize {
        self.height
    }

    /// Whether the cell at (`row`, `col`) is active; out of range is inactive.
    #[must_use]
    pub fn is_active(&self, row: usize, col: usize) -> bool {
        if col >= self.width {
            return false;
        }
        cell_index(self.width, row, col)
            .and_then(|index| self.cells.get(index))
            .copied()
            .unwrap_or(false)
    }

    /// How many cells are active.
    #[must_use]
    pub fn active_count(&self) -> usize {
        self.cells.iter().filter(|&&cell| cell).count()
    }
}

/// A node's address in the expansion tree: child indices from the root, each
/// in **structural order** (`row × kernel_width + column`), independent of
/// any traversal.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NodePath(Vec<u32>);

impl NodePath {
    /// The child indices from the root.
    #[must_use]
    pub fn as_slice(&self) -> &[u32] {
        &self.0
    }
}

impl From<Vec<u32>> for NodePath {
    fn from(indices: Vec<u32>) -> Self {
        Self(indices)
    }
}

/// Structural expansion limits. Required — the library ships no defaults;
/// frontends document their own (ADR-0029 §6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExpansionBudget {
    /// The deepest expansion level allowed.
    pub max_depth: u8,
    /// The most grid cells (active or not) an expansion may materialize.
    pub max_cells: u64,
}

/// Density decay in basis points, `0..=10000`. Floats never appear.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DensityBps(u16);

impl DensityBps {
    /// Validates the basis-point range.
    ///
    /// # Errors
    /// [`PatternError::InvalidDensity`] above 10000.
    pub const fn new(bps: u16) -> Result<Self, PatternError> {
        if bps > BPS_SCALE {
            return Err(PatternError::InvalidDensity { bps });
        }
        Ok(Self(bps))
    }

    /// The raw basis points.
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }
}

/// Deterministic pruning: an explicit seed and a density, nothing ambient.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PruneSpec {
    /// The rhythm seed — independent of any generation seed by law.
    pub seed: u64,
    /// Survival density per expansion level.
    pub density: DensityBps,
}

/// A materialized expansion: the level-`depth` occupancy grid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expansion {
    width: usize,
    height: usize,
    depth: u8,
    cells: Vec<bool>,
}

impl Expansion {
    /// Grid width in cells — `kernel_width ^ (depth + 1)`, since depth 0 is
    /// the kernel itself.
    #[must_use]
    pub const fn width(&self) -> usize {
        self.width
    }

    /// Grid height in cells — `kernel_height ^ (depth + 1)`.
    #[must_use]
    pub const fn height(&self) -> usize {
        self.height
    }

    /// The expansion level this grid materializes.
    #[must_use]
    pub const fn depth(&self) -> u8 {
        self.depth
    }

    /// Whether the cell at (`row`, `col`) is active; out of range is inactive.
    #[must_use]
    pub fn is_active(&self, row: usize, col: usize) -> bool {
        if col >= self.width {
            return false;
        }
        cell_index(self.width, row, col)
            .and_then(|index| self.cells.get(index))
            .copied()
            .unwrap_or(false)
    }

    /// How many cells are active.
    #[must_use]
    pub fn active_count(&self) -> usize {
        self.cells.iter().filter(|&&cell| cell).count()
    }
}

/// Expands `kernel` to `depth`.
///
/// Active cells become scaled kernel copies, inactive cells become empty
/// blocks, and pruning (when given) removes whole subtrees by the
/// path-addressed hash test of `docs/swang/spec.md` §1.8. Pruning applies to
/// levels `1..=depth`; the kernel's own cells are given. A pruned parent
/// yields an entirely empty subtree.
///
/// # Errors
/// [`PatternError::MaxDepthExceeded`] when `depth > budget.max_depth`, and
/// [`PatternError::MaxCellsExceeded`] — **before any grid allocation** — when
/// the level-`depth` grid would exceed `budget.max_cells`.
pub fn fractalize(
    kernel: &Kernel,
    depth: u8,
    prune: Option<PruneSpec>,
    budget: ExpansionBudget,
) -> Result<Expansion, PatternError> {
    if depth > budget.max_depth {
        return Err(PatternError::MaxDepthExceeded {
            depth,
            max_depth: budget.max_depth,
        });
    }

    // The whole-grid cell count, checked in u128 before anything allocates.
    // Depth 0 is the kernel itself, so a depth-d grid carries d + 1 factors.
    let levels = u32::from(depth).saturating_add(1);
    let per_level = u128::try_from(kernel.width)
        .ok()
        .zip(u128::try_from(kernel.height).ok())
        .and_then(|(w, h)| w.checked_mul(h))
        .unwrap_or(u128::MAX);
    let needed = per_level.checked_pow(levels).unwrap_or(u128::MAX);
    let over_budget = || PatternError::MaxCellsExceeded {
        path: NodePath::default(),
        needed: u64::try_from(needed).unwrap_or(u64::MAX),
        max_cells: budget.max_cells,
    };
    if needed > u128::from(budget.max_cells) {
        return Err(over_budget());
    }

    let out_width = kernel.width.checked_pow(levels).ok_or_else(over_budget)?;
    let out_height = kernel.height.checked_pow(levels).ok_or_else(over_budget)?;
    let total = out_width.checked_mul(out_height).ok_or_else(over_budget)?;

    // Row and column strides, most-significant digit first: digit j of a
    // coordinate selects the kernel position chosen at substitution level j.
    let strides = |base: usize| -> Vec<usize> {
        (0..=u32::from(depth))
            .rev()
            .map(|level| base.checked_pow(level).unwrap_or(usize::MAX))
            .collect()
    };
    let row_strides = strides(kernel.height);
    let col_strides = strides(kernel.width);

    let expander = Expander {
        kernel,
        depth,
        row_strides,
        col_strides,
        keyed: prune.map(|spec| (mix64(DOMAIN ^ spec.seed), prune_threshold(spec.density))),
    };

    let mut cells = Vec::with_capacity(total);
    for row in 0..out_height {
        for col in 0..out_width {
            cells.push(expander.cell_survives(row, col));
        }
    }

    Ok(Expansion {
        width: out_width,
        height: out_height,
        depth,
        cells,
    })
}

/// The per-cell context of one expansion pass: the kernel, the digit strides,
/// and (when pruning) the pre-mixed root key with the survival threshold
/// (`None` = density 10000, keep all).
struct Expander<'a> {
    kernel: &'a Kernel,
    depth: u8,
    row_strides: Vec<usize>,
    col_strides: Vec<usize>,
    keyed: Option<(u64, Option<u64>)>,
}

impl Expander<'_> {
    /// Whether one grid cell of the level-`depth` expansion is active.
    ///
    /// Every coordinate digit must land on an active kernel cell, and (when
    /// pruning) every proper path prefix — the levels `1..=depth` — must
    /// pass the hash test of spec §1.8.
    #[allow(
        clippy::arithmetic_side_effects,
        // The strides are >= 1 by construction (checked_pow of a non-zero
        // base, saturating at usize::MAX) and the kernel dimensions are
        // non-zero, so the digit division and modulo cannot panic.
        reason = "division and modulo by construction-guaranteed non-zero values"
    )]
    fn cell_survives(&self, row: usize, col: usize) -> bool {
        let mut key = self.keyed.map(|(root, _)| root);
        for level in 0..=usize::from(self.depth) {
            let row_stride = self.row_strides.get(level).copied().unwrap_or(1).max(1);
            let col_stride = self.col_strides.get(level).copied().unwrap_or(1).max(1);
            let kernel_row = (row / row_stride) % self.kernel.height.max(1);
            let kernel_col = (col / col_stride) % self.kernel.width.max(1);
            if !self.kernel.is_active(kernel_row, kernel_col) {
                return false;
            }
            // Digits 0..depth (all but the last) are the path prefixes of
            // the levels 1..=depth — the ones the spec prunes.
            if level < usize::from(self.depth) {
                if let Some((_, threshold)) = self.keyed {
                    let child = kernel_row
                        .saturating_mul(self.kernel.width)
                        .saturating_add(kernel_col);
                    let folded = mix64(
                        key.unwrap_or_default()
                            ^ u64::try_from(child).unwrap_or(u64::MAX).wrapping_add(GAMMA),
                    );
                    key = Some(folded);
                    if let Some(limit) = threshold {
                        if folded >= limit {
                            return false;
                        }
                    }
                }
            }
        }
        true
    }
}

/// How a two-dimensional expansion becomes a one-dimensional sequence.
/// Always explicit — a grid has no default reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Traversal {
    /// Rows left-to-right, top-to-bottom.
    RowMajor,
    /// Boustrophedon: alternating rows reverse, keeping consecutive cells
    /// edge-adjacent across row boundaries.
    Snake,
}

/// A linearized expansion. Every cell of the grid is preserved: an inactive
/// cell is a future timed rest, never dropped (spec §1.10).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivitySequence(Vec<bool>);

impl ActivitySequence {
    /// Total cells, active and inactive alike.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether the sequence holds no cells at all.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Every cell in traversal order.
    #[must_use]
    pub fn cells(&self) -> &[bool] {
        &self.0
    }

    /// The slot indices of the active cells, ascending.
    #[must_use]
    pub fn onsets(&self) -> Vec<usize> {
        self.0
            .iter()
            .enumerate()
            .filter_map(|(slot, &active)| active.then_some(slot))
            .collect()
    }
}

/// Reads `expansion` in `traversal` order into a sequence that keeps every
/// cell.
#[must_use]
pub fn linearize(expansion: &Expansion, traversal: Traversal) -> ActivitySequence {
    let width = expansion.width.max(1);
    let mut cells = Vec::with_capacity(expansion.cells.len());
    for (row, chunk) in expansion.cells.chunks(width).enumerate() {
        let reversed = matches!(traversal, Traversal::Snake) && row & 1 == 1;
        if reversed {
            cells.extend(chunk.iter().rev().copied());
        } else {
            cells.extend_from_slice(chunk);
        }
    }
    ActivitySequence(cells)
}

/// `swang-prune-hash-v1`: the path-addressed key of spec §1.8.
///
/// A splitmix64 finalizer folded down the tree from `mix64(DOMAIN ^ seed)`,
/// one child index at a time. Deterministic, order-independent,
/// non-cryptographic.
#[must_use]
pub fn prune_hash_v1(seed: u64, path: &[u32]) -> u64 {
    let mut key = mix64(DOMAIN ^ seed);
    for &child in path {
        key = mix64(key ^ u64::from(child).wrapping_add(GAMMA));
    }
    key
}

/// The constant per-node survival threshold: `floor(bps × 2^64 / 10000)`
/// computed in `u128`. `None` means density 10000 — keep everything, no test
/// (2^64 is not representable as a `u64` threshold).
#[must_use]
#[allow(
    clippy::arithmetic_side_effects,
    // bps <= 9999 needs 78 bits after the shift — no u128 overflow — and the
    // divisor is a non-zero constant.
    reason = "exact by construction"
)]
pub fn prune_threshold(density: DensityBps) -> Option<u64> {
    let bps = density.get();
    if bps >= BPS_SCALE {
        return None;
    }
    let scaled = u128::from(bps) << 64_u32;
    u64::try_from(scaled / u128::from(BPS_SCALE)).ok()
}

/// Everything that can go wrong in the pattern core. Every budget breach
/// carries the offending [`NodePath`]; nothing is silently truncated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatternError {
    /// No rows, or a first row with no cells.
    EmptyKernel,
    /// A row's length differs from the first row's.
    RaggedKernel {
        /// The offending row (0-based).
        row: usize,
        /// Cells the first row established.
        expected: usize,
        /// Cells this row actually has.
        got: usize,
    },
    /// A character other than `X` or `.`.
    InvalidCell {
        /// The offending row (0-based).
        row: usize,
        /// The offending column (0-based).
        col: usize,
        /// The character found.
        cell: char,
    },
    /// The requested depth exceeds the budget.
    MaxDepthExceeded {
        /// The depth asked for.
        depth: u8,
        /// The budget's ceiling.
        max_depth: u8,
    },
    /// The level-`depth` grid would exceed the cell budget. Raised before
    /// any allocation.
    MaxCellsExceeded {
        /// The subtree that broke the budget (the root for the up-front
        /// whole-grid check).
        path: NodePath,
        /// Cells the grid would need.
        needed: u64,
        /// The budget's ceiling.
        max_cells: u64,
    },
    /// Density outside `0..=10000` basis points.
    InvalidDensity {
        /// The rejected value.
        bps: u16,
    },
}

impl fmt::Display for PatternError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyKernel => {
                write!(f, "empty kernel: no rows, or no cells in the first row")
            }
            Self::RaggedKernel { row, expected, got } => write!(
                f,
                "ragged kernel: row {row} has {got} cells, expected {expected}"
            ),
            Self::InvalidCell { row, col, cell } => write!(
                f,
                "invalid kernel cell {cell:?} at row {row}, col {col}: only `X` and `.`"
            ),
            Self::MaxDepthExceeded { depth, max_depth } => {
                write!(
                    f,
                    "depth {depth} exceeds the budget's max_depth {max_depth}"
                )
            }
            Self::MaxCellsExceeded {
                path,
                needed,
                max_cells,
            } => write!(
                f,
                "expansion at path {:?} needs {needed} cells, over the budget's {max_cells}",
                path.as_slice()
            ),
            Self::InvalidDensity { bps } => {
                write!(f, "density {bps} bps is outside 0..=10000")
            }
        }
    }
}

impl Error for PatternError {}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::missing_assert_message,
    clippy::arithmetic_side_effects
)]
mod tests {
    use proptest::prelude::*;

    use super::{
        fractalize, linearize, prune_hash_v1, prune_threshold, ActivitySequence, DensityBps,
        ExpansionBudget, Kernel, NodePath, PatternError, PruneSpec, Traversal,
    };

    /// The spec's worked kernel (`docs/swang/spec.md` §1.6/§1.9).
    fn spec_kernel() -> Kernel {
        Kernel::from_rows(&["X.X", "XX.", ".XX"]).expect("the spec kernel parses")
    }

    fn roomy() -> ExpansionBudget {
        ExpansionBudget {
            max_depth: 4,
            max_cells: 10_000,
        }
    }

    fn bps(v: u16) -> DensityBps {
        DensityBps::new(v).expect("valid density")
    }

    // ---- kernel laws (spec §1.6) ----

    #[test]
    fn a_ragged_kernel_is_rejected() {
        let err = Kernel::from_rows(&["X.X", "XX"]).expect_err("ragged must fail");
        assert_eq!(
            err,
            PatternError::RaggedKernel {
                row: 1,
                expected: 3,
                got: 2
            }
        );
    }

    #[test]
    fn an_invalid_cell_character_is_rejected() {
        let err = Kernel::from_rows(&["X.X", "XO."]).expect_err("O is not a cell");
        assert_eq!(
            err,
            PatternError::InvalidCell {
                row: 1,
                col: 1,
                cell: 'O'
            }
        );
    }

    #[test]
    fn an_empty_kernel_is_rejected() {
        assert_eq!(
            Kernel::from_rows(&[]).expect_err("no rows"),
            PatternError::EmptyKernel
        );
        assert_eq!(
            Kernel::from_rows(&[""]).expect_err("no cells"),
            PatternError::EmptyKernel
        );
    }

    // ---- fractalize (spec §1.7) ----

    #[test]
    fn depth_zero_is_the_kernel_itself() {
        let k = spec_kernel();
        let e = fractalize(&k, 0, None, roomy()).expect("depth 0 expands");
        assert_eq!((e.width(), e.height(), e.depth()), (3, 3, 0));
        for row in 0..3 {
            for col in 0..3 {
                assert_eq!(e.is_active(row, col), k.is_active(row, col));
            }
        }
    }

    #[test]
    fn an_active_parent_expands_into_a_kernel_copy() {
        let k = spec_kernel();
        let e = fractalize(&k, 1, None, roomy()).expect("depth 1 expands");
        assert_eq!((e.width(), e.height()), (9, 9));
        // Kernel cell (0,0) is X: its level-1 block is a kernel replica.
        for row in 0..3 {
            for col in 0..3 {
                assert_eq!(e.is_active(row, col), k.is_active(row, col));
            }
        }
    }

    #[test]
    fn an_empty_parent_expands_into_an_empty_block() {
        let k = spec_kernel();
        let e = fractalize(&k, 1, None, roomy()).expect("depth 1 expands");
        // Kernel cell (0,1) is `.`: its whole block stays silent.
        for row in 0..3 {
            for col in 3..6 {
                assert!(!e.is_active(row, col));
            }
        }
    }

    #[test]
    fn the_cell_budget_fires_before_allocation() {
        let k = spec_kernel();
        let tight = ExpansionBudget {
            max_depth: 4,
            max_cells: 80, // depth 1 needs (3·3)^2 = 81
        };
        let err = fractalize(&k, 1, None, tight).expect_err("81 > 80");
        assert_eq!(
            err,
            PatternError::MaxCellsExceeded {
                path: NodePath::default(),
                needed: 81,
                max_cells: 80
            }
        );
    }

    #[test]
    fn the_depth_budget_rejects_a_deeper_ask() {
        let k = spec_kernel();
        let short = ExpansionBudget {
            max_depth: 2,
            max_cells: 10_000,
        };
        let err = fractalize(&k, 3, None, short).expect_err("3 > 2");
        assert_eq!(
            err,
            PatternError::MaxDepthExceeded {
                depth: 3,
                max_depth: 2
            }
        );
    }

    #[test]
    fn expansion_is_deterministic() {
        let k = spec_kernel();
        let prune = PruneSpec {
            seed: 17,
            density: bps(5000),
        };
        let a = fractalize(&k, 2, Some(prune), roomy()).expect("expands");
        let b = fractalize(&k, 2, Some(prune), roomy()).expect("expands");
        assert_eq!(a, b);
    }

    // ---- traversals (spec §1.9 worked example) ----

    #[test]
    fn row_major_matches_the_spec_vector() {
        let e = fractalize(&spec_kernel(), 0, None, roomy()).expect("expands");
        let seq = linearize(&e, Traversal::RowMajor);
        assert_eq!(seq.len(), 9);
        assert_eq!(seq.onsets(), vec![0, 2, 3, 4, 7, 8]);
    }

    #[test]
    fn snake_matches_the_spec_vector() {
        let e = fractalize(&spec_kernel(), 0, None, roomy()).expect("expands");
        let seq = linearize(&e, Traversal::Snake);
        assert_eq!(seq.len(), 9);
        assert_eq!(seq.onsets(), vec![0, 2, 4, 5, 7, 8]);
    }

    #[test]
    fn linearize_preserves_inactive_cells() {
        let e = fractalize(&spec_kernel(), 1, None, roomy()).expect("expands");
        let seq = linearize(&e, Traversal::RowMajor);
        // Every one of the 81 cells is a slot; silence survives.
        assert_eq!(seq.len(), 81);
        assert_eq!(seq.cells().len(), 81);
        assert_eq!(seq.onsets().len(), e.active_count());
    }

    // ---- swang-prune-hash-v1 (spec §1.8) ----
    // Golden vectors computed by an independent implementation of the spec
    // (BigInteger arithmetic, PowerShell) before this crate existed.

    #[test]
    fn the_prune_hash_matches_the_independent_vectors() {
        assert_eq!(prune_hash_v1(17, &[]), 0x1075_e562_a02c_525a);
        assert_eq!(prune_hash_v1(17, &[0]), 0xec2c_beaa_f681_f342);
        assert_eq!(prune_hash_v1(17, &[1]), 0x1ad1_7157_fdd6_f86b);
        assert_eq!(prune_hash_v1(17, &[2]), 0x3a29_3925_dec7_7170);
        assert_eq!(prune_hash_v1(17, &[8]), 0xeaff_4a5c_cbd0_e561);
        assert_eq!(prune_hash_v1(17, &[0, 0]), 0x7e48_fcbc_e55b_2fe7);
        assert_eq!(prune_hash_v1(17, &[2, 5]), 0x4d9e_a7ec_177d_3964);
        assert_eq!(prune_hash_v1(42, &[]), 0x41cf_2b35_1258_b180);
        assert_eq!(prune_hash_v1(0, &[]), 0x175b_69af_1921_411f);
    }

    #[test]
    fn the_threshold_is_exact() {
        assert_eq!(prune_threshold(bps(8000)), Some(14_757_395_258_967_641_292));
        assert_eq!(prune_threshold(bps(5000)), Some(1 << 63));
        assert_eq!(prune_threshold(bps(0)), Some(0));
        assert_eq!(prune_threshold(bps(10000)), None);
    }

    #[test]
    fn density_bps_rejects_out_of_range() {
        assert_eq!(
            DensityBps::new(10_001).expect_err("out of range"),
            PatternError::InvalidDensity { bps: 10_001 }
        );
    }

    #[test]
    fn bps_zero_leaves_nothing_below_the_root() {
        let prune = PruneSpec {
            seed: 17,
            density: bps(0),
        };
        let e = fractalize(&spec_kernel(), 1, Some(prune), roomy()).expect("expands");
        assert_eq!(e.active_count(), 0);
    }

    #[test]
    fn bps_full_equals_no_pruning() {
        let k = spec_kernel();
        let full = PruneSpec {
            seed: 17,
            density: bps(10000),
        };
        let pruned = fractalize(&k, 2, Some(full), roomy()).expect("expands");
        let unpruned = fractalize(&k, 2, None, roomy()).expect("expands");
        assert_eq!(pruned, unpruned);
    }

    #[test]
    fn a_pruned_parent_yields_an_empty_subtree() {
        // At seed 17, bps 5000 (threshold 2^63): child 0 hashes to
        // 0xec2c… ≥ 2^63 — pruned; child 2 hashes to 0x3a29… < 2^63 — kept.
        let k = spec_kernel();
        let prune = PruneSpec {
            seed: 17,
            density: bps(5000),
        };

        let depth1 = fractalize(&k, 1, Some(prune), roomy()).expect("expands");
        // Child 0's block spans rows 0..3 × cols 0..3 — silent throughout.
        for row in 0..3 {
            for col in 0..3 {
                assert!(!depth1.is_active(row, col), "child 0 is pruned");
            }
        }
        // Child 2's block starts at column 6; kernel (0,0) is X, so the
        // block's top-left cell sounds.
        assert!(depth1.is_active(0, 6), "child 2 survives");

        let depth2 = fractalize(&k, 2, Some(prune), roomy()).expect("expands");
        // At depth 2 the grid is 27×27 and child 0's whole subtree is the
        // 9×9 block at the origin.
        for row in 0..9 {
            for col in 0..9 {
                assert!(
                    !depth2.is_active(row, col),
                    "the pruned child 0 must have an entirely empty subtree"
                );
            }
        }
    }

    // ---- properties ----

    /// Random small kernels as row strings: 1..=3 rows of 1..=3 cells.
    fn kernel_rows() -> impl Strategy<Value = Vec<String>> {
        use proptest::bool::ANY;
        use proptest::collection::vec;

        let width = 1..=3_usize;
        width.prop_flat_map(|w| {
            vec(
                vec(ANY, w..=w).prop_map(|cells| {
                    cells
                        .into_iter()
                        .map(|active| if active { 'X' } else { '.' })
                        .collect::<String>()
                }),
                1..=3,
            )
        })
    }

    proptest! {
        #[test]
        fn expansion_never_exceeds_the_declared_grid(rows in kernel_rows(), depth in 0_u8..=2) {
            let refs: Vec<&str> = rows.iter().map(String::as_str).collect();
            let k = Kernel::from_rows(&refs).expect("generated kernels are rectangular");
            let e = fractalize(&k, depth, None, roomy()).expect("roomy budget");
            let cells = e.width() * e.height();
            // depth 0 is the kernel itself, so depth d is d + 1 kernel factors.
            prop_assert_eq!(e.width(), k.width().pow(u32::from(depth) + 1));
            prop_assert_eq!(e.height(), k.height().pow(u32::from(depth) + 1));
            prop_assert!(e.active_count() <= cells);
            prop_assert_eq!(
                linearize(&e, Traversal::Snake).len(),
                cells,
                "linearize preserves every cell"
            );
        }

        #[test]
        fn an_empty_kernel_cell_stays_empty_at_depth_one(rows in kernel_rows()) {
            let refs: Vec<&str> = rows.iter().map(String::as_str).collect();
            let k = Kernel::from_rows(&refs).expect("generated kernels are rectangular");
            let e = fractalize(&k, 1, None, roomy()).expect("roomy budget");
            for row in 0..k.height() {
                for col in 0..k.width() {
                    if !k.is_active(row, col) {
                        for sub_row in 0..k.height() {
                            for sub_col in 0..k.width() {
                                prop_assert!(!e.is_active(
                                    row * k.height() + sub_row,
                                    col * k.width() + sub_col
                                ));
                            }
                        }
                    }
                }
            }
        }
    }

    // Keep the unused-type warning away until green wires it in.
    #[test]
    fn activity_sequence_reports_emptiness() {
        let e = fractalize(&spec_kernel(), 0, None, roomy()).expect("expands");
        let seq: ActivitySequence = linearize(&e, Traversal::RowMajor);
        assert!(!seq.is_empty());
    }
}
