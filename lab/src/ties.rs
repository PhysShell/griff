//! Exact analysis of a fingering objective's **optimum set**, and a learned
//! **secondary objective** that breaks its ties.
//!
//! The optimality-gap audit (`docs/audit/2026-09-fingering-optimality-gap.md`)
//! found the production DP exact but its fitted weights under-discriminative:
//! many fingerings tie at the optimum, and the DP's fixed tie-break keeps far
//! less agreement with tab authors than the optimum set contains. This module
//! measures that set exactly — how many optimal paths, and the least, most and
//! expected agreement with a reference among them — with chain DPs instead of
//! an external solver, and learns a human-blind tie-break over it: primary cost
//! first, a learned secondary cost second, both minimized lexicographically.
//!
//! Research tooling only (lab crate); nothing here is a production dependency.

use griff_core::event::{FretboardPosition, Pitch, Tuning};
use griff_core::fretboard::FingeringWeights;

use crate::fingering::v1_unary;
use crate::problems::LabError;

/// Number of secondary features ([`FEATURE_NAMES`]).
pub const FEATURES: usize = 21;

/// Secondary feature names, in [`Features`] order. Per note: `fret`, `open`,
/// one-hot `string_1` … `string_7` (strings above 7 count as 7). Per
/// transition (Δ = this note − previous note): `fret_distance` |Δfret|,
/// `string_distance` |Δstring|, `string_change` [Δstring ≠ 0], `same_fret`
/// [Δfret = 0, both fretted], `span_over_3` / `span_over_5` [|Δfret| > 3 / 5,
/// both fretted], `open_transition` [either open], `diagonal` [Δstring ≠ 0 and
/// Δfret ≠ 0], `toward_high_string` [Δstring < 0], `fret_up` [Δfret > 0],
/// `box_move` [Δstring and Δfret nonzero with the same sign]. With an anchor
/// ([`Chain::with_anchor`]), per fretted note: `anchor_distance`
/// |fret − anchor|.
pub const FEATURE_NAMES: [&str; FEATURES] = [
    "fret",
    "open",
    "string_1",
    "string_2",
    "string_3",
    "string_4",
    "string_5",
    "string_6",
    "string_7",
    "fret_distance",
    "string_distance",
    "string_change",
    "same_fret",
    "span_over_3",
    "span_over_5",
    "open_transition",
    "diagonal",
    "toward_high_string",
    "fret_up",
    "box_move",
    "anchor_distance",
];

/// A feature vector (or a weight vector over it).
pub type Features = [i64; FEATURES];

/// A chain-structured fingering objective over one line: per note the
/// candidate positions (in [`Tuning::candidates`] order) with unary costs, and
/// a cost for every transition between consecutive candidates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chain {
    positions: Vec<Vec<FretboardPosition>>,
    unary: Vec<Vec<i64>>,
    /// `pairwise[i][a][b]`: candidate `a` of note `i − 1` to candidate `b` of
    /// note `i`; `pairwise[0]` is empty.
    pairwise: Vec<Vec<Vec<i64>>>,
    /// Fret the hand was at before the line, when known.
    anchor: Option<u8>,
}

