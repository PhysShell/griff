// TDD red phase for S7 graph layer.
// Fails to compile until `griff_core::graph` is implemented.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::missing_assert_message,
    clippy::absolute_paths,
    clippy::missing_const_for_fn
)]

use griff_core::{
    feature::PhraseFeatures,
    generate::GenerationSeed,
    graph::{
        beam_search, cosine_similarity, phrase_features_to_vec, random_walk, BeamSearchRequest,
        ChordMovementNode, EdgeKind, EnergyStateNode, GraphEdge, GraphNode, MotifNode,
        NodeFeatureVec, NodeId, PhraseGraph, PhraseNode, RhythmCellNode, WalkRequest,
    },
};
use griff_core::event::{Pitch, Ticks};

// ── helpers ───────────────────────────────────────────────────────────────────

fn feature_vec(v: Vec<f64>) -> NodeFeatureVec {
    NodeFeatureVec { values: v }
}

fn phrase_node(label: &str) -> GraphNode {
    GraphNode::Phrase(PhraseNode {
        label: label.to_owned(),
        features: feature_vec(vec![1.0, 0.0, 0.0]),
    })
}

/// Triangle graph: 3 phrase nodes, edges forming a cycle with varying weights.
///
/// ```text
/// 0 --1.0--> 1
/// 0 --2.0--> 2
/// 1 --1.0--> 2
/// 2 --1.0--> 0
/// ```
fn triangle_graph() -> PhraseGraph {
    let mut g = PhraseGraph::new();
    g.add_node(phrase_node("A"));
    g.add_node(phrase_node("B"));
    g.add_node(phrase_node("C"));
    g.add_edge(GraphEdge {
        from: NodeId(0),
        to: NodeId(1),
        weight: 1.0,
        kind: EdgeKind::Similarity,
    });
    g.add_edge(GraphEdge {
        from: NodeId(0),
        to: NodeId(2),
        weight: 2.0,
        kind: EdgeKind::Similarity,
    });
    g.add_edge(GraphEdge {
        from: NodeId(1),
        to: NodeId(2),
        weight: 1.0,
        kind: EdgeKind::TransitionProbability,
    });
    g.add_edge(GraphEdge {
        from: NodeId(2),
        to: NodeId(0),
        weight: 1.0,
        kind: EdgeKind::TagCoOccurrence,
    });
    g
}

// ── PhraseGraph construction ──────────────────────────────────────────────────

#[test]
fn new_graph_is_empty() {
    let g = PhraseGraph::new();
    assert_eq!(g.node_count(), 0);
    assert_eq!(g.edge_count(), 0);
}

#[test]
fn add_node_returns_sequential_ids() {
    let mut g = PhraseGraph::new();
    let id0 = g.add_node(phrase_node("A"));
    let id1 = g.add_node(phrase_node("B"));
    assert_eq!(id0.0, 0, "first node must have id 0");
    assert_eq!(id1.0, 1, "second node must have id 1");
    assert_eq!(g.node_count(), 2);
}

#[test]
fn add_edge_increments_edge_count() {
    let mut g = PhraseGraph::new();
    g.add_node(phrase_node("A"));
    g.add_node(phrase_node("B"));
    g.add_edge(GraphEdge {
        from: NodeId(0),
        to: NodeId(1),
        weight: 1.0,
        kind: EdgeKind::Similarity,
    });
    assert_eq!(g.edge_count(), 1);
}

#[test]
fn edges_from_returns_correct_outgoing_edges() {
    let g = triangle_graph();
    let out0 = g.edges_from(NodeId(0));
    let out1 = g.edges_from(NodeId(1));
    let out2 = g.edges_from(NodeId(2));
    assert_eq!(out0.len(), 2, "node 0 has 2 outgoing edges");
    assert_eq!(out1.len(), 1, "node 1 has 1 outgoing edge");
    assert_eq!(out2.len(), 1, "node 2 has 1 outgoing edge");
}

