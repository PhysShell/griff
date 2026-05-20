//! Graph layer for phrase recombination (S7).
//!
//! Provides a weighted directed graph over musical nodes (`Phrase`, `Motif`,
//! `RhythmCell`, `ChordMovement`, `EnergyState`) and two traversal strategies:
//! weighted random walk and beam search.

use crate::{
    event::{Pitch, Ticks},
    feature::PhraseFeatures,
    generate::GenerationSeed,
};

// ── identifiers ────────────────────────────────────────────────────────────────

/// Index of a node within a [`PhraseGraph`]'s node list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(pub usize);

// ── feature vectors ───────────────────────────────────────────────────────────

/// A floating-point feature vector used for similarity computation.
#[derive(Debug, Clone, PartialEq)]
pub struct NodeFeatureVec {
    /// Component values; empty means "no features".
    pub values: Vec<f64>,
}

/// Computes the cosine similarity of two feature vectors.
///
/// Returns `0.0` when either vector has zero magnitude or is empty.
pub fn cosine_similarity(a: &NodeFeatureVec, b: &NodeFeatureVec) -> f64 {
    if a.values.is_empty() || b.values.is_empty() {
        return 0.0;
    }
    let len = a.values.len().min(b.values.len());
    let mut dot = 0.0_f64;
    let mut mag_a = 0.0_f64;
    let mut mag_b = 0.0_f64;

    for i in 0..len {
        let av = a.values.get(i).copied().unwrap_or(0.0);
        let bv = b.values.get(i).copied().unwrap_or(0.0);
        dot += av * bv;
        mag_a += av * av;
        mag_b += bv * bv;
    }

    let denom = mag_a.sqrt() * mag_b.sqrt();
    if denom == 0.0 {
        0.0
    } else {
        dot / denom
    }
}

/// Converts [`PhraseFeatures`] into a normalised three-component feature vector:
/// `[note_density, avg_velocity_norm, pitch_span_norm]`.
pub fn phrase_features_to_vec(features: &PhraseFeatures) -> NodeFeatureVec {
    let bar_count = u32::try_from(features.bar_count.max(1)).unwrap_or(u32::MAX);
    let note_count = u32::try_from(features.note_count).unwrap_or(u32::MAX);
    let note_density = f64::from(note_count) / f64::from(bar_count);

    let avg_velocity_norm = features.velocity_range.map_or(0.0, |vr| {
        let avg = (f64::from(vr.lowest.0) + f64::from(vr.highest.0)) / 2.0;
        avg / 127.0
    });

    let pitch_span_norm = features.pitch_range.map_or(0.0, |pr| {
        f64::from(pr.highest.0.saturating_sub(pr.lowest.0)) / 127.0
    });

    NodeFeatureVec {
        values: vec![note_density, avg_velocity_norm, pitch_span_norm],
    }
}

// ── node types ─────────────────────────────────────────────────────────────────

/// A corpus phrase node.
#[derive(Debug, Clone)]
pub struct PhraseNode {
    /// Human-readable identifier (e.g. chunk id or a bar range).
    pub label: String,
    /// Feature vector used for similarity edges.
    pub features: NodeFeatureVec,
}

/// A short pitch sequence (motif) node.
#[derive(Debug, Clone)]
pub struct MotifNode {
    /// Ordered MIDI pitches forming the motif.
    pub pitches: Vec<Pitch>,
}

/// A rhythm pattern (durations only, no pitches).
#[derive(Debug, Clone)]
pub struct RhythmCellNode {
    /// Ordered note durations in ticks.
    pub durations: Vec<Ticks>,
}

/// A chord root-movement node (e.g. ascending fourth = +5).
#[derive(Debug, Clone, Copy)]
pub struct ChordMovementNode {
    /// Movement in semitones (positive = up, negative = down).
    pub root_movement: i8,
}

/// An energy-level node representing the intensity of a section.
#[derive(Debug, Clone, Copy)]
pub struct EnergyStateNode {
    /// Normalised energy in `[0.0, 1.0]` (0 = calm, 1 = peak).
    pub energy: f64,
}