impl Chain {
    /// The production `v1` objective (as `griff_core::fretboard::infer_positions`
    /// minimizes it) as a chain.
    ///
    /// # Errors
    ///
    /// [`LabError::EmptyLine`] for no pitches; [`LabError::UnpositionablePitch`]
    /// when a pitch has no candidate at or below `max_fret`.
    pub fn v1(
        pitches: &[Pitch],
        tuning: &Tuning,
        weights: &FingeringWeights,
        max_fret: u8,
    ) -> Result<Self, LabError> {
        if pitches.is_empty() {
            return Err(LabError::EmptyLine);
        }
        let mut positions: Vec<Vec<FretboardPosition>> = Vec::with_capacity(pitches.len());
        let mut unary = Vec::with_capacity(pitches.len());
        let mut pairwise = Vec::with_capacity(pitches.len());
        for (index, &pitch) in pitches.iter().enumerate() {
            let candidates = tuning.candidates(pitch, max_fret);
            if candidates.is_empty() {
                return Err(LabError::UnpositionablePitch {
                    index,
                    pitch: pitch.0,
                });
            }
            unary.push(
                candidates
                    .iter()
                    .map(|c| v1_unary(c.fret, weights))
                    .collect(),
            );
            pairwise.push(positions.last().map_or_else(Vec::new, |previous| {
                previous
                    .iter()
                    .map(|a| {
                        candidates
                            .iter()
                            .map(|b| {
                                let shift = weights
                                    .position_shift
                                    .saturating_mul(i64::from(a.fret.abs_diff(b.fret)));
                                let change = if a.string == b.string {
                                    0
                                } else {
                                    weights.string_change
                                };
                                shift.saturating_add(change)
                            })
                            .collect()
                    })
                    .collect()
            }));
            positions.push(candidates);
        }
        Ok(Self {
            positions,
            unary,
            pairwise,
            anchor: None,
        })
    }

    /// The same chain with a hand anchor (e.g. `TabLine::anchor_fret`) for the
    /// `anchor_distance` secondary feature. The primary objective is unchanged.
    #[must_use]
    pub fn with_anchor(self, anchor: Option<u8>) -> Self {
        let _ = anchor;
        todo!("chain anchor — green step")
    }

    /// The hand anchor, when set.
    #[must_use]
    pub const fn anchor(&self) -> Option<u8> {
        self.anchor
    }

    /// Notes in the line.
    #[must_use]
    pub fn len(&self) -> usize {
        self.positions.len()
    }

    /// `true` when the line has no notes (not constructible via [`Chain::v1`]).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.positions.is_empty()
    }

    /// Candidate positions of note `note` (empty when out of range).
    #[must_use]
    pub fn candidates(&self, note: usize) -> &[FretboardPosition] {
        self.positions.get(note).map_or(&[], Vec::as_slice)
    }

    /// Primary cost of a path given as one candidate index per note; `None`
    /// for a ragged path or an out-of-range index.
    #[must_use]
    pub fn cost(&self, path: &[usize]) -> Option<i64> {
        if path.len() != self.len() {
            return None;
        }
        let mut total = 0_i64;
        for (note, &c) in path.iter().enumerate() {
            total = total.saturating_add(*self.unary.get(note)?.get(c)?);
            if let Some(previous) = note.checked_sub(1) {
                let a = *path.get(previous)?;
                total = total.saturating_add(*self.pairwise.get(note)?.get(a)?.get(c)?);
            }
        }
        Some(total)
    }

    /// The positions a path selects; `None` as for [`Chain::cost`].
    #[must_use]
    pub fn positions_of(&self, path: &[usize]) -> Option<Vec<FretboardPosition>> {
        if path.len() != self.len() {
            return None;
        }
        path.iter()
            .enumerate()
            .map(|(note, &c)| self.positions.get(note)?.get(c).copied())
            .collect()
    }
}

/// A path count: exact while it fits `u64`, with its natural logarithm always.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PathCount {
    /// The count, saturated at `u64::MAX`.
    pub exact: u64,
    /// `true` when the true count exceeds `u64::MAX`.
    pub saturated: bool,
    /// Natural logarithm of the true count.
    pub ln: f64,
}

/// Agreement with a reference over the optimum set.
#[derive(Debug, Clone, PartialEq)]
pub struct AgreementRange {
    /// Fewest reference matches of any optimal path.
    pub min: usize,
    /// Most reference matches of any optimal path — the ceiling any
    /// tie-break can reach.
    pub max: usize,
    /// Expected matches when an optimal path is drawn uniformly at random.
    pub expected: f64,
    /// An optimal path attaining `max` (ties: lowest candidate indices, as
    /// the production DP breaks them) — the achievable target for learning.
    pub best_path: Vec<usize>,
}

/// The optimum set of a chain.
#[derive(Debug, Clone, PartialEq)]
pub struct OptimumSet {
    /// The optimal primary cost.
    pub optimum: i64,
    /// How many paths attain it.
    pub count: PathCount,
    /// Agreement with the reference, when one of the chain's length was given.
    pub agreement: Option<AgreementRange>,
}