#[test]
fn all_node_types_can_be_inserted() {
    let mut g = PhraseGraph::new();
    g.add_node(GraphNode::Phrase(PhraseNode {
        label: "phrase".to_owned(),
        features: feature_vec(vec![1.0]),
    }));
    g.add_node(GraphNode::Motif(MotifNode {
        pitches: vec![Pitch(60), Pitch(62)],
    }));
    g.add_node(GraphNode::RhythmCell(RhythmCellNode {
        durations: vec![Ticks(480), Ticks(240)],
    }));
    g.add_node(GraphNode::ChordMovement(ChordMovementNode {
        root_movement: 5,
    }));
    g.add_node(GraphNode::EnergyState(EnergyStateNode { energy: 0.8 }));
    assert_eq!(g.node_count(), 5);
}

// ── cosine_similarity ─────────────────────────────────────────────────────────

#[test]
fn cosine_identical_vectors_returns_one() {
    let a = feature_vec(vec![1.0, 0.0, 0.0]);
    let b = feature_vec(vec![1.0, 0.0, 0.0]);
    let s = cosine_similarity(&a, &b);
    assert!((s - 1.0).abs() < 1e-10, "identical vectors: similarity must be 1.0, got {s}");
}

#[test]
fn cosine_orthogonal_vectors_returns_zero() {
    let a = feature_vec(vec![1.0, 0.0]);
    let b = feature_vec(vec![0.0, 1.0]);
    let s = cosine_similarity(&a, &b);
    assert!(s.abs() < 1e-10, "orthogonal vectors: similarity must be 0.0, got {s}");
}

#[test]
fn cosine_antiparallel_vectors_returns_minus_one() {
    let a = feature_vec(vec![1.0, 0.0]);
    let b = feature_vec(vec![-1.0, 0.0]);
    let s = cosine_similarity(&a, &b);
    assert!((s + 1.0).abs() < 1e-10, "antiparallel vectors: similarity must be -1.0, got {s}");
}

#[test]
fn cosine_zero_vector_returns_zero() {
    let a = feature_vec(vec![0.0, 0.0]);
    let b = feature_vec(vec![1.0, 1.0]);
    let s = cosine_similarity(&a, &b);
    assert_eq!(s, 0.0, "zero magnitude vector: similarity must be 0.0");
}

#[test]
fn cosine_empty_vectors_returns_zero() {
    let a = feature_vec(vec![]);
    let b = feature_vec(vec![]);
    let s = cosine_similarity(&a, &b);
    assert_eq!(s, 0.0, "empty vectors: similarity must be 0.0");
}

// ── phrase_features_to_vec ────────────────────────────────────────────────────

#[test]
fn phrase_features_to_vec_produces_finite_values() {
    let feats = PhraseFeatures {
        bar_count: 2,
        event_count: 8,
        note_count: 8,
        rest_count: 0,
        articulated_note_count: 0,
        total_duration: Ticks(3840),
        pitch_range: Some(griff_core::feature::PitchRange {
            lowest: Pitch(40),
            highest: Pitch(52),
        }),
        velocity_range: Some(griff_core::feature::VelocityRange {
            lowest: griff_core::event::Velocity(80),
            highest: griff_core::event::Velocity(100),
        }),
    };
    let v = phrase_features_to_vec(&feats);
    assert!(!v.values.is_empty(), "feature vec must be non-empty");
    for &val in &v.values {
        assert!(val.is_finite(), "every feature value must be finite, got {val}");
        assert!(val >= 0.0, "every feature value must be non-negative, got {val}");
    }
}

// ── random_walk ───────────────────────────────────────────────────────────────

#[test]
fn walk_returns_steps_plus_one_nodes() {
    let g = triangle_graph();
    let walk = random_walk(
        &g,
        &WalkRequest {
            start: NodeId(0),
            steps: 4,
            seed: GenerationSeed(42),
        },
    );
    assert_eq!(walk.len(), 5, "walk of 4 steps must return start + 4 = 5 node ids");
}

#[test]
fn walk_starts_at_requested_node() {
    let g = triangle_graph();
    let walk = random_walk(
        &g,
        &WalkRequest {
            start: NodeId(2),
            steps: 3,
            seed: GenerationSeed(7),
        },
    );
    assert_eq!(walk[0].0, 2, "first element must be the start node");
}

