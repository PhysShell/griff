// TDD red phase for S9 human-feedback layer.
// Fails to compile until `griff_core::feedback` is implemented.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::missing_assert_message,
    clippy::absolute_paths,
    clippy::missing_const_for_fn,
    clippy::float_cmp
)]

use griff_core::{
    feedback::{rerank, PreferenceProfile, Rating},
    graph::NodeFeatureVec,
};

// ── helpers ───────────────────────────────────────────────────────────────────

fn fv(values: Vec<f64>) -> NodeFeatureVec {
    NodeFeatureVec { values }
}

fn uniform3() -> PreferenceProfile {
    PreferenceProfile::new(3)
}

// ── construction ──────────────────────────────────────────────────────────────

#[test]
fn new_profile_uniform_weights_three_dim() {
    let p = uniform3();
    let expected = 1.0 / 3.0;
    for &w in &p.weights {
        assert!(
            (w - expected).abs() < 1e-10,
            "initial weights must be uniform 1/3, got {w}"
        );
    }
}

#[test]
fn new_profile_weights_sum_to_one() {
    let p = uniform3();
    let sum: f64 = p.weights.iter().sum();
    assert!(
        (sum - 1.0).abs() < 1e-10,
        "initial weights must sum to 1.0, got {sum}"
    );
}

#[test]
fn zero_dim_profile_has_empty_weights() {
    let p = PreferenceProfile::new(0);
    assert!(p.weights.is_empty(), "zero-dim profile must have empty weights");
}

#[test]
fn with_alpha_sets_learning_rate() {
    let p = PreferenceProfile::with_alpha(3, 0.25);
    assert!(
        (p.alpha - 0.25).abs() < 1e-10,
        "with_alpha must store the given alpha"
    );
}

// ── update: Like ──────────────────────────────────────────────────────────────

#[test]
fn like_increases_weight_for_high_feature() {
    let mut p = uniform3();
    let before = p.weights[0];
    p.update(&fv(vec![1.0, 0.0, 0.0]), Rating::Like);
    assert!(
        p.weights[0] > before,
        "Like on [1,0,0] must increase weights[0]"
    );
}

#[test]
fn weights_sum_to_one_after_like() {
    let mut p = uniform3();
    p.update(&fv(vec![0.8, 0.6, 0.4]), Rating::Like);
    let sum: f64 = p.weights.iter().sum();
    assert!(
        (sum - 1.0).abs() < 1e-10,
        "weights must sum to 1.0 after Like, got {sum}"
    );
}

// ── update: Dislike ───────────────────────────────────────────────────────────

#[test]
fn dislike_decreases_weight_for_high_feature() {
    let mut p = uniform3();
    let before = p.weights[0];
    p.update(&fv(vec![1.0, 0.0, 0.0]), Rating::Dislike);
    assert!(
        p.weights[0] < before,
        "Dislike on [1,0,0] must decrease weights[0]"
    );
}

#[test]
fn weights_sum_to_one_after_dislike() {
    let mut p = uniform3();
    p.update(&fv(vec![0.8, 0.1, 0.1]), Rating::Dislike);
    let sum: f64 = p.weights.iter().sum();
    assert!(
        (sum - 1.0).abs() < 1e-10,
        "weights must sum to 1.0 after Dislike, got {sum}"
    );
}

#[test]
fn all_weights_nonneg_after_dislike() {
    let mut p = uniform3();
    p.update(&fv(vec![1.0, 0.0, 0.0]), Rating::Dislike);
    for &w in &p.weights {
        assert!(w >= 0.0, "weights must be non-negative after Dislike, got {w}");
    }
}

#[test]
fn extreme_dislike_resets_to_uniform() {
    // alpha=1.0 means complement=0, so all weights ← sign*feature;
    // disliking [1,0,0] drives w[0]←−1 (clamped to 0) and w[1]=w[2]←0.
    // All-zero → should recover to uniform rather than staying at zero.
    let mut p = PreferenceProfile::with_alpha(3, 1.0);
    p.update(&fv(vec![1.0, 0.0, 0.0]), Rating::Dislike);
    let sum: f64 = p.weights.iter().sum();
    assert!(
        (sum - 1.0).abs() < 1e-10,
        "extreme dislike must not leave all-zero weights; sum={sum}"
    );
}

// ── update: Favorite ──────────────────────────────────────────────────────────

#[test]
fn favorite_shifts_more_than_like() {
    let feature = fv(vec![1.0, 0.0, 0.0]);

    let mut p_like = uniform3();
    p_like.update(&feature, Rating::Like);
    let w0_like = p_like.weights[0];

    let mut p_fav = uniform3();
    p_fav.update(&feature, Rating::Favorite);
    let w0_fav = p_fav.weights[0];

    assert!(
        w0_fav > w0_like,
        "Favorite must shift weights more than Like: fav={w0_fav}, like={w0_like}"
    );
}