/// Exactly analyses the optimum set of `chain` (forward/backward counting and
/// lexicographic DPs; no search). `reference` positions are compared per note;
/// a reference of a different length yields `agreement: None`.
#[must_use]
pub fn optimum_set(chain: &Chain, reference: Option<&[FretboardPosition]>) -> OptimumSet {
    let n = chain.len();
    if n == 0 {
        return OptimumSet {
            optimum: 0,
            count: Count::one().into(),
            agreement: reference.filter(|r| r.is_empty()).map(|_| AgreementRange {
                min: 0,
                max: 0,
                expected: 0.0,
                best_path: Vec::new(),
            }),
        };
    }
    let (forward, backward) = cost_tables(chain);
    let last = n - 1;
    let optimum = forward.cost[last].iter().copied().min().unwrap_or(0);
    let mut total = Count::zero();
    for (c, &cost) in forward.cost[last].iter().enumerate() {
        if cost == optimum {
            total = total.add(forward.count[last][c]);
        }
    }
    let solved = Solved {
        forward,
        backward,
        optimum,
        total,
    };
    let agreement = reference
        .filter(|r| r.len() == n)
        .map(|r| agreement_range(chain, &solved, r));
    OptimumSet {
        optimum,
        count: total.into(),
        agreement,
    }
}

/// Reference matches of a path; `None` for a ragged path or reference.
#[must_use]
pub fn path_matches(
    chain: &Chain,
    path: &[usize],
    reference: &[FretboardPosition],
) -> Option<usize> {
    if path.len() != chain.len() || reference.len() != chain.len() {
        return None;
    }
    let positions = chain.positions_of(path)?;
    Some(
        positions
            .iter()
            .zip(reference)
            .filter(|(a, b)| a == b)
            .count(),
    )
}

/// Secondary features of a path, summed over notes and transitions
/// ([`FEATURE_NAMES`]); `None` as for [`Chain::cost`].
#[must_use]
pub fn path_features(chain: &Chain, path: &[usize]) -> Option<Features> {
    let positions = chain.positions_of(path)?;
    let mut total = [0_i64; FEATURES];
    for (note, position) in positions.iter().enumerate() {
        add_features(&mut total, &note_features(*position));
        if let Some(previous) = note.checked_sub(1).and_then(|i| positions.get(i)) {
            add_features(&mut total, &transition_features(*previous, *position));
        }
    }
    Some(total)
}

/// The path minimizing `(primary cost, secondary cost)` lexicographically,
/// secondary cost = `weights · features` (+ `margin` per note that matches the
/// reference when `augment = Some((reference, margin))` — loss-augmented
/// inference: it prefers cheap paths that *disagree*). Remaining ties keep the
/// lowest candidate indices, so zero weights and no augmentation reproduce the
/// production DP's path exactly.
#[must_use]
pub fn lexicographic_path(
    chain: &Chain,
    weights: &Features,
    augment: Option<(&[FretboardPosition], i64)>,
) -> Vec<usize> {
    let reference = augment.filter(|(r, _)| r.len() == chain.len());
    match reference {
        Some((r, margin)) => lexicographic_dp(chain, weights, Some(r), Tiebreak::Augment(margin)),
        None => lexicographic_dp(chain, weights, None, Tiebreak::Plain),
    }
}

/// The latent learning target for one line: among the optimal paths that
/// agree most with `reference`, the one cheapest under the secondary `weights`
/// (remaining ties: lowest candidate indices). With zero weights it is
/// [`AgreementRange::best_path`]. `None` when the reference length differs from
/// the chain's.
#[must_use]
pub fn latent_target(
    chain: &Chain,
    weights: &Features,
    reference: &[FretboardPosition],
) -> Option<Vec<usize>> {
    if reference.len() != chain.len() {
        return None;
    }
    Some(lexicographic_dp(
        chain,
        weights,
        Some(reference),
        Tiebreak::Latent,
    ))
}