#[test]
fn walk_all_ids_valid() {
    let g = triangle_graph();
    let walk = random_walk(
        &g,
        &WalkRequest {
            start: NodeId(0),
            steps: 10,
            seed: GenerationSeed(1),
        },
    );
    let n = g.node_count();
    for id in &walk {
        assert!(id.0 < n, "node id {} must be < node_count {n}", id.0);
    }
}

#[test]
fn walk_is_deterministic_under_same_seed() {
    let g = triangle_graph();
    let req = WalkRequest {
        start: NodeId(0),
        steps: 8,
        seed: GenerationSeed(42),
    };
    let a = random_walk(&g, &req);
    let b = random_walk(&g, &req);
    assert_eq!(a, b, "same seed must produce identical walk");
}

#[test]
fn walk_isolated_node_stays_put() {
    let mut g = PhraseGraph::new();
    g.add_node(phrase_node("isolated"));
    let walk = random_walk(
        &g,
        &WalkRequest {
            start: NodeId(0),
            steps: 3,
            seed: GenerationSeed(42),
        },
    );
    assert!(
        walk.iter().all(|id| id.0 == 0),
        "isolated node: every step must stay at node 0",
    );
}

#[test]
fn walk_zero_steps_returns_only_start() {
    let g = triangle_graph();
    let walk = random_walk(
        &g,
        &WalkRequest {
            start: NodeId(1),
            steps: 0,
            seed: GenerationSeed(0),
        },
    );
    assert_eq!(walk.len(), 1, "zero steps must return only the start node");
    assert_eq!(walk[0].0, 1);
}

// ── beam_search ───────────────────────────────────────────────────────────────

#[test]
fn beam_returns_up_to_beam_width_candidates() {
    let g = triangle_graph();
    let beams = beam_search(
        &g,
        &BeamSearchRequest {
            start: NodeId(0),
            steps: 3,
            beam_width: 2,
            seed: GenerationSeed(42),
        },
    );
    assert!(
        !beams.is_empty(),
        "beam search must return at least one candidate"
    );
    assert!(
        beams.len() <= 2,
        "beam search must return at most beam_width candidates, got {}",
        beams.len()
    );
}

#[test]
fn beam_each_path_has_correct_length() {
    let g = triangle_graph();
    let beams = beam_search(
        &g,
        &BeamSearchRequest {
            start: NodeId(0),
            steps: 3,
            beam_width: 2,
            seed: GenerationSeed(42),
        },
    );
    for beam in &beams {
        assert_eq!(
            beam.path.len(),
            4,
            "3-step beam path must have 4 nodes (start + 3 steps)"
        );
    }
}

#[test]
fn beam_is_deterministic_under_same_seed() {
    let g = triangle_graph();
    let req = BeamSearchRequest {
        start: NodeId(0),
        steps: 3,
        beam_width: 2,
        seed: GenerationSeed(42),
    };
    let a = beam_search(&g, &req);
    let b = beam_search(&g, &req);
    assert_eq!(a.len(), b.len(), "same seed must return same number of beams");
    for (ac, bc) in a.iter().zip(b.iter()) {
        assert_eq!(ac.path, bc.path, "same seed must produce identical paths");
        assert!(
            (ac.score - bc.score).abs() < 1e-10,
            "same seed must produce identical scores"
        );
    }
}

#[test]
fn beam_scores_are_finite_and_non_negative() {
    let g = triangle_graph();
    let beams = beam_search(
        &g,
        &BeamSearchRequest {
            start: NodeId(0),
            steps: 3,
            beam_width: 2,
            seed: GenerationSeed(42),
        },
    );
    for beam in &beams {
        assert!(beam.score.is_finite(), "beam score must be finite");
        assert!(beam.score >= 0.0, "beam score must be non-negative");
    }
}

#[test]
fn beam_zero_steps_returns_start_only() {
    let g = triangle_graph();
    let beams = beam_search(
        &g,
        &BeamSearchRequest {
            start: NodeId(1),
            steps: 0,
            beam_width: 2,
            seed: GenerationSeed(0),
        },
    );
    assert_eq!(beams.len(), 1, "zero steps returns one candidate");
    assert_eq!(beams[0].path.len(), 1, "zero-step path contains only the start");
    assert_eq!(beams[0].path[0].0, 1);
}
