//! Deterministic layered-path engine (S7 Slice A, ADR-0013).
//!
//! The *route*, not the map: given ordered layers of feasible states, the
//! caller's local and transition cost facts, and a versioned weight policy,
//! this returns the single best path — one state per layer — by exact dynamic
//! programming.
//!
//! The engine is deliberately domain-free. It knows nothing of notes, bars,
//! strategies, generation, or any frontend; a state is an ordinal in a layer
//! and a cost is a weighted set of caller-supplied [`Axes`]. That is what makes
//! it reusable: the first client (S7 Slice B's multi-bar candidate chain) is a
//! client, not a special case.
//!
//! Determinism (SPEC §6, ADR-0013 §3) comes from construction, not from a seed:
//! exact DP over a fixed cost function has a unique optimum, and exact ties
//! break by the **lexicographically smallest vector of state ordinals**. No RNG
//! and no seed take part in selection.
//!
//! Explainability reuses the ADR-0017 vocabulary from [`crate::scoring`] — the
//! same [`Axes`], [`WeightPolicy`], and [`Scored`] envelope every other score
//! in `griff` wears. The total is *derived* from the retained per-axis
//! rationale, never the only thing kept (the anti-scalar rule, ADR-0017 §2).
//!
//! Determinism also has an arithmetic half. Float addition is not associative,
//! so a path's cost is not well defined until the *order of the additions* is:
//! [`PATH_COST_ASSOCIATION`] fixes it, and the DP, the walk, the reported
//! total, and every client baseline fold under that one grouping. This is why
//! clients evaluate their baselines *through* [`solve`] rather than adding the
//! same terms up themselves.

use std::cmp::{Ordering, Reverse};
use std::collections::BinaryHeap;

use crate::scoring::{Axes, Provenance, Scored, WeightPolicy};

/// A state's address: its layer and its ordinal within that layer.
///
/// The ordinal is the caller's stable order. It is the tie-breaking key, so the
/// caller controls which of two equally-good paths wins by choosing the order
/// it hands the states in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StateId {
    /// Index of the layer, `0..layers`.
    pub layer: usize,
    /// Index of the state within its layer.
    pub ordinal: usize,
}

/// An edge's address: the two adjacent states it joins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EdgeId {
    /// The state in layer `i`.
    pub from: StateId,
    /// The state in layer `i + 1`.
    pub to: StateId,
}

/// One layered problem: the caller's cost facts plus the policy weighting them.
///
/// Everything is borrowed — the engine never mutates the layers, and the caller
/// keeps ownership of its facts.
#[derive(Debug, Clone, Copy)]
pub struct LayeredProblem<'a> {
    /// Local cost facts: `locals[i][s]` describes state `s` of layer `i`. The
    /// outer length is the layer count; each inner length is that layer's size.
    pub locals: &'a [Vec<Axes>],
    /// Transition cost facts: `transitions[i][p][s]` describes the edge from
    /// state `p` of layer `i` to state `s` of layer `i + 1`. The outer length
    /// must be `locals.len() - 1`.
    pub transitions: &'a [Vec<Vec<Axes>>],
    /// The versioned policy weighting both local and transition axes. Weights
    /// are data (ADR-0013 §4): an axis the policy does not name contributes
    /// nothing, so the caller must name every axis it wants counted.
    pub policy: &'a WeightPolicy,
}

/// Why a layered problem could not be solved.
///
/// Every variant names *where* the problem is, so a caller can point at the
/// offending layer, state, or edge rather than guess.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PathError {
    /// The problem had no layers at all.
    NoLayers,
    /// Layer `layer` had no states, so no path can cross it.
    EmptyLayer {
        /// The empty layer's index.
        layer: usize,
    },
    /// The number of transition tables is not `layers - 1`.
    ///
    /// A count mismatch is its own fact, not the shape of an imaginary layer:
    /// an extra table joins nothing, and a one-layer problem has nowhere to
    /// put one.
    TransitionCount {
        /// The count the layers require: `layers - 1`.
        expected: usize,
        /// The count the caller supplied.
        found: usize,
    },
    /// The transition table's shape does not match the layers it joins.
    TransitionShape {
        /// The layer the transition table leaves from.
        layer: usize,
        /// The shape the layers require: `(|L[layer]|, |L[layer + 1]|)`.
        expected: (usize, usize),
        /// The shape the caller supplied.
        found: (usize, usize),
    },
    /// A local cost was not finite (`NaN`, `+∞`, or `-∞`), so no total order
    /// over costs exists.
    NonFiniteLocal {
        /// The offending state.
        state: StateId,
        /// The non-finite aggregate.
        cost: f64,
    },
    /// A transition cost was not finite, so no total order over costs exists.
    NonFiniteTransition {
        /// The offending edge.
        edge: EdgeId,
        /// The non-finite aggregate.
        cost: f64,
    },
    /// Finite costs accumulated to a non-finite running total.
    ///
    /// Individually finite aggregates do not make a finite *path*: a sum can
    /// still overflow to `±∞`. The solver refuses rather than clamp, and never
    /// returns a solution whose total is not finite.
    NonFiniteAccumulation {
        /// The state whose completion (or running total) went non-finite.
        state: StateId,
        /// The offending accumulated value.
        cost: f64,
    },
    /// [`solve_k_best`] was asked for no alternatives at all.
    ///
    /// An empty set is not an answer to "show me the alternatives"; it is a
    /// caller that has not decided how many it wants.
    KZero,
    /// [`solve_k_best`] was asked for a minimum distance of zero.
    ///
    /// Distance zero admits a path identical to one already chosen, which would
    /// make the word *alternative* false. The rule has to demand at least one
    /// layer of difference to mean anything.
    MinDistanceZero,
    /// The requested minimum distance exceeds the number of layers, so no two
    /// paths in this problem could ever satisfy it.
    ///
    /// Returning the optimum alone would answer a question the caller did not
    /// ask — it looks like "there are no alternatives" when the truth is "that
    /// rule cannot be met here".
    MinDistanceUnsatisfiable {
        /// The distance asked for.
        min_distance: usize,
        /// The layers available to differ in.
        layers: usize,
    },
}

/// The one association every path cost in this engine is folded under.
///
/// Float addition is not associative: `(a + b) + c` and `a + (b + c)` are
/// different functions, and on costs of very different magnitudes they select
/// different winners and report different totals. One of them therefore has to
/// be normative, and everything — the DP, the lexicographic walk, the reported
/// [`PathSolution::total_cost`], and any client's baseline — has to use that
/// one. A number folded a second way describes a path this engine did not
/// choose for a reason it did not have.
///
/// The recurrence's own grouping is normative, right-associated from the end:
///
/// ```text
/// cost(last)  = local(last)
/// cost(i)     = local(i) + ( edge(i) + cost(i+1) )
/// ```
pub const PATH_COST_ASSOCIATION: &str = "local + (edge + suffix), folded from the last layer back";