/// One training line: its primary chain and the tab author's positions.
#[derive(Debug, Clone)]
pub struct Example {
    /// The primary objective over the line.
    pub chain: Chain,
    /// The tab author's positions, one per note.
    pub human: Vec<FretboardPosition>,
}

/// Perceptron settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PerceptronConfig {
    /// Maximum passes over the examples.
    pub epochs: usize,
    /// Loss augmentation per agreeing note during training (0 = plain
    /// perceptron).
    pub margin: i64,
}

/// A trained secondary objective.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrainedSecondary {
    /// Averaged weights (the running sum of per-example weights — the same
    /// argmin as the average, kept in integers).
    pub weights: Features,
    /// Updates made.
    pub updates: u64,
    /// Epochs run (fewer than configured when an epoch made no update).
    pub epochs: usize,
}

/// Learns secondary weights with an averaged, loss-augmented, latent-target
/// structured perceptron **inside the primary optimum set**. The target per
/// step is the achievable one under the current weights — [`latent_target`]:
/// the cheapest of the most-agreeing optimal paths, not the human path, which
/// is often not primary-optimal. When the (augmented) prediction agrees with
/// the tab author less than the target does, the weights move by
/// `features(prediction) − features(target)`. Deterministic: examples in the
/// given order, integer arithmetic.
#[must_use]
pub fn train_secondary(examples: &[Example], config: &PerceptronConfig) -> TrainedSecondary {
    // The most agreement any optimal path reaches, per example.
    let prepared: Vec<(&Example, usize)> = examples
        .iter()
        .filter_map(|example| {
            let range = optimum_set(&example.chain, Some(&example.human)).agreement?;
            Some((example, range.max))
        })
        .collect();
    let mut weights = [0_i64; FEATURES];
    let mut sum = [0_i64; FEATURES];
    let mut updates = 0_u64;
    let mut epochs = 0;
    while epochs < config.epochs {
        epochs += 1;
        let mut changed = false;
        for (example, most) in &prepared {
            let augment = (config.margin != 0).then_some((example.human.as_slice(), config.margin));
            let predicted = lexicographic_path(&example.chain, &weights, augment);
            let matches = path_matches(&example.chain, &predicted, &example.human).unwrap_or(0);
            if matches < *most {
                let target = latent_target(&example.chain, &weights, &example.human)
                    .and_then(|t| path_features(&example.chain, &t));
                if let (Some(features), Some(target)) =
                    (path_features(&example.chain, &predicted), target)
                {
                    for ((w, f), t) in weights.iter_mut().zip(features).zip(target) {
                        *w = w.saturating_add(f.saturating_sub(t));
                    }
                    updates = updates.saturating_add(1);
                    changed = true;
                }
            }
            for (s, w) in sum.iter_mut().zip(weights) {
                *s = s.saturating_add(w);
            }
        }
        if !changed {
            break;
        }
    }
    TrainedSecondary {
        weights: sum,
        updates,
        epochs,
    }
}

// ── private machinery ─────────────────────────────────────────────────────────

/// What the lexicographic DP does with reference matches.
#[derive(Clone, Copy)]
enum Tiebreak {
    /// Ignore the reference: `(primary, secondary)`.
    Plain,
    /// Add `margin` to the secondary cost per matching note.
    Augment(i64),
    /// `(primary, −matches, secondary)`: the most-agreeing optimal paths first.
    Latent,
}