/// A node in the phrase graph; one of the five musical node types.
// The variants intentionally differ in size (some heap-allocating, some not).
#[allow(variant_size_differences)]
#[derive(Debug, Clone)]
pub enum GraphNode {
    /// A corpus phrase.
    Phrase(PhraseNode),
    /// A short pitch motif.
    Motif(MotifNode),
    /// A rhythm cell (durations only).
    RhythmCell(RhythmCellNode),
    /// A chord root-movement.
    ChordMovement(ChordMovementNode),
    /// An energy state.
    EnergyState(EnergyStateNode),
}

// ── edges ─────────────────────────────────────────────────────────────────────

/// How a graph edge was derived.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EdgeKind {
    /// Cosine similarity on feature vectors.
    Similarity,
    /// Transition counted from corpus co-occurrences.
    TransitionProbability,
    /// Tag co-occurrence from corpus metadata.
    TagCoOccurrence,
}

/// A directed, weighted edge between two graph nodes.
#[derive(Debug, Clone, Copy)]
pub struct GraphEdge {
    /// Source node.
    pub from: NodeId,
    /// Destination node.
    pub to: NodeId,
    /// Non-negative edge weight.
    pub weight: f64,
    /// How the edge was derived.
    pub kind: EdgeKind,
}

// ── graph ──────────────────────────────────────────────────────────────────────

/// Weighted directed graph over musical nodes.
#[derive(Debug, Clone, Default)]
pub struct PhraseGraph {
    /// All nodes; index into this Vec is the `NodeId`.
    pub nodes: Vec<GraphNode>,
    /// All directed edges (unsorted flat list).
    pub edges: Vec<GraphEdge>,
}

impl PhraseGraph {
    /// Creates an empty graph.
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends a node and returns its assigned [`NodeId`].
    pub fn add_node(&mut self, node: GraphNode) -> NodeId {
        let id = NodeId(self.nodes.len());
        self.nodes.push(node);
        id
    }

    /// Appends a directed edge.
    pub fn add_edge(&mut self, edge: GraphEdge) {
        self.edges.push(edge);
    }

    /// Number of nodes.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Number of edges.
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    /// Returns references to all edges whose `from` matches `node`.
    pub fn edges_from(&self, node: NodeId) -> Vec<&GraphEdge> {
        self.edges.iter().filter(|e| e.from.0 == node.0).collect()
    }
}

// ── traversal: random walk ────────────────────────────────────────────────────

/// Request for a weighted random walk on the graph.
#[derive(Debug, Clone, Copy)]
pub struct WalkRequest {
    /// Starting node.
    pub start: NodeId,
    /// Number of steps to take (result has `steps + 1` nodes).
    pub steps: usize,
    /// Deterministic PRNG seed.
    pub seed: GenerationSeed,
}

/// Performs a weighted random walk on `graph`, returning the chain of visited
/// [`NodeId`]s (including the start node).
///
/// At each step the next node is chosen proportional to outgoing edge weights.
/// If the current node has no outgoing edges the walk stays put.
pub fn random_walk(graph: &PhraseGraph, req: &WalkRequest) -> Vec<NodeId> {
    let mut prng = WalkPrng::new(req.seed.0);
    let mut chain = Vec::with_capacity(req.steps.saturating_add(1));
    chain.push(req.start);

    let mut current = req.start;
    for _ in 0..req.steps {
        let out = graph.edges_from(current);
        if out.is_empty() {
            chain.push(current);
            continue;
        }
        current = weighted_pick(&out, &mut prng);
        chain.push(current);
    }
    chain
}

// ── traversal: beam search ────────────────────────────────────────────────────

/// Request for a beam search over the graph.
#[derive(Debug, Clone, Copy)]
pub struct BeamSearchRequest {
    /// Starting node.
    pub start: NodeId,
    /// Number of expansion steps (each path has `steps + 1` nodes).
    pub steps: usize,
    /// Maximum number of candidates kept after each step.
    pub beam_width: usize,
    /// Deterministic PRNG seed (used for tie-breaking).
    pub seed: GenerationSeed,
}