// ── reset ─────────────────────────────────────────────────────────────────────

#[test]
fn reset_after_updates_restores_uniform() {
    let mut p = uniform3();
    p.update(&fv(vec![1.0, 0.0, 0.0]), Rating::Like);
    p.update(&fv(vec![0.0, 1.0, 0.0]), Rating::Dislike);
    p.reset();
    let expected = 1.0 / 3.0;
    for &w in &p.weights {
        assert!(
            (w - expected).abs() < 1e-10,
            "reset must restore uniform weights, got {w}"
        );
    }
}

#[test]
fn reset_preserves_dim_and_alpha() {
    let mut p = PreferenceProfile::with_alpha(5, 0.2);
    p.update(&fv(vec![1.0, 0.5, 0.2, 0.0, 0.0]), Rating::Like);
    p.reset();
    assert_eq!(p.weights.len(), 5, "reset must preserve dimension");
    assert!(
        (p.alpha - 0.2).abs() < 1e-10,
        "reset must preserve alpha"
    );
}

// ── score ─────────────────────────────────────────────────────────────────────

#[test]
fn score_is_dot_product_of_weights_and_features() {
    let mut p = PreferenceProfile::new(3);
    // Override weights directly to a known value (they're pub).
    p.weights = vec![0.5, 0.3, 0.2];
    // score = 0.5*1 + 0.3*1 + 0.2*1 = 1.0
    let s = p.score(&fv(vec![1.0, 1.0, 1.0]));
    assert!(
        (s - 1.0).abs() < 1e-10,
        "score must equal dot product: expected 1.0, got {s}"
    );
}

#[test]
fn like_increases_score_for_liked_features() {
    let feature = fv(vec![0.9, 0.1, 0.1]);
    let mut p = uniform3();
    let before = p.score(&feature);
    p.update(&feature, Rating::Like);
    let after = p.score(&feature);
    assert!(
        after > before,
        "Like must increase score for those features: before={before}, after={after}"
    );
}

#[test]
fn dislike_decreases_score_for_disliked_features() {
    let feature = fv(vec![0.9, 0.1, 0.1]);
    let mut p = uniform3();
    let before = p.score(&feature);
    p.update(&feature, Rating::Dislike);
    let after = p.score(&feature);
    assert!(
        after < before,
        "Dislike must decrease score for those features: before={before}, after={after}"
    );
}

#[test]
fn multiple_likes_accumulate_toward_liked_features() {
    let feature = fv(vec![1.0, 0.0, 0.0]);
    let mut p = uniform3();
    let initial = p.weights[0];
    p.update(&feature, Rating::Like);
    let after_one = p.weights[0];
    p.update(&feature, Rating::Like);
    let after_two = p.weights[0];
    assert!(
        after_one > initial,
        "first Like must increase weights[0]"
    );
    assert!(
        after_two > after_one,
        "second Like must increase weights[0] further"
    );
}

// ── rerank ────────────────────────────────────────────────────────────────────

#[test]
fn rerank_empty_slice_returns_empty() {
    let p = uniform3();
    let result = rerank(&[], &p);
    assert!(result.is_empty(), "rerank of empty slice must return empty vec");
}

#[test]
fn rerank_single_element_returns_zero() {
    let p = uniform3();
    let result = rerank(&[fv(vec![0.5, 0.5, 0.5])], &p);
    assert_eq!(result, vec![0], "single-element rerank must return [0]");
}

#[test]
fn rerank_returns_all_indices() {
    let p = uniform3();
    let fvs = vec![
        fv(vec![0.3, 0.3, 0.3]),
        fv(vec![0.6, 0.2, 0.2]),
        fv(vec![0.1, 0.8, 0.1]),
    ];
    let mut result = rerank(&fvs, &p);
    result.sort_unstable();
    assert_eq!(result, vec![0, 1, 2], "rerank must return all indices exactly once");
}

#[test]
fn rerank_orders_by_descending_score() {
    // Weights heavily biased toward first dimension.
    let mut p = PreferenceProfile::new(3);
    p.weights = vec![0.8, 0.1, 0.1];

    let fvs = vec![
        fv(vec![0.1, 0.9, 0.9]), // score ≈ 0.08+0.09+0.09=0.26 (lowest)
        fv(vec![0.5, 0.5, 0.5]), // score = 0.8*0.5+0.1*0.5+0.1*0.5 = 0.5 (mid)
        fv(vec![0.9, 0.1, 0.1]), // score ≈ 0.72+0.01+0.01=0.74 (highest)
    ];
    let order = rerank(&fvs, &p);
    assert_eq!(order[0], 2, "highest-scoring candidate must rank first");
    assert_eq!(order[1], 1, "mid-scoring candidate must rank second");
    assert_eq!(order[2], 0, "lowest-scoring candidate must rank last");
}