/// The lexicographic chain DP behind [`lexicographic_path`] and
/// [`latent_target`]: minimizes `(primary, middle, secondary)` with strict
/// comparisons, so ties keep the lowest candidate indices as the production DP
/// does. `reference` must have the chain's length when given.
fn lexicographic_dp(
    chain: &Chain,
    weights: &Features,
    reference: Option<&[FretboardPosition]>,
    mode: Tiebreak,
) -> Vec<usize> {
    let n = chain.len();
    if n == 0 {
        return Vec::new();
    }
    let mut best: Vec<Vec<Lexi>> = Vec::with_capacity(n);
    let mut parent: Vec<Vec<usize>> = Vec::with_capacity(n);
    for note in 0..n {
        let candidates = chain.candidates(note);
        let mut layer = Vec::with_capacity(candidates.len());
        let mut parents = Vec::with_capacity(candidates.len());
        for (c, position) in candidates.iter().enumerate() {
            let unary = chain
                .unary
                .get(note)
                .and_then(|u| u.get(c))
                .copied()
                .unwrap_or(0);
            let matched = reference.is_some_and(|r| r.get(note) == Some(position));
            let mut middle = 0_i64;
            let mut secondary = dot(weights, &note_features(*position));
            match mode {
                Tiebreak::Plain => {}
                Tiebreak::Augment(margin) => {
                    if matched {
                        secondary = secondary.saturating_add(i128::from(margin));
                    }
                }
                Tiebreak::Latent => middle = -i64::from(matched),
            }
            let (from, parent_index) = match note.checked_sub(1) {
                None => ((0_i64, 0_i64, 0_i128), 0),
                Some(previous) => {
                    let mut chosen: Option<(Lexi, usize)> = None;
                    for (a, (prev_value, prev_position)) in best[previous]
                        .iter()
                        .zip(chain.candidates(previous))
                        .enumerate()
                    {
                        let transition = chain
                            .pairwise
                            .get(note)
                            .and_then(|p| p.get(a))
                            .and_then(|p| p.get(c))
                            .copied()
                            .unwrap_or(0);
                        let value = (
                            prev_value.0.saturating_add(transition),
                            prev_value.1,
                            prev_value.2.saturating_add(dot(
                                weights,
                                &transition_features(*prev_position, *position),
                            )),
                        );
                        if chosen.is_none_or(|(v, _)| value < v) {
                            chosen = Some((value, a));
                        }
                    }
                    chosen.unwrap_or(((0, 0, 0), 0))
                }
            };
            layer.push((
                from.0.saturating_add(unary),
                from.1.saturating_add(middle),
                from.2.saturating_add(secondary),
            ));
            parents.push(parent_index);
        }
        best.push(layer);
        parent.push(parents);
    }
    let mut c = 0;
    for (index, value) in best[n - 1].iter().enumerate() {
        if *value < best[n - 1][c] {
            c = index;
        }
    }
    let mut path = vec![0; n];
    for note in (0..n).rev() {
        path[note] = c;
        c = parent[note][c];
    }
    path
}

/// A path count carried through the DPs: exact (saturating) and in logs.
#[derive(Debug, Clone, Copy)]
struct Count {
    exact: u64,
    saturated: bool,
    ln: f64,
}

impl Count {
    const fn zero() -> Self {
        Self {
            exact: 0,
            saturated: false,
            ln: f64::NEG_INFINITY,
        }
    }

    const fn one() -> Self {
        Self {
            exact: 1,
            saturated: false,
            ln: 0.0,
        }
    }

    fn add(self, other: Self) -> Self {
        let (exact, overflow) = self.exact.overflowing_add(other.exact);
        Self {
            exact: if overflow { u64::MAX } else { exact },
            saturated: self.saturated || other.saturated || overflow,
            ln: log_add(self.ln, other.ln),
        }
    }

    fn mul(self, other: Self) -> Self {
        let product = self.exact.checked_mul(other.exact);
        Self {
            exact: product.unwrap_or(u64::MAX),
            saturated: self.saturated || other.saturated || product.is_none(),
            ln: self.ln + other.ln,
        }
    }
}

impl From<Count> for PathCount {
    fn from(c: Count) -> Self {
        Self {
            exact: c.exact,
            saturated: c.saturated,
            ln: c.ln,
        }
    }
}

/// `ln(e^a + e^b)` without overflow.
fn log_add(a: f64, b: f64) -> f64 {
    if a == f64::NEG_INFINITY {
        return b;
    }
    if b == f64::NEG_INFINITY {
        return a;
    }
    let (hi, lo) = if a >= b { (a, b) } else { (b, a) };
    hi + (lo - hi).exp().ln_1p()
}