/// The deterministic best path: one state per layer, with its explanation.
///
/// `total_cost` is derived from the retained rationales — the trace is the
/// truth, the scalar is a convenience.
#[derive(Debug, Clone)]
pub struct PathSolution {
    /// The selected state of each layer, in layer order, each with its local
    /// axes, rationale, and provenance.
    pub steps: Vec<Scored<StateId>>,
    /// The selected edge between each adjacent pair, in layer order. Length is
    /// `steps.len() - 1`.
    pub edges: Vec<Scored<EdgeId>>,
    /// The path's cost under the **canonical association** — see
    /// [`PATH_COST_ASSOCIATION`]. Not merely the sum of the trace: float
    /// addition is not associative, so *how* the trace is folded is part of what
    /// the number means, and this is folded exactly as the search folded it.
    pub total_cost: f64,
    /// The policy the costs were weighed under (no seed: selection uses none).
    pub provenance: Provenance,
}

impl PathSolution {
    /// The selected ordinal of each layer, in layer order — the vector the
    /// tie-breaking rule minimises lexicographically.
    #[must_use]
    pub fn ordinals(&self) -> Vec<usize> {
        self.steps.iter().map(|s| s.value.ordinal).collect()
    }
}

/// Solves a layered problem exactly: the minimum-cost path, ties broken by the
/// lexicographically smallest vector of state ordinals.
///
/// Minimises `Σ local(selected) + Σ transition(selected adjacent pairs)` by
/// dynamic programming over the layered DAG, in
/// `O(Σᵢ |L[i-1]| × |L[i]|)` — polynomial, never greedy and never a beam.
///
/// # Errors
/// [`PathError`] when the problem has no layers, an empty layer, a transition
/// table whose shape does not match its layers, or any non-finite cost.
pub fn solve(problem: &LayeredProblem<'_>) -> Result<PathSolution, PathError> {
    let prepared = Prepared::of(problem)?;

    // Forward pass: walk the optimum, taking the lowest ordinal among exact
    // ties at every layer. Deciding front-to-back is what makes the winner the
    // lexicographically smallest optimal path rather than merely *an* optimum.
    let chosen = prepared.walk_optimum();

    prepared.assemble(problem, &chosen)
}

/// How many alternatives to return, and how different they must be.
///
/// `min_distance` is a **Hamming distance in layers**: two paths are that far
/// apart when they choose different states in at least that many layers. It is
/// the whole reason this is not plain k-best — over a trellis the second-best
/// path is almost always the winner with one layer nudged, and a list of those
/// is one alternative wearing several hats. `1` means "merely a different
/// path"; `2` and above force genuinely different routes.
///
/// The rule is a *constraint*, never a score: nothing here trades cost against
/// novelty behind a tuning constant, so what came back is always explainable as
/// "the cheapest paths that are this far apart".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KBestRequest {
    /// How many alternatives to return, at most.
    pub k: usize,
    /// The fewest layers any two returned paths must differ in.
    pub min_distance: usize,
}

/// Several ranked global paths, and the rule they were selected under.
///
/// **Identity is the ordinal vector, not the rank.** A route keeps
/// [`PathSolution::ordinals`] across re-planning, while its index moves the
/// moment `k` or `min_distance` changes — so an S9 record of "the human chose
/// this one" must store the vector, and an S8 display that labels alternatives
/// by position is labelling something that does not hold still.
#[derive(Debug, Clone)]
pub struct KBestSolution {
    /// The alternatives, cheapest first, each a complete [`PathSolution`].
    ///
    /// Ordering is by cost, and exact ties break by the lexicographically
    /// smallest ordinal vector — the same rule that decides [`solve`]'s single
    /// winner, so the set is as reproducible as the winner was.
    pub paths: Vec<PathSolution>,
    /// The request these paths answer. It travels with them because the rule is
    /// part of what the set means.
    pub request: KBestRequest,
    /// Whether the search ran out of qualifying paths before reaching `k`.
    ///
    /// `true` with fewer than `k` paths is the honest shortfall: the problem
    /// (or the diversity rule) admits no more. The set is never padded and
    /// never repeats itself to reach a count.
    pub exhausted: bool,
}

impl KBestSolution {
    /// The ordinal vector of each alternative, in rank order.
    #[must_use]
    pub fn ordinals(&self) -> Vec<Vec<usize>> {
        self.paths.iter().map(PathSolution::ordinals).collect()
    }
}

/// Returns up to `k` ranked global paths, no two closer than
/// `request.min_distance` layers apart.
///
/// **Prior art.** This is a trellis, so the k-best problem is an old one and is
/// not reinvented here. The enumeration is the *serial* list Viterbi algorithm
/// (Seshadri & Sundberg 1994) in Lawler's formulation (1972): each already-found
/// path is branched at every layer after its own deviation point, the branch's
/// cost is read off the backward table [`solve`] already computes, and a heap
/// pops them in nondecreasing order. Every path is generated exactly once,
/// because the paths differing from a found one first at layer `i` partition the
/// remainder. Eppstein's sidetrack heap (1998) buys asymptotics that bar-scale
/// problems do not need, and Yen (1971) recomputes shortest paths this engine
/// already has. The diversity rule is the greedy conditioning of `DivMBest` (Batra
/// et al., ECCV 2012) — accept the cheapest path far enough from everything
/// already accepted — chosen over MMR's relevance/novelty trade-off and over
/// DPPs, both of which price diversity with a constant instead of stating a rule.
///
/// **Greedy, and says so.** The set is not jointly optimal: it is what you get
/// by taking the cheapest qualifying path, then the cheapest qualifying path
/// given that one, and so on. A jointly optimal diverse set is a different and
/// much harder problem, and it is not what an alternatives list needs.
///
/// Determinism is Slice A's, unchanged: no seed, no RNG, exact costs, and exact
/// ties broken by the lexicographically smallest ordinal vector.
///
/// # Errors
/// [`PathError::KZero`], [`PathError::MinDistanceZero`] or
/// [`PathError::MinDistanceUnsatisfiable`] for a request that cannot mean
/// anything, plus every structural and finiteness error [`solve`] can return —
/// the two entry points validate through one function.
pub fn solve_k_best(
    problem: &LayeredProblem<'_>,
    request: KBestRequest,
) -> Result<KBestSolution, PathError> {
    if request.k == 0 {
        return Err(PathError::KZero);
    }
    if request.min_distance == 0 {
        return Err(PathError::MinDistanceZero);
    }

    let prepared = Prepared::of(problem)?;
    let layers = prepared.layers();
    if request.min_distance > layers {
        return Err(PathError::MinDistanceUnsatisfiable {
            min_distance: request.min_distance,
            layers,
        });
    }

    let mut accepted: Vec<Vec<usize>> = Vec::new();
    let mut paths: Vec<PathSolution> = Vec::new();
    let mut queue: BinaryHeap<Reverse<Candidate>> = BinaryHeap::new();

    let optimum = prepared.walk_optimum();
    if let Some(cost) = prepared.prefix_cost(&optimum, 0) {
        queue.push(Reverse(Candidate {
            cost,
            ordinals: optimum,
            branch_from: 0,
        }));
    }

    let exhausted = loop {
        let Some(Reverse(candidate)) = queue.pop() else {
            break true;
        };

        // Rejected paths still branch: a near-clone of an accepted path can have
        // descendants far from it, and pruning here would lose them silently.
        if accepted
            .iter()
            .all(|chosen| hamming(chosen, &candidate.ordinals) >= request.min_distance)
        {
            paths.push(prepared.assemble(problem, &candidate.ordinals)?);
            accepted.push(candidate.ordinals.clone());
            if paths.len() == request.k {
                break false;
            }
        }

        for layer in candidate.branch_from..layers {
            let held = candidate.ordinals.get(layer).copied().unwrap_or(0);
            let width = prepared.local.get(layer).map_or(0, Vec::len);
            for ordinal in (0..width).filter(|&o| o != held) {
                let mut ordinals: Vec<usize> =
                    candidate.ordinals.get(..layer).unwrap_or_default().to_vec();
                ordinals.extend(prepared.walk_from(layer, ordinal));
                if let Some(cost) = prepared.prefix_cost(&ordinals, layer) {
                    queue.push(Reverse(Candidate {
                        cost,
                        ordinals,
                        branch_from: layer.saturating_add(1),
                    }));
                }
            }
        }
    };

    Ok(KBestSolution {
        paths,
        request,
        exhausted,
    })
}