/// A single candidate path produced by beam search.
#[derive(Debug, Clone, PartialEq)]
pub struct BeamCandidate {
    /// Sequence of node ids from start to current position.
    pub path: Vec<NodeId>,
    /// Cumulative edge-weight score along the path.
    pub score: f64,
}

/// Runs a greedy beam search from `req.start`, returning up to `req.beam_width`
/// highest-scoring paths of length `req.steps + 1`.
pub fn beam_search(graph: &PhraseGraph, req: &BeamSearchRequest) -> Vec<BeamCandidate> {
    if req.beam_width == 0 {
        return Vec::new();
    }

    let initial = BeamCandidate {
        path: vec![req.start],
        score: 0.0,
    };
    let mut beam: Vec<BeamCandidate> = vec![initial];

    for _ in 0..req.steps {
        let mut next_beam: Vec<BeamCandidate> = Vec::new();

        for candidate in &beam {
            let current = candidate.path.last().copied().unwrap_or(req.start);
            let out = graph.edges_from(current);

            if out.is_empty() {
                // No outgoing edges: extend path with the current node (stay).
                let mut extended = candidate.clone();
                extended.path.push(current);
                next_beam.push(extended);
            } else {
                for edge in &out {
                    let new_score = candidate.score + edge.weight;
                    let mut new_path = candidate.path.clone();
                    new_path.push(edge.to);
                    next_beam.push(BeamCandidate {
                        path: new_path,
                        score: new_score,
                    });
                }
            }
        }

        // Sort by score descending, then keep top beam_width.
        next_beam.sort_unstable_by(|a, b| b.score.total_cmp(&a.score));
        next_beam.truncate(req.beam_width);
        beam = next_beam;
    }

    beam
}

// ── private PRNG ───────────────────────────────────────────────────────────────

/// Xorshift64 PRNG for deterministic traversal.
struct WalkPrng(u64);

impl WalkPrng {
    const fn new(seed: u64) -> Self {
        Self(if seed == 0 {
            6_364_136_223_846_793_005_u64
        } else {
            seed
        })
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x.wrapping_shl(13);
        x ^= x.wrapping_shr(7);
        x ^= x.wrapping_shl(17);
        self.0 = x;
        x
    }

    fn next_f64(&mut self) -> f64 {
        // Map u64 to [0.0, 1.0).
        // Shift right by 11 to get a 53-bit mantissa, divide by 2^53.
        let mantissa = self.next_u64().wrapping_shr(11);
        // 9_007_199_254_740_992.0 = 2^53
        f64::from_bits(0x3FF0_0000_0000_0000_u64 | mantissa) - 1.0
    }
}

/// Picks a destination node from `edges` proportional to edge weights.
///
/// Precondition: `edges` is non-empty.
fn weighted_pick(edges: &[&GraphEdge], prng: &mut WalkPrng) -> NodeId {
    let total: f64 = edges.iter().map(|e| e.weight.max(0.0)).sum();
    let fallback = edges.first().map_or(NodeId(0), |e| e.to);
    if total <= 0.0 {
        // All weights ≤ 0: fall back to uniform pick.
        let n64 = u64::try_from(edges.len()).unwrap_or(u64::MAX);
        let idx = prng.next_u64().checked_rem(n64).unwrap_or(0);
        return edges
            .get(usize::try_from(idx).unwrap_or(0))
            .map_or(fallback, |e| e.to);
    }
    let threshold = prng.next_f64() * total;
    let mut cumulative = 0.0_f64;
    for edge in edges {
        cumulative += edge.weight.max(0.0);
        if cumulative > threshold {
            return edge.to;
        }
    }
    // Fallback to last edge (handles floating-point rounding).
    edges.last().map_or(fallback, |e| e.to)
}