/// A lexicographic `(primary, middle, secondary)` cost; the middle term is
/// `−matches` for [`latent_target`] and 0 otherwise.
type Lexi = (i64, i64, i128);

/// Most reference matches reaching a candidate along optimal edges, with the
/// predecessor attaining it; `None` off the optimum.
type MostMatches = Option<(usize, usize)>;

/// The solved cost tables of a chain with its optimum and optimal-path count.
struct Solved {
    forward: Table,
    backward: Table,
    optimum: i64,
    total: Count,
}

/// Per note, per candidate: least cost and how many (sub)paths attain it.
struct Table {
    cost: Vec<Vec<i64>>,
    count: Vec<Vec<Count>>,
}

/// Forward table (prefix ending at a candidate, its unary included) and
/// backward table (suffix after a candidate, its unary excluded).
fn cost_tables(chain: &Chain) -> (Table, Table) {
    let n = chain.len();
    let mut forward = Table {
        cost: Vec::with_capacity(n),
        count: Vec::with_capacity(n),
    };
    for note in 0..n {
        let k = chain.candidates(note).len();
        let mut costs = Vec::with_capacity(k);
        let mut counts = Vec::with_capacity(k);
        for c in 0..k {
            let unary = chain.unary[note][c];
            if note == 0 {
                costs.push(unary);
                counts.push(Count::one());
                continue;
            }
            let mut least = i64::MAX;
            let mut count = Count::zero();
            for a in 0..chain.candidates(note - 1).len() {
                let value = forward.cost[note - 1][a].saturating_add(chain.pairwise[note][a][c]);
                if value < least {
                    least = value;
                    count = forward.count[note - 1][a];
                } else if value == least {
                    count = count.add(forward.count[note - 1][a]);
                }
            }
            costs.push(least.saturating_add(unary));
            counts.push(count);
        }
        forward.cost.push(costs);
        forward.count.push(counts);
    }

    let mut backward = Table {
        cost: vec![Vec::new(); n],
        count: vec![Vec::new(); n],
    };
    for note in (0..n).rev() {
        let k = chain.candidates(note).len();
        if note + 1 == n {
            backward.cost[note] = vec![0; k];
            backward.count[note] = vec![Count::one(); k];
            continue;
        }
        let mut costs = Vec::with_capacity(k);
        let mut counts = Vec::with_capacity(k);
        for a in 0..k {
            let mut least = i64::MAX;
            let mut count = Count::zero();
            for b in 0..chain.candidates(note + 1).len() {
                let value = chain.pairwise[note + 1][a][b]
                    .saturating_add(chain.unary[note + 1][b])
                    .saturating_add(backward.cost[note + 1][b]);
                if value < least {
                    least = value;
                    count = backward.count[note + 1][b];
                } else if value == least {
                    count = count.add(backward.count[note + 1][b]);
                }
            }
            costs.push(least);
            counts.push(count);
        }
        backward.cost[note] = costs;
        backward.count[note] = counts;
    }
    (forward, backward)
}