/// The number of layers two paths disagree on.
fn hamming(a: &[usize], b: &[usize]) -> usize {
    a.iter().zip(b.iter()).filter(|(x, y)| x != y).count()
}

/// One enumerated path waiting its turn, with the first layer its own branches
/// may deviate at.
///
/// Ordered cheapest first, exact ties by the lexicographically smallest ordinal
/// vector — so the queue imposes precisely the order the contract promises.
/// `total_cmp` is a total order and every cost reaching here has been checked
/// finite, so no comparison can be undefined.
#[derive(Debug, Clone)]
struct Candidate {
    cost: f64,
    ordinals: Vec<usize>,
    branch_from: usize,
}

impl Ord for Candidate {
    fn cmp(&self, other: &Self) -> Ordering {
        self.cost
            .total_cmp(&other.cost)
            .then_with(|| self.ordinals.cmp(&other.ordinals))
    }
}

impl PartialOrd for Candidate {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for Candidate {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Candidate {}

/// The shared preparation behind [`solve`] and [`solve_k_best`]: the validated
/// problem, its weighed costs, and the backward DP table.
///
/// Both entry points run this one function, so k-best cannot become a second,
/// laxer door — the same shape checks, the same finiteness checks, the same
/// `suffix` table, and therefore the same cost association.
struct Prepared {
    local: LocalCosts,
    transition: TransitionCosts,
    /// `suffix[i][s]`: the cheapest completion from state `s` of layer `i` to
    /// the end, its own local cost included.
    suffix: Vec<Vec<f64>>,
}

impl Prepared {
    /// Validates `problem` and runs the backward DP.
    fn of(problem: &LayeredProblem<'_>) -> Result<Self, PathError> {
        let layers = problem.locals.len();
        if layers == 0 {
            return Err(PathError::NoLayers);
        }
        for (layer, states) in problem.locals.iter().enumerate() {
            if states.is_empty() {
                return Err(PathError::EmptyLayer { layer });
            }
        }
        // The outer count first: an unreachable extra table must never get to
        // report its own contents as the problem.
        let expected = layers.saturating_sub(1);
        if problem.transitions.len() != expected {
            return Err(PathError::TransitionCount {
                expected,
                found: problem.transitions.len(),
            });
        }
        check_transition_shapes(problem)?;

        let local = score_locals(problem)?;
        let transition = score_transitions(problem)?;
        // Fallible: finite costs can still accumulate past f64's range.
        let suffix = suffix_costs(&local, &transition)?;

        Ok(Self {
            local,
            transition,
            suffix,
        })
    }

    /// The number of layers.
    const fn layers(&self) -> usize {
        self.local.len()
    }

    /// The lexicographically smallest optimal path, as ordinals.
    fn walk_optimum(&self) -> Vec<usize> {
        let first = self.suffix.first().map_or(0, |s| argmin_first(s));
        self.walk_from(0, first)
    }

    /// The lexicographically smallest optimal completion from state `ordinal`
    /// of layer `layer`, as the ordinals of layers `layer..`.
    fn walk_from(&self, layer: usize, ordinal: usize) -> Vec<usize> {
        let mut chosen = Vec::with_capacity(self.layers().saturating_sub(layer));
        chosen.push(ordinal);
        for current in layer..self.layers().saturating_sub(1) {
            let from = chosen.last().copied().unwrap_or(0);
            let completions = self
                .suffix
                .get(current.saturating_add(1))
                .map_or(&[][..], Vec::as_slice);
            let combined: Vec<f64> = self
                .transition
                .get(current)
                .and_then(|t| t.get(from))
                .map_or_else(Vec::new, |row| {
                    row.iter()
                        .zip(completions.iter())
                        .map(|(edge, &completion)| edge.aggregate + completion)
                        .collect()
                });
            chosen.push(argmin_first(&combined));
        }
        chosen
    }

    /// The cost of the path that follows `ordinals` through layer `through` and
    /// then completes optimally, folded under [`PATH_COST_ASSOCIATION`].
    ///
    /// The fold starts from `suffix[through][ordinals[through]]` — already the
    /// right-associated completion cost — and wraps the fixed prefix around it
    /// from the back, which is the recurrence's own grouping. A key computed as
    /// `prefix + suffix` would be a different function of the same terms, and
    /// would order alternatives by a number none of them reports.
    // `total = x + total` is NOT `total += x`: see `trace_total`.
    #[allow(clippy::assign_op_pattern)]
    fn prefix_cost(&self, ordinals: &[usize], through: usize) -> Option<f64> {
        let mut total = *self.suffix.get(through)?.get(*ordinals.get(through)?)?;
        for layer in (0..through).rev() {
            let from = *ordinals.get(layer)?;
            let to = *ordinals.get(layer.saturating_add(1))?;
            let edge = self.transition.get(layer)?.get(from)?.get(to)?.aggregate;
            total = self.local.get(layer)?.get(from)?.aggregate + (edge + total);
        }
        Some(total)
    }

    /// Builds the full [`PathSolution`] for an ordinal vector.
    fn assemble(
        &self,
        problem: &LayeredProblem<'_>,
        chosen: &[usize],
    ) -> Result<PathSolution, PathError> {
        let steps: Vec<Scored<StateId>> = chosen
            .iter()
            .enumerate()
            .filter_map(|(layer, &ordinal)| {
                self.local
                    .get(layer)?
                    .get(ordinal)
                    .map(|c| c.scored.clone())
            })
            .collect();
        let edges: Vec<Scored<EdgeId>> = chosen
            .windows(2)
            .enumerate()
            .filter_map(|(layer, pair)| {
                let (from, to) = (*pair.first()?, *pair.get(1)?);
                self.transition
                    .get(layer)?
                    .get(from)?
                    .get(to)
                    .map(|c| c.scored.clone())
            })
            .collect();

        // Derived from the retained rationale — the trace is the truth
        // (ADR-0017 §2). Summed in path order and checked at every step: a
        // finite `suffix` does not make this sum finite, because it adds the
        // same terms in a different order.
        let total_cost = trace_total(&steps, &edges)?;

        Ok(PathSolution {
            steps,
            edges,
            total_cost,
            provenance: Provenance {
                policy_id: problem.policy.id,
                policy_version: problem.policy.version,
                seed: None,
            },
        })
    }
}

/// Folds the selected trace in **the recurrence's association**, checking every
/// addition.
///
/// The engine derives its total from the retained rationale rather than from
/// `suffix[0]` (ADR-0017 §2: the trace is the truth), so this fold must be the
/// same arithmetic the search ran on — float addition is not associative, and a
/// total grouped differently from the selection is a number for a path the
/// engine did not choose. It therefore walks **from the end**, mirroring
/// [`suffix_costs`] exactly:
///
/// ```text
/// total = local(last)
/// total = local(i) + (edge(i) + total)   for each preceding layer, in reverse
/// ```
///
/// Every step is checked in its own right: `suffix` being finite does not make
/// this finite, since the two are re-derived rather than shared. The first
/// state whose addition goes non-finite is named.
// `total = x + total` is NOT `total += x`: the accumulator has to stay on the
// right, or the fold silently becomes the left-associated one this function
// exists to stop being.
#[allow(clippy::assign_op_pattern)]
fn trace_total(steps: &[Scored<StateId>], edges: &[Scored<EdgeId>]) -> Result<f64, PathError> {
    let mut back = steps.iter().rev();
    let Some(last) = back.next() else {
        return Ok(0.0);
    };
    let mut total = last.aggregate();
    check_accumulation(total, last.value)?;
    // `steps` has one more entry than `edges`, so reversing both pairs step `i`
    // with the edge leaving it.
    for (step, edge) in back.zip(edges.iter().rev()) {
        total = edge.aggregate() + total;
        check_accumulation(total, edge.value.to)?;
        total = step.aggregate() + total;
        check_accumulation(total, step.value)?;
    }
    Ok(total)
}

/// Rejects a non-finite accumulation, naming the state it happened at.
const fn check_accumulation(total: f64, state: StateId) -> Result<(), PathError> {
    if total.is_finite() {
        Ok(())
    } else {
        Err(PathError::NonFiniteAccumulation { state, cost: total })
    }
}

/// A scored cost with its aggregate kept beside it, so the DP inner loops read
/// a number instead of re-summing a rationale.
#[derive(Debug, Clone)]
struct Cost<T> {
    scored: Scored<T>,
    aggregate: f64,
}

/// Per layer, per state: the weighed local cost.
type LocalCosts = Vec<Vec<Cost<StateId>>>;

/// Per adjacent layer pair, per `(from, to)`: the weighed transition cost.
type TransitionCosts = Vec<Vec<Vec<Cost<EdgeId>>>>;

/// A borrowed view of [`LocalCosts`].
type LocalCostSlice = [Vec<Cost<StateId>>];

/// A borrowed view of [`TransitionCosts`].
type TransitionCostSlice = [Vec<Vec<Cost<EdgeId>>>];

/// Rejects a transition table whose shape does not match the layers it joins.
fn check_transition_shapes(problem: &LayeredProblem<'_>) -> Result<(), PathError> {
    for layer in 0..problem.locals.len().saturating_sub(1) {
        let expected = (
            problem.locals.get(layer).map_or(0, Vec::len),
            problem
                .locals
                .get(layer.saturating_add(1))
                .map_or(0, Vec::len),
        );
        let table = problem.transitions.get(layer);
        let found = table.map_or((0, 0), |t| (t.len(), t.first().map_or(0, Vec::len)));
        let ok = table
            .is_some_and(|t| t.len() == expected.0 && t.iter().all(|row| row.len() == expected.1));
        if !ok {
            return Err(PathError::TransitionShape {
                layer,
                expected,
                found,
            });
        }
    }
    Ok(())
}

/// Weighs every local axis set, rejecting the first non-finite cost.
fn score_locals(problem: &LayeredProblem<'_>) -> Result<LocalCosts, PathError> {
    problem
        .locals
        .iter()
        .enumerate()
        .map(|(layer, states)| {
            states
                .iter()
                .enumerate()
                .map(|(ordinal, axes)| {
                    let state = StateId { layer, ordinal };
                    let scored = Scored::new(state, axes.clone(), problem.policy, None);
                    let aggregate = scored.aggregate();
                    if aggregate.is_finite() {
                        Ok(Cost { scored, aggregate })
                    } else {
                        Err(PathError::NonFiniteLocal {
                            state,
                            cost: aggregate,
                        })
                    }
                })
                .collect()
        })
        .collect()
}

/// Weighs every transition axis set, rejecting the first non-finite cost.
fn score_transitions(problem: &LayeredProblem<'_>) -> Result<TransitionCosts, PathError> {
    problem
        .transitions
        .iter()
        .enumerate()
        .map(|(layer, table)| {
            table
                .iter()
                .enumerate()
                .map(|(from, row)| {
                    row.iter()
                        .enumerate()
                        .map(|(to, axes)| {
                            let edge = EdgeId {
                                from: StateId {
                                    layer,
                                    ordinal: from,
                                },
                                to: StateId {
                                    layer: layer.saturating_add(1),
                                    ordinal: to,
                                },
                            };
                            let scored = Scored::new(edge, axes.clone(), problem.policy, None);
                            let aggregate = scored.aggregate();
                            if aggregate.is_finite() {
                                Ok(Cost { scored, aggregate })
                            } else {
                                Err(PathError::NonFiniteTransition {
                                    edge,
                                    cost: aggregate,
                                })
                            }
                        })
                        .collect()
                })
                .collect()
        })
        .collect()
}

/// The backward DP: `suffix[i][s] = local(i,s) + min_t(trans(i,s,t) + suffix[i+1][t])`,
/// with the last layer's suffix being its local cost alone.
fn suffix_costs(
    local: &LocalCostSlice,
    transition: &TransitionCostSlice,
) -> Result<Vec<Vec<f64>>, PathError> {
    let layers = local.len();
    let mut back: Vec<Vec<f64>> = Vec::with_capacity(layers);
    let mut next: Vec<f64> = local
        .last()
        .map(|states| states.iter().map(|c| c.aggregate).collect())
        .unwrap_or_default();
    back.push(next.clone());

    for layer in (0..layers.saturating_sub(1)).rev() {
        let states = local.get(layer).map_or(&[][..], Vec::as_slice);
        let table = transition.get(layer);
        let mut current: Vec<f64> = Vec::with_capacity(states.len());
        for (from, cost) in states.iter().enumerate() {
            let state = StateId {
                layer,
                ordinal: from,
            };
            // Every `edge + completion` is checked: two finite costs can still
            // sum to ±∞, and an unchecked ∞ would make distinct alternatives
            // compare equal and hand the tie-break a wrong winner.
            let mut best = f64::INFINITY;
            if let Some(row) = table.and_then(|t| t.get(from)) {
                for (edge, &completion) in row.iter().zip(next.iter()) {
                    let reach = edge.aggregate + completion;
                    if !reach.is_finite() {
                        return Err(PathError::NonFiniteAccumulation { state, cost: reach });
                    }
                    if reach < best {
                        best = reach;
                    }
                }
            }
            let total = cost.aggregate + best;
            if !total.is_finite() {
                return Err(PathError::NonFiniteAccumulation { state, cost: total });
            }
            current.push(total);
        }
        next.clone_from(&current);
        back.push(current);
    }
    back.reverse();
    Ok(back)
}

/// The index of the smallest value; the **first** wins an exact tie, which is
/// what makes the path lexicographically smallest.
fn argmin_first(values: &[f64]) -> usize {
    let mut best_index = 0;
    let mut best_value = f64::INFINITY;
    for (index, &value) in values.iter().enumerate() {
        if value < best_value {
            best_value = value;
            best_index = index;
        }
    }
    best_index
}

#[cfg(test)]
// `float_cmp` and `assign_op_pattern` are opted out of deliberately here: the
// association laws below are about exact identity and about which side of the
// `+` the accumulator sits on. Both lints would ask for the very code the laws
// forbid.
#[allow(
    clippy::missing_assert_message,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_cmp,
    clippy::assign_op_pattern
)]
mod tests {
    use super::{solve, EdgeId, LayeredProblem, PathError, PathSolution, StateId};
    use crate::scoring::{Axes, Axis, Scored, WeightPolicy};