/// Least / most / expected reference agreement over the optimal paths, with a
/// most-agreeing optimal path.
#[allow(clippy::cast_precision_loss)]
fn agreement_range(
    chain: &Chain,
    solved: &Solved,
    reference: &[FretboardPosition],
) -> AgreementRange {
    let Solved {
        forward,
        backward,
        optimum,
        total,
    } = solved;
    let (optimum, total) = (*optimum, *total);
    let n = chain.len();
    let on_optimum = |note: usize, c: usize| {
        forward.cost[note][c].saturating_add(backward.cost[note][c]) == optimum
    };
    let matches =
        |note: usize, c: usize| usize::from(chain.candidates(note).get(c) == reference.get(note));

    // Expected agreement: P(candidate c at note i) = paths through it / total.
    let mut expected = 0.0;
    for note in 0..n {
        for c in 0..chain.candidates(note).len() {
            if matches(note, c) == 1 && on_optimum(note, c) {
                let through = forward.count[note][c].mul(backward.count[note][c]);
                expected += (through.ln - total.ln).exp();
            }
        }
    }

    // Min / max agreement along optimal edges only.
    let mut low: Vec<Vec<Option<usize>>> = Vec::with_capacity(n);
    let mut high: Vec<Vec<MostMatches>> = Vec::with_capacity(n);
    for note in 0..n {
        let k = chain.candidates(note).len();
        let mut low_layer = Vec::with_capacity(k);
        let mut high_layer = Vec::with_capacity(k);
        for c in 0..k {
            if !on_optimum(note, c) {
                low_layer.push(None);
                high_layer.push(None);
                continue;
            }
            let here = matches(note, c);
            if note == 0 {
                low_layer.push(Some(here));
                high_layer.push(Some((here, 0)));
                continue;
            }
            let mut lo: Option<usize> = None;
            let mut hi: MostMatches = None;
            for a in 0..chain.candidates(note - 1).len() {
                let edge_on_optimum = forward.cost[note - 1][a]
                    .saturating_add(chain.pairwise[note][a][c])
                    .saturating_add(chain.unary[note][c])
                    .saturating_add(backward.cost[note][c])
                    == optimum;
                if !edge_on_optimum {
                    continue;
                }
                if let Some(value) = low[note - 1][a] {
                    lo = Some(lo.map_or(value, |l| l.min(value)));
                }
                if let Some((value, _)) = high[note - 1][a] {
                    if hi.is_none_or(|(h, _)| value > h) {
                        hi = Some((value, a));
                    }
                }
            }
            low_layer.push(lo.map(|l| l + here));
            high_layer.push(hi.map(|(h, a)| (h + here, a)));
        }
        low.push(low_layer);
        high.push(high_layer);
    }

    let last = n - 1;
    let min = low[last].iter().flatten().copied().min().unwrap_or(0);
    let mut end: Option<(usize, usize)> = None;
    for (c, cell) in high[last].iter().enumerate() {
        if let Some((value, _)) = cell {
            if end.is_none_or(|(best, _)| *value > best) {
                end = Some((*value, c));
            }
        }
    }
    let (max, mut c) = end.unwrap_or((0, 0));
    let mut best_path = vec![0; n];
    for note in (0..n).rev() {
        best_path[note] = c;
        c = high[note][c].map_or(0, |(_, parent)| parent);
    }
    AgreementRange {
        min,
        max,
        expected,
        best_path,
    }
}

fn dot(weights: &Features, features: &Features) -> i128 {
    weights
        .iter()
        .zip(features)
        .map(|(w, f)| i128::from(*w).saturating_mul(i128::from(*f)))
        .fold(0, i128::saturating_add)
}

fn add_features(total: &mut Features, part: &Features) {
    for (t, p) in total.iter_mut().zip(part) {
        *t = t.saturating_add(*p);
    }
}

fn note_features(position: FretboardPosition) -> Features {
    let mut f = [0_i64; FEATURES];
    f[0] = i64::from(position.fret);
    f[1] = i64::from(position.fret == 0);
    let string = usize::from(position.string.clamp(1, 7));
    f[1 + string] = 1;
    f
}

fn transition_features(from: FretboardPosition, to: FretboardPosition) -> Features {
    let ds = i64::from(to.string) - i64::from(from.string);
    let df = i64::from(to.fret) - i64::from(from.fret);
    let fretted = from.fret > 0 && to.fret > 0;
    let mut f = [0_i64; FEATURES];
    f[9] = df.abs();
    f[10] = ds.abs();
    f[11] = i64::from(ds != 0);
    f[12] = i64::from(df == 0 && fretted);
    f[13] = i64::from(df.abs() > 3 && fretted);
    f[14] = i64::from(df.abs() > 5 && fretted);
    f[15] = i64::from(from.fret == 0 || to.fret == 0);
    f[16] = i64::from(ds != 0 && df != 0);
    f[17] = i64::from(ds < 0);
    f[18] = i64::from(df > 0);
    f[19] = i64::from(ds != 0 && df != 0 && ds.signum() == df.signum());
    f
}