    /// The test policy: one local axis and one transition axis, each weighted
    /// `1.0`, so an axis value *is* its cost and the arithmetic stays readable.
    fn policy() -> WeightPolicy {
        WeightPolicy::new("test_path", 1, vec![("local", 1.0), ("trans", 1.0)])
    }

    fn local(value: f64) -> Axes {
        Axes::new(vec![Axis {
            label: "local",
            value,
        }])
    }

    fn trans(value: f64) -> Axes {
        Axes::new(vec![Axis {
            label: "trans",
            value,
        }])
    }

    /// Builds `locals` from per-layer cost values.
    fn locals_of(values: &[&[f64]]) -> Vec<Vec<Axes>> {
        values
            .iter()
            .map(|layer| layer.iter().map(|&v| local(v)).collect())
            .collect()
    }

    /// Builds `transitions` from per-edge cost matrices.
    fn transitions_of(values: &[&[&[f64]]]) -> Vec<Vec<Vec<Axes>>> {
        values
            .iter()
            .map(|matrix| {
                matrix
                    .iter()
                    .map(|row| row.iter().map(|&v| trans(v)).collect())
                    .collect()
            })
            .collect()
    }

    #[test]
    fn a_single_layer_selects_its_lowest_local_cost() {
        let locals = locals_of(&[&[3.0, 1.0, 2.0]]);
        let transitions: Vec<Vec<Vec<Axes>>> = Vec::new();
        let p = policy();
        let solution = solve(&LayeredProblem {
            locals: &locals,
            transitions: &transitions,
            policy: &p,
        })
        .expect("one layer solves");
        assert_eq!(solution.ordinals(), vec![1]);
        assert!((solution.total_cost - 1.0).abs() < 1e-9);
        assert!(solution.edges.is_empty(), "no edges without a second layer");
    }

    #[test]
    fn one_state_per_layer_returns_that_only_path() {
        let locals = locals_of(&[&[5.0], &[7.0]]);
        let transitions = transitions_of(&[&[&[11.0]]]);
        let p = policy();
        let solution = solve(&LayeredProblem {
            locals: &locals,
            transitions: &transitions,
            policy: &p,
        })
        .expect("solves");
        assert_eq!(solution.ordinals(), vec![0, 0]);
        assert!((solution.total_cost - 23.0).abs() < 1e-9, "5 + 7 + 11");
    }

    #[test]
    fn the_global_optimum_beats_the_greedy_path() {
        // Layer 0: state 0 is locally cheapest (0 vs 1), but every edge out of
        // it is ruinous. The global optimum takes the dearer local state.
        //   greedy:  0 -> 0 -> 0 = 0 + 100 + 0 + 100 + 0 = 200
        //   optimum: 1 -> 1 -> 1 = 1 + 0 + 1 + 0 + 1 = 3
        let locals = locals_of(&[&[0.0, 1.0], &[0.0, 1.0], &[0.0, 1.0]]);
        let transitions = transitions_of(&[
            &[&[100.0, 100.0], &[100.0, 0.0]],
            &[&[100.0, 100.0], &[100.0, 0.0]],
        ]);
        let p = policy();
        let solution = solve(&LayeredProblem {
            locals: &locals,
            transitions: &transitions,
            policy: &p,
        })
        .expect("solves");
        assert_eq!(
            solution.ordinals(),
            vec![1, 1, 1],
            "DP takes the globally best path, not the locally best state",
        );
        assert!((solution.total_cost - 3.0).abs() < 1e-9);
    }

    #[test]
    fn all_equal_costs_select_ordinal_zero_everywhere() {
        let locals = locals_of(&[&[1.0, 1.0, 1.0], &[1.0, 1.0, 1.0]]);
        let transitions =
            transitions_of(&[&[&[1.0, 1.0, 1.0], &[1.0, 1.0, 1.0], &[1.0, 1.0, 1.0]]]);
        let p = policy();
        let solution = solve(&LayeredProblem {
            locals: &locals,
            transitions: &transitions,
            policy: &p,
        })
        .expect("solves");
        assert_eq!(solution.ordinals(), vec![0, 0], "ties break to ordinal 0");
    }

    #[test]
    fn equal_totals_select_the_lexicographically_lowest_path() {
        // Exactly two paths cost 2.0 — [0,1] = 0+2+0 and [1,0] = 1+0+1 — while
        // [0,0] and [1,1] cost 10. Lexicographic order picks [0,1]: the earliest
        // layer decides.
        let locals = locals_of(&[&[0.0, 1.0], &[1.0, 0.0]]);
        let transitions = transitions_of(&[&[&[9.0, 2.0], &[0.0, 9.0]]]);
        let p = policy();
        let solution = solve(&LayeredProblem {
            locals: &locals,
            transitions: &transitions,
            policy: &p,
        })
        .expect("solves");
        assert_eq!(solution.ordinals(), vec![0, 1]);
        assert!((solution.total_cost - 2.0).abs() < 1e-9);
    }

    #[test]
    fn an_empty_problem_reports_no_layers() {
        let locals: Vec<Vec<Axes>> = Vec::new();
        let transitions: Vec<Vec<Vec<Axes>>> = Vec::new();
        let p = policy();
        assert_eq!(
            solve(&LayeredProblem {
                locals: &locals,
                transitions: &transitions,
                policy: &p,
            })
            .unwrap_err(),
            PathError::NoLayers,
        );
    }

    #[test]
    fn an_empty_middle_layer_reports_its_index() {
        let locals = locals_of(&[&[1.0], &[], &[1.0]]);
        let transitions = transitions_of(&[&[&[]], &[]]);
        let p = policy();
        assert_eq!(
            solve(&LayeredProblem {
                locals: &locals,
                transitions: &transitions,
                policy: &p,
            })
            .unwrap_err(),
            PathError::EmptyLayer { layer: 1 },
        );
    }

    #[test]
    fn a_non_finite_local_cost_is_rejected_with_its_location() {
        let locals = locals_of(&[&[1.0, f64::NAN]]);
        let transitions: Vec<Vec<Vec<Axes>>> = Vec::new();
        let p = policy();
        match solve(&LayeredProblem {
            locals: &locals,
            transitions: &transitions,
            policy: &p,
        }) {
            Err(PathError::NonFiniteLocal { state, cost }) => {
                assert_eq!(
                    state,
                    StateId {
                        layer: 0,
                        ordinal: 1
                    }
                );
                assert!(cost.is_nan());
            }
            other => panic!("expected NonFiniteLocal, got {other:?}"),
        }
    }

    #[test]
    fn a_non_finite_transition_cost_is_rejected_with_its_edge() {
        let locals = locals_of(&[&[1.0], &[1.0, 1.0]]);
        let transitions = transitions_of(&[&[&[0.0, f64::INFINITY]]]);
        let p = policy();
        match solve(&LayeredProblem {
            locals: &locals,
            transitions: &transitions,
            policy: &p,
        }) {
            Err(PathError::NonFiniteTransition { edge, cost }) => {
                assert_eq!(
                    edge,
                    EdgeId {
                        from: StateId {
                            layer: 0,
                            ordinal: 0
                        },
                        to: StateId {
                            layer: 1,
                            ordinal: 1
                        },
                    },
                );
                assert!(cost.is_infinite());
            }
            other => panic!("expected NonFiniteTransition, got {other:?}"),
        }
    }

    #[test]
    fn one_layer_with_a_transition_table_is_rejected() {
        // A single layer has nowhere to put a transition: the table joins nothing.
        let locals = locals_of(&[&[1.0]]);
        let transitions = transitions_of(&[&[&[0.0]]]);
        let p = policy();
        assert_eq!(
            solve(&LayeredProblem {
                locals: &locals,
                transitions: &transitions,
                policy: &p,
            })
            .unwrap_err(),
            PathError::TransitionCount {
                expected: 0,
                found: 1,
            },
        );
    }

    #[test]
    fn an_extra_transition_table_is_rejected() {
        let locals = locals_of(&[&[1.0], &[1.0]]);
        let transitions = transitions_of(&[&[&[0.0]], &[&[0.0]]]); // 2 tables, 1 needed
        let p = policy();
        assert_eq!(
            solve(&LayeredProblem {
                locals: &locals,
                transitions: &transitions,
                policy: &p,
            })
            .unwrap_err(),
            PathError::TransitionCount {
                expected: 1,
                found: 2,
            },
        );
    }

    #[test]
    fn a_missing_transition_table_is_rejected() {
        let locals = locals_of(&[&[1.0], &[1.0]]);
        let transitions: Vec<Vec<Vec<Axes>>> = Vec::new();
        let p = policy();
        assert_eq!(
            solve(&LayeredProblem {
                locals: &locals,
                transitions: &transitions,
                policy: &p,
            })
            .unwrap_err(),
            PathError::TransitionCount {
                expected: 1,
                found: 0,
            },
        );
    }

    #[test]
    fn the_exact_transition_count_is_accepted() {
        let locals = locals_of(&[&[1.0], &[1.0], &[1.0]]);
        let transitions = transitions_of(&[&[&[0.0]], &[&[0.0]]]);
        let p = policy();
        solve(&LayeredProblem {
            locals: &locals,
            transitions: &transitions,
            policy: &p,
        })
        .expect("the exact count solves");
    }

    #[test]
    fn an_extra_table_holding_nan_reports_the_count_not_the_nan() {
        // The count is checked before any fact is scored, so an unreachable
        // table's NaN never gets the chance to masquerade as the real problem.
        let locals = locals_of(&[&[1.0]]);
        let transitions = transitions_of(&[&[&[f64::NAN]]]);
        let p = policy();
        assert_eq!(
            solve(&LayeredProblem {
                locals: &locals,
                transitions: &transitions,
                policy: &p,
            })
            .unwrap_err(),
            PathError::TransitionCount {
                expected: 0,
                found: 1,
            },
        );
    }

    #[test]
    fn a_mismatched_transition_table_reports_the_expected_shape() {
        let locals = locals_of(&[&[1.0, 2.0], &[1.0]]);
        let transitions = transitions_of(&[&[&[0.0]]]); // 1x1, but 2x1 is required
        let p = policy();
        assert_eq!(
            solve(&LayeredProblem {
                locals: &locals,
                transitions: &transitions,
                policy: &p,
            })
            .unwrap_err(),
            PathError::TransitionShape {
                layer: 0,
                expected: (2, 1),
                found: (1, 1),
            },
        );
    }

    #[test]
    fn two_finite_costs_overflowing_to_infinity_are_rejected() {
        // Each aggregate is finite; their sum is not. The path is refused, not
        // clamped, and not silently carried as an infinite total.
        let big = f64::MAX * 0.75;
        let locals = locals_of(&[&[big], &[big]]);
        let transitions = transitions_of(&[&[&[0.0]]]);
        let p = policy();
        match solve(&LayeredProblem {
            locals: &locals,
            transitions: &transitions,
            policy: &p,
        }) {
            Err(PathError::NonFiniteAccumulation { cost, .. }) => {
                assert!(cost.is_infinite(), "the accumulation overflowed");
            }
            other => panic!("expected NonFiniteAccumulation, got {other:?}"),
        }
    }

    #[test]
    fn two_finite_costs_underflowing_to_negative_infinity_are_rejected() {
        let small = f64::MIN * 0.75;
        let locals = locals_of(&[&[small], &[small]]);
        let transitions = transitions_of(&[&[&[0.0]]]);
        let p = policy();
        match solve(&LayeredProblem {
            locals: &locals,
            transitions: &transitions,
            policy: &p,
        }) {
            Err(PathError::NonFiniteAccumulation { cost, .. }) => {
                assert!(
                    cost.is_infinite() && cost.is_sign_negative(),
                    "the accumulation underflowed",
                );
            }
            other => panic!("expected NonFiniteAccumulation, got {other:?}"),
        }
    }

    #[test]
    fn overflow_does_not_collapse_distinct_alternatives_onto_ordinal_zero() {
        // Left unchecked, both candidate completions become +inf, compare equal,
        // and the lexicographic rule hands back ordinal 0 — a wrong answer
        // wearing a plausible face. Refusing is the only honest option here.
        let big = f64::MAX * 0.75;
        let locals = locals_of(&[&[0.0, 0.0], &[big, big]]);
        let transitions = transitions_of(&[&[&[big, big * 0.5], &[big, big]]]);
        let p = policy();
        assert!(
            matches!(
                solve(&LayeredProblem {
                    locals: &locals,
                    transitions: &transitions,
                    policy: &p,
                }),
                Err(PathError::NonFiniteAccumulation { .. })
            ),
            "an overflowed comparison must never silently pick ordinal 0",
        );
    }

    #[test]
    fn an_overflow_in_a_later_suffix_layer_names_its_state() {
        let big = f64::MAX * 0.75;
        let locals = locals_of(&[&[0.0], &[big], &[big]]);
        let transitions = transitions_of(&[&[&[0.0]], &[&[0.0]]]);
        let p = policy();
        match solve(&LayeredProblem {
            locals: &locals,
            transitions: &transitions,
            policy: &p,
        }) {
            Err(PathError::NonFiniteAccumulation { state, .. }) => {
                assert_eq!(
                    state,
                    StateId {
                        layer: 1,
                        ordinal: 0
                    },
                    "the layer whose completion overflowed is named",
                );
            }
            other => panic!("expected NonFiniteAccumulation, got {other:?}"),
        }
    }

    #[test]
    fn a_returned_solution_always_has_a_finite_total() {
        let locals = locals_of(&[&[2.0, 5.0], &[3.0, 1.0], &[0.5, 4.0]]);
        let transitions =
            transitions_of(&[&[&[4.0, 0.5], &[1.0, 1.0]], &[&[2.0, 3.0], &[1.5, 0.25]]]);
        let p = policy();
        let solution = solve(&LayeredProblem {
            locals: &locals,
            transitions: &transitions,
            policy: &p,
        })
        .expect("solves");
        assert!(
            solution.total_cost.is_finite(),
            "the solver never returns a non-finite total",
        );
    }

    #[test]
    fn the_explanations_sum_to_the_reported_total() {
        let locals = locals_of(&[&[2.0, 5.0], &[3.0, 1.0]]);
        let transitions = transitions_of(&[&[&[4.0, 0.5], &[1.0, 1.0]]]);
        let p = policy();
        let solution = solve(&LayeredProblem {
            locals: &locals,
            transitions: &transitions,
            policy: &p,
        })
        .expect("solves");
        // Folded in the recurrence's own association, and compared exactly.
        // Summing the steps and the edges in two separate passes is a third
        // grouping again, and "within 1e-12" is how a real disagreement about
        // what the number means gets waved through as rounding.
        assert_eq!(
            canonical_total(&solution),
            solution.total_cost,
            "the trace explains the whole total, in the order it was optimised",
        );
    }

    /// The path cost of a solution's own trace, folded from the end in the
    /// recurrence's association: `local + (edge + rest)`.
    fn canonical_total(solution: &PathSolution) -> f64 {
        let mut steps = solution.steps.iter().rev();
        let mut total = steps.next().map_or(0.0, Scored::aggregate);
        for (step, edge) in steps.zip(solution.edges.iter().rev()) {
            total = edge.aggregate() + total;
            total = step.aggregate() + total;
        }
        total
    }

    /// The selected state ordinals, layer by layer.
    fn ordinals(solution: &PathSolution) -> Vec<usize> {
        solution.steps.iter().map(|s| s.value.ordinal).collect()
    }

    #[test]
    fn the_two_associations_really_do_disagree_here() {
        // Guards the fixture below rather than the engine: `1e16` and `-1e16`
        // cancel exactly, and the ulp at 1e16 is 2, so a 1.0 added *before* the
        // cancellation is rounded away while the same 1.0 added *after* it
        // survives. If f64 ever stopped behaving this way the laws below would
        // pass while proving nothing.
        assert_eq!(1e16 + (-1e16 + 1.0), 0.0, "the 1.0 is absorbed");
        assert_eq!((1e16 + -1e16) + 1.0, 1.0, "the 1.0 survives");
        assert_eq!(1e16 + (-1e16 + 0.5), 0.0);
        assert_eq!((1e16 + -1e16) + 0.5, 0.5);
    }

    #[test]
    fn the_reported_total_is_the_cost_the_recurrence_minimised() {
        // One layer of one state, then two alternatives. Under the recurrence's
        // association both alternatives cost 0.0 — the tail is absorbed — so
        // the tie-break takes ordinal 0. Under a forward left-to-right sum they
        // cost 1.0 and 0.5, and ordinal 1 would win.
        //
        // Two associations, two different winners and two different totals. The
        // engine optimises the first, so the first is what it must report: a
        // total from an arithmetic the selection did not use describes a path
        // the engine did not choose for a reason it did not have.
        let locals = locals_of(&[&[1e16], &[1.0, 0.5]]);
        let transitions = transitions_of(&[&[&[-1e16, -1e16]]]);
        let p = policy();
        let solution = solve(&LayeredProblem {
            locals: &locals,
            transitions: &transitions,
            policy: &p,
        })
        .expect("solves");

        assert_eq!(
            ordinals(&solution),
            vec![0, 0],
            "the recurrence sees a tie and the tie-break takes the lowest ordinal",
        );
        assert_eq!(
            solution.total_cost, 0.0,
            "reported: the cost the recurrence actually minimised",
        );
        assert_eq!(
            canonical_total(&solution),
            solution.total_cost,
            "and the trace re-folds to the very same number",
        );
    }

    #[test]
    fn the_baseline_of_a_single_path_is_folded_the_same_way() {
        // The same fixture with the alternative removed: no tie, no choice, and
        // still the recurrence's association — `1e16 + (-1e16 + 1.0)` is 0.0,
        // not 1.0. A one-state-per-layer problem is a cost evaluation rather
        // than a search, and it must not quietly use different arithmetic from
        // the search that it is the baseline for.
        let locals = locals_of(&[&[1e16], &[1.0]]);
        let transitions = transitions_of(&[&[&[-1e16]]]);
        let p = policy();
        let solution = solve(&LayeredProblem {
            locals: &locals,
            transitions: &transitions,
            policy: &p,
        })
        .expect("solves");
        assert_eq!(solution.total_cost, 0.0);
    }

    #[test]
    fn repeated_calls_return_an_identical_path_and_trace() {
        let locals = locals_of(&[&[2.0, 2.0, 1.0], &[1.0, 3.0, 1.0], &[5.0, 0.0, 5.0]]);
        let transitions = transitions_of(&[
            &[&[1.0, 2.0, 3.0], &[3.0, 2.0, 1.0], &[2.0, 2.0, 2.0]],
            &[&[1.0, 1.0, 1.0], &[2.0, 0.0, 2.0], &[3.0, 3.0, 3.0]],
        ]);
        let p = policy();
        let problem = LayeredProblem {
            locals: &locals,
            transitions: &transitions,
            policy: &p,
        };
        let a = solve(&problem).expect("solves");
        let b = solve(&problem).expect("solves");
        assert_eq!(a.ordinals(), b.ordinals());
        assert!((a.total_cost - b.total_cost).abs() < 1e-12);
        let trace = |s: &PathSolution| -> Vec<(f64, f64)> {
            s.steps
                .iter()
                .flat_map(|x| x.rationale.entries().iter().map(|e| (e.value, e.weight)))
                .collect()
        };
        assert_eq!(trace(&a), trace(&b), "the trace is identical too");
    }

    #[test]
    fn the_input_layers_are_not_mutated() {
        let locals = locals_of(&[&[2.0, 5.0], &[3.0, 1.0]]);
        let transitions = transitions_of(&[&[&[4.0, 0.5], &[1.0, 1.0]]]);
        let before: Vec<Vec<f64>> = locals
            .iter()
            .map(|l| l.iter().map(|a| a.get("local").unwrap()).collect())
            .collect();
        let p = policy();
        drop(
            solve(&LayeredProblem {
                locals: &locals,
                transitions: &transitions,
                policy: &p,
            })
            .expect("solves"),
        );
        let after: Vec<Vec<f64>> = locals
            .iter()
            .map(|l| l.iter().map(|a| a.get("local").unwrap()).collect())
            .collect();
        assert_eq!(before, after, "the engine borrows; it never mutates");
    }

    /// The oracle: enumerate every path, keep the minimal `(cost, path)` with
    /// lexicographic tie-breaking. Three nested loops beat a new dependency.
    fn brute_force(
        shape: &[usize],
        local_values: &[Vec<f64>],
        trans_values: &[Vec<Vec<f64>>],
    ) -> (f64, Vec<usize>) {
        let layers = shape.len();
        let mut best: Option<(f64, Vec<usize>)> = None;
        let mut path = vec![0_usize; layers];
        loop {
            let mut total = 0.0;
            for (i, &s) in path.iter().enumerate() {
                total += local_values[i][s];
                if i > 0 {
                    total += trans_values[i - 1][path[i - 1]][s];
                }
            }
            let better = match &best {
                None => true,
                Some((bc, bp)) => {
                    total < *bc - 1e-12
                        || ((total - *bc).abs() <= 1e-12 && path.as_slice() < bp.as_slice())
                }
            };
            if better {
                best = Some((total, path.clone()));
            }
            // Odometer over the shape; `done` when the most significant digit wraps.
            let mut i = layers;
            let mut done = true;
            while i > 0 {
                i -= 1;
                path[i] += 1;
                if path[i] < shape[i] {
                    done = false;
                    break;
                }
                path[i] = 0;
            }
            if done {
                break;
            }
        }
        best.expect("at least one path")
    }

    #[test]
    fn dp_matches_a_brute_force_oracle_on_many_tiny_problems() {
        // An exhaustive check: every tiny problem shape, deterministic integer
        // costs, DP compared against full enumeration for total AND exact path.
        for layers in 1..=4_usize {
            for width in 1..=3_usize {
                let shape: Vec<usize> = (0..layers).map(|_| width).collect();
                // Deterministic pseudo-costs — no RNG, just a fixed mixer.
                let cost_of = |a: usize, b: usize, c: usize| -> f64 {
                    f64::from(u32::try_from((a * 7 + b * 13 + c * 29) % 11).unwrap())
                };
                let local_values: Vec<Vec<f64>> = (0..layers)
                    .map(|i| (0..width).map(|s| cost_of(i, s, 3)).collect())
                    .collect();
                let trans_values: Vec<Vec<Vec<f64>>> = (0..layers.saturating_sub(1))
                    .map(|i| {
                        (0..width)
                            .map(|p| (0..width).map(|s| cost_of(i, p, s)).collect())
                            .collect()
                    })
                    .collect();

                let locals: Vec<Vec<Axes>> = local_values
                    .iter()
                    .map(|l| l.iter().map(|&v| local(v)).collect())
                    .collect();
                let transitions: Vec<Vec<Vec<Axes>>> = trans_values
                    .iter()
                    .map(|m| {
                        m.iter()
                            .map(|r| r.iter().map(|&v| trans(v)).collect())
                            .collect()
                    })
                    .collect();
                let p = policy();
                let got = solve(&LayeredProblem {
                    locals: &locals,
                    transitions: &transitions,
                    policy: &p,
                })
                .expect("solves");

                let (want_cost, want_path) = brute_force(&shape, &local_values, &trans_values);
                assert_eq!(
                    got.ordinals(),
                    want_path,
                    "layers={layers} width={width}: DP path must match the oracle",
                );
                assert!(
                    (got.total_cost - want_cost).abs() < 1e-9,
                    "layers={layers} width={width}: DP total must match the oracle",
                );
            }
        }
    }
}
