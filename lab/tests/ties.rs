//! Red → contract tests for the optimum-set analysis and the learned
//! secondary objective (`ties`).
//!
//! Pins, against brute force on exhaustive small families: the chain mirrors
//! the production `v1` objective; the optimum set's optimum, exact path count,
//! and least / most / expected agreement with a reference; the lexicographic
//! DP is primary- then secondary-optimal and, with zero secondary weights,
//! reproduces the production DP path exactly; loss augmentation finds the
//! least-agreeing optimal path; counts saturate with an exact logarithm; and
//! the perceptron learns a separable tie-break deterministically.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::missing_assert_message,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::float_cmp,
    clippy::cast_possible_wrap
)]

use griff_constraint_lab::{
    fingering::v1_cost,
    problems::LabError,
    ties::{
        latent_target, lexicographic_path, optimum_set, path_features, path_matches,
        train_secondary, Chain, Example, Features, PerceptronConfig, FEATURES, FEATURE_NAMES,
    },
};
use griff_core::{
    event::{FretboardPosition, Pitch, Tuning},
    fretboard::{infer_positions, FingeringWeights, STANDARD_MAX_FRET},
};

fn pitch(p: u8) -> Pitch {
    Pitch::new(p).expect("valid pitch")
}

fn pitches_of(raw: &[u8]) -> Vec<Pitch> {
    raw.iter().map(|&p| pitch(p)).collect()
}

fn pos(string: u8, fret: u8) -> FretboardPosition {
    FretboardPosition { string, fret }
}

fn weights(
    fret: i64,
    open_string: i64,
    position_shift: i64,
    string_change: i64,
) -> FingeringWeights {
    FingeringWeights {
        fret,
        open_string,
        position_shift,
        string_change,
    }
}

fn weight_sets() -> Vec<FingeringWeights> {
    vec![
        FingeringWeights::v1(),
        weights(0, -3, 1, 0),
        weights(0, 0, 0, 0),
        weights(2, 4, 0, 3),
    ]
}

fn sequences(alphabet: &[u8], len: usize) -> Vec<Vec<u8>> {
    let mut out = vec![Vec::new()];
    for _ in 0..len {
        out = out
            .into_iter()
            .flat_map(|prefix| {
                alphabet.iter().map(move |&a| {
                    let mut next = prefix.clone();
                    next.push(a);
                    next
                })
            })
            .collect();
    }
    out
}

/// Every path of a chain, as candidate indices.
fn all_paths(chain: &Chain) -> Vec<Vec<usize>> {
    let mut out = vec![Vec::new()];
    for note in 0..chain.len() {
        let k = chain.candidates(note).len();
        out = out
            .into_iter()
            .flat_map(|prefix| {
                (0..k).map(move |c| {
                    let mut next = prefix.clone();
                    next.push(c);
                    next
                })
            })
            .collect();
    }
    out
}

fn lcg_lines(count: usize, len: usize, lo: u8, hi: u8) -> Vec<Vec<Pitch>> {
    let mut state: u64 = 0x2545_f491_4f6c_dd1d;
    let span = u64::from(hi - lo + 1);
    (0..count)
        .map(|_| {
            (0..len)
                .map(|_| {
                    state ^= state << 13;
                    state ^= state >> 7;
                    state ^= state << 17;
                    pitch(lo + (state % span) as u8)
                })
                .collect()
        })
        .collect()
}

fn dot(w: &Features, f: &Features) -> i128 {
    w.iter()
        .zip(f)
        .map(|(a, b)| i128::from(*a) * i128::from(*b))
        .sum()
}

const ALPHABET: [u8; 6] = [40, 47, 52, 57, 59, 64];

// ── chain ─────────────────────────────────────────────────────────────────────

#[test]
fn chain_v1_costs_every_path_like_v1_cost() {
    let tuning = Tuning::standard_e();
    for w in weight_sets() {
        for raw in sequences(&ALPHABET, 3) {
            let chain = Chain::v1(&pitches_of(&raw), &tuning, &w, STANDARD_MAX_FRET).unwrap();
            assert_eq!(chain.len(), 3);
            for path in all_paths(&chain) {
                let positions = chain.positions_of(&path).unwrap();
                assert_eq!(chain.cost(&path), Some(v1_cost(&positions, &w)));
            }
            assert_eq!(chain.cost(&[0, 0]), None);
            assert_eq!(chain.cost(&[0, 0, 99]), None);
        }
    }
}

#[test]
fn chain_candidates_follow_tuning_order() {
    let tuning = Tuning::standard_e();
    let p = pitches_of(&[52]);
    let chain = Chain::v1(&p, &tuning, &FingeringWeights::v1(), STANDARD_MAX_FRET).unwrap();
    assert_eq!(
        chain.candidates(0),
        tuning.candidates(p[0], STANDARD_MAX_FRET).as_slice()
    );
    assert!(chain.candidates(1).is_empty());
}

#[test]
fn chain_v1_refuses_empty_and_unpositionable_lines() {
    let tuning = Tuning::standard_e();
    let w = FingeringWeights::v1();
    assert_eq!(
        Chain::v1(&[], &tuning, &w, STANDARD_MAX_FRET),
        Err(LabError::EmptyLine)
    );
    assert_eq!(
        Chain::v1(&pitches_of(&[40, 30]), &tuning, &w, STANDARD_MAX_FRET),
        Err(LabError::UnpositionablePitch {
            index: 1,
            pitch: 30
        })
    );
}

// ── optimum set ───────────────────────────────────────────────────────────────

#[test]
fn optimum_set_matches_brute_force() {
    let tuning = Tuning::standard_e();
    for w in weight_sets() {
        for len in 1..=3 {
            for raw in sequences(&ALPHABET, len) {
                let chain = Chain::v1(&pitches_of(&raw), &tuning, &w, STANDARD_MAX_FRET).unwrap();
                let paths = all_paths(&chain);
                let costs: Vec<i64> = paths.iter().map(|p| chain.cost(p).unwrap()).collect();
                let optimum = *costs.iter().min().unwrap();
                let optimal: Vec<&Vec<usize>> = paths
                    .iter()
                    .zip(&costs)
                    .filter(|(_, c)| **c == optimum)
                    .map(|(p, _)| p)
                    .collect();

                let bare = optimum_set(&chain, None);
                assert_eq!(bare.optimum, optimum, "{raw:?} {w:?}");
                assert_eq!(bare.count.exact, optimal.len() as u64);
                assert!(!bare.count.saturated);
                assert!((bare.count.ln - (optimal.len() as f64).ln()).abs() < 1e-9);
                assert_eq!(bare.agreement, None);

                // Every path of the line doubles as a reference.
                for reference_path in &paths {
                    let reference = chain.positions_of(reference_path).unwrap();
                    let matches: Vec<usize> = optimal
                        .iter()
                        .map(|p| path_matches(&chain, p, &reference).unwrap())
                        .collect();
                    let set = optimum_set(&chain, Some(&reference));
                    let range = set.agreement.expect("reference of the chain's length");
                    assert_eq!(range.min, *matches.iter().min().unwrap());
                    assert_eq!(range.max, *matches.iter().max().unwrap());
                    let mean = matches.iter().sum::<usize>() as f64 / matches.len() as f64;
                    assert!((range.expected - mean).abs() < 1e-9);
                    assert_eq!(chain.cost(&range.best_path), Some(optimum));
                    assert_eq!(
                        path_matches(&chain, &range.best_path, &reference),
                        Some(range.max)
                    );
                }
                assert_eq!(
                    optimum_set(&chain, Some(&[pos(1, 0)][..0])).agreement,
                    None,
                    "a reference of another length gives no agreement"
                );
            }
        }
    }
}

#[test]
fn optimum_set_matches_brute_force_on_longer_lines() {
    let tuning = Tuning::standard_e();
    for w in weight_sets() {
        for pitches in lcg_lines(10, 6, 40, 70) {
            let chain = Chain::v1(&pitches, &tuning, &w, STANDARD_MAX_FRET).unwrap();
            let paths = all_paths(&chain);
            let reference = chain.positions_of(&paths[paths.len() / 2]).unwrap();
            let optimum = paths.iter().map(|p| chain.cost(p).unwrap()).min().unwrap();
            let matches: Vec<usize> = paths
                .iter()
                .filter(|p| chain.cost(p) == Some(optimum))
                .map(|p| path_matches(&chain, p, &reference).unwrap())
                .collect();
            let set = optimum_set(&chain, Some(&reference));
            assert_eq!(set.optimum, optimum);
            assert_eq!(set.count.exact, matches.len() as u64);
            let range = set.agreement.unwrap();
            assert_eq!(range.min, *matches.iter().min().unwrap());
            assert_eq!(range.max, *matches.iter().max().unwrap());
        }
    }
}

#[test]
fn path_counts_saturate_with_an_exact_logarithm() {
    // E3 has three candidates in Standard E; with zero weights every path ties.
    let tuning = Tuning::standard_e();
    let zero = weights(0, 0, 0, 0);
    let exact = Chain::v1(&[pitch(52); 30], &tuning, &zero, STANDARD_MAX_FRET).unwrap();
    let count = optimum_set(&exact, None).count;
    assert_eq!(count.exact, 3_u64.pow(30));
    assert!(!count.saturated);

    let huge = Chain::v1(&[pitch(52); 60], &tuning, &zero, STANDARD_MAX_FRET).unwrap();
    let count = optimum_set(&huge, None).count;
    assert!(count.saturated);
    assert_eq!(count.exact, u64::MAX);
    assert!((count.ln - 60.0 * 3.0_f64.ln()).abs() < 1e-9);
}

// ── lexicographic tie-break ───────────────────────────────────────────────────

fn production_path(
    chain: &Chain,
    pitches: &[Pitch],
    w: &FingeringWeights,
) -> Vec<FretboardPosition> {
    let _ = chain;
    infer_positions(pitches, &Tuning::standard_e(), w, STANDARD_MAX_FRET)
        .into_iter()
        .map(Option::unwrap)
        .collect()
}

#[test]
fn zero_secondary_reproduces_the_production_path() {
    let tuning = Tuning::standard_e();
    let zero: Features = [0; FEATURES];
    for w in weight_sets() {
        for raw in sequences(&ALPHABET, 3) {
            let pitches = pitches_of(&raw);
            let chain = Chain::v1(&pitches, &tuning, &w, STANDARD_MAX_FRET).unwrap();
            let path = lexicographic_path(&chain, &zero, None);
            assert_eq!(
                chain.positions_of(&path).unwrap(),
                production_path(&chain, &pitches, &w),
                "{raw:?} {w:?}"
            );
        }
        for pitches in lcg_lines(40, 24, 40, 76) {
            let chain = Chain::v1(&pitches, &tuning, &w, STANDARD_MAX_FRET).unwrap();
            let path = lexicographic_path(&chain, &zero, None);
            assert_eq!(
                chain.positions_of(&path).unwrap(),
                production_path(&chain, &pitches, &w)
            );
        }
    }
}

#[test]
fn lexicographic_path_is_primary_then_secondary_optimal() {
    let tuning = Tuning::standard_e();
    let mut secondary: Features = [0; FEATURES];
    for (i, w) in secondary.iter_mut().enumerate() {
        *w = (i as i64 % 7) - 3;
    }
    for w in weight_sets() {
        for raw in sequences(&ALPHABET, 3) {
            let chain = Chain::v1(&pitches_of(&raw), &tuning, &w, STANDARD_MAX_FRET).unwrap();
            let best = all_paths(&chain)
                .iter()
                .map(|p| {
                    (
                        chain.cost(p).unwrap(),
                        dot(&secondary, &path_features(&chain, p).unwrap()),
                    )
                })
                .min()
                .unwrap();
            let path = lexicographic_path(&chain, &secondary, None);
            assert_eq!(
                (
                    chain.cost(&path).unwrap(),
                    dot(&secondary, &path_features(&chain, &path).unwrap())
                ),
                best
            );
        }
    }
}

#[test]
fn loss_augmentation_finds_the_least_agreeing_optimal_path() {
    let tuning = Tuning::standard_e();
    let zero: Features = [0; FEATURES];
    for w in weight_sets() {
        for pitches in lcg_lines(30, 5, 40, 70) {
            let chain = Chain::v1(&pitches, &tuning, &w, STANDARD_MAX_FRET).unwrap();
            let reference = production_path(&chain, &pitches, &w);
            let set = optimum_set(&chain, Some(&reference));
            let path = lexicographic_path(&chain, &zero, Some((&reference, 1)));
            assert_eq!(chain.cost(&path), Some(set.optimum));
            assert_eq!(
                path_matches(&chain, &path, &reference),
                Some(set.agreement.unwrap().min)
            );
        }
    }
}

#[test]
fn latent_target_is_the_cheapest_most_agreeing_optimal_path() {
    let tuning = Tuning::standard_e();
    let mut secondary: Features = [0; FEATURES];
    for (i, w) in secondary.iter_mut().enumerate() {
        *w = 2 - (i as i64 % 5);
    }
    for w in weight_sets() {
        for raw in sequences(&ALPHABET, 3) {
            let chain = Chain::v1(&pitches_of(&raw), &tuning, &w, STANDARD_MAX_FRET).unwrap();
            let paths = all_paths(&chain);
            let optimum = paths.iter().map(|p| chain.cost(p).unwrap()).min().unwrap();
            for reference_path in paths.iter().step_by(3) {
                let reference = chain.positions_of(reference_path).unwrap();
                let best = paths
                    .iter()
                    .filter(|p| chain.cost(p) == Some(optimum))
                    .map(|p| {
                        (
                            std::cmp::Reverse(path_matches(&chain, p, &reference).unwrap()),
                            dot(&secondary, &path_features(&chain, p).unwrap()),
                        )
                    })
                    .min()
                    .unwrap();
                let target = latent_target(&chain, &secondary, &reference).unwrap();
                assert_eq!(chain.cost(&target), Some(optimum));
                assert_eq!(
                    (
                        std::cmp::Reverse(path_matches(&chain, &target, &reference).unwrap()),
                        dot(&secondary, &path_features(&chain, &target).unwrap())
                    ),
                    best,
                    "{raw:?} {w:?}"
                );
                assert_eq!(
                    latent_target(&chain, &[0; FEATURES], &reference),
                    optimum_set(&chain, Some(&reference))
                        .agreement
                        .map(|a| a.best_path),
                    "zero weights give the best_path"
                );
            }
            assert_eq!(latent_target(&chain, &secondary, &[]), None);
        }
    }
}

// ── features ──────────────────────────────────────────────────────────────────

fn feature(f: &Features, name: &str) -> i64 {
    f[FEATURE_NAMES
        .iter()
        .position(|n| *n == name)
        .expect("known feature")]
}

#[test]
fn path_features_are_summed_over_notes_and_transitions() {
    // E3 on (6,12), A3 on (5,12), E4 open on (1,0).
    let tuning = Tuning::standard_e();
    let pitches = pitches_of(&[52, 57, 64]);
    let chain = Chain::v1(
        &pitches,
        &tuning,
        &FingeringWeights::v1(),
        STANDARD_MAX_FRET,
    )
    .unwrap();
    let want = [pos(6, 12), pos(5, 12), pos(1, 0)];
    let path: Vec<usize> = want
        .iter()
        .enumerate()
        .map(|(i, p)| chain.candidates(i).iter().position(|c| c == p).unwrap())
        .collect();
    let f = path_features(&chain, &path).unwrap();
    assert_eq!(feature(&f, "fret"), 24);
    assert_eq!(feature(&f, "open"), 1);
    assert_eq!(feature(&f, "string_6"), 1);
    assert_eq!(feature(&f, "string_5"), 1);
    assert_eq!(feature(&f, "string_1"), 1);
    // (6,12)→(5,12): Δs −1, Δf 0.  (5,12)→(1,0): Δs −4, Δf −12, involves an open string.
    assert_eq!(feature(&f, "fret_distance"), 12);
    assert_eq!(feature(&f, "string_distance"), 5);
    assert_eq!(feature(&f, "string_change"), 2);
    assert_eq!(feature(&f, "same_fret"), 1);
    assert_eq!(feature(&f, "span_over_3"), 0);
    assert_eq!(feature(&f, "open_transition"), 1);
    assert_eq!(feature(&f, "diagonal"), 1);
    assert_eq!(feature(&f, "toward_high_string"), 2);
    assert_eq!(feature(&f, "fret_up"), 0);
    assert_eq!(feature(&f, "box_move"), 1);
    assert_eq!(path_features(&chain, &path[..2]), None);
}

#[test]
fn anchor_distance_sums_fretted_distance_to_the_anchor() {
    let tuning = Tuning::standard_e();
    let pitches = pitches_of(&[52, 57, 64]);
    let chain = Chain::v1(
        &pitches,
        &tuning,
        &FingeringWeights::v1(),
        STANDARD_MAX_FRET,
    )
    .unwrap();
    let want = [pos(6, 12), pos(5, 12), pos(1, 0)];
    let path: Vec<usize> = want
        .iter()
        .enumerate()
        .map(|(i, p)| chain.candidates(i).iter().position(|c| c == p).unwrap())
        .collect();
    assert_eq!(chain.anchor(), None);
    assert_eq!(
        feature(&path_features(&chain, &path).unwrap(), "anchor_distance"),
        0
    );
    let anchored = chain.clone().with_anchor(Some(10));
    assert_eq!(anchored.anchor(), Some(10));
    // |12 − 10| + |12 − 10|; the open string does not count.
    assert_eq!(
        feature(&path_features(&anchored, &path).unwrap(), "anchor_distance"),
        4
    );
    assert_eq!(anchored.cost(&path), chain.cost(&path), "primary unchanged");
}

#[test]
fn anchored_lexicographic_path_is_secondary_optimal() {
    let tuning = Tuning::standard_e();
    let mut secondary: Features = [0; FEATURES];
    secondary[FEATURE_NAMES
        .iter()
        .position(|n| *n == "anchor_distance")
        .unwrap()] = 3;
    secondary[0] = -1;
    for w in weight_sets() {
        for raw in sequences(&ALPHABET, 3) {
            for anchor in [1, 7, 15] {
                let chain = Chain::v1(&pitches_of(&raw), &tuning, &w, STANDARD_MAX_FRET)
                    .unwrap()
                    .with_anchor(Some(anchor));
                let best = all_paths(&chain)
                    .iter()
                    .map(|p| {
                        (
                            chain.cost(p).unwrap(),
                            dot(&secondary, &path_features(&chain, p).unwrap()),
                        )
                    })
                    .min()
                    .unwrap();
                let path = lexicographic_path(&chain, &secondary, None);
                assert_eq!(
                    (
                        chain.cost(&path).unwrap(),
                        dot(&secondary, &path_features(&chain, &path).unwrap())
                    ),
                    best
                );
            }
        }
    }
}

#[test]
fn path_matches_counts_equal_positions() {
    let tuning = Tuning::standard_e();
    let chain = Chain::v1(
        &pitches_of(&[52, 57]),
        &tuning,
        &FingeringWeights::v1(),
        STANDARD_MAX_FRET,
    )
    .unwrap();
    let path = lexicographic_path(&chain, &[0; FEATURES], None);
    let positions = chain.positions_of(&path).unwrap();
    assert_eq!(path_matches(&chain, &path, &positions), Some(2));
    assert_eq!(path_matches(&chain, &path, &positions[..1]), None);
}

// ── perceptron ────────────────────────────────────────────────────────────────

/// Zero primary weights make every fingering tie; the "tab author" always
/// takes the highest-numbered (lowest-pitched) string — separable by the
/// string one-hot features.
fn separable_examples(lines: &[Vec<Pitch>]) -> Vec<Example> {
    let tuning = Tuning::standard_e();
    lines
        .iter()
        .map(|pitches| {
            let chain =
                Chain::v1(pitches, &tuning, &weights(0, 0, 0, 0), STANDARD_MAX_FRET).unwrap();
            let human = (0..chain.len())
                .map(|i| *chain.candidates(i).iter().max_by_key(|c| c.string).unwrap())
                .collect();
            Example { chain, human }
        })
        .collect()
}

#[test]
fn perceptron_learns_a_separable_tie_break() {
    let train = separable_examples(&lcg_lines(40, 8, 45, 64));
    let config = PerceptronConfig {
        epochs: 20,
        margin: 1,
    };
    let trained = train_secondary(&train, &config);
    assert!(trained.updates > 0);
    assert!(
        trained.epochs < config.epochs,
        "separable data converges: an epoch without updates"
    );
    let (mut agree, mut notes) = (0, 0);
    for ex in separable_examples(&lcg_lines(10, 12, 45, 64)) {
        let path = lexicographic_path(&ex.chain, &trained.weights, None);
        agree += path_matches(&ex.chain, &path, &ex.human).unwrap();
        notes += ex.chain.len();
    }
    assert!(
        agree * 10 >= notes * 9,
        "held-out agreement {agree}/{notes} below 90%"
    );
    assert_eq!(train_secondary(&train, &config), trained, "deterministic");
}

/// Zero primary weights; each line has its own anchor, and the "author" plays
/// every note at the candidate nearest that anchor.
fn anchored_examples(lines: &[Vec<Pitch>], offset: usize) -> Vec<Example> {
    let tuning = Tuning::standard_e();
    lines
        .iter()
        .enumerate()
        .map(|(i, pitches)| {
            let anchor = 3 + ((i + offset) % 13) as u8;
            let chain = Chain::v1(pitches, &tuning, &weights(0, 0, 0, 0), STANDARD_MAX_FRET)
                .unwrap()
                .with_anchor(Some(anchor));
            let human = (0..chain.len())
                .map(|n| {
                    *chain
                        .candidates(n)
                        .iter()
                        .filter(|c| c.fret > 0)
                        .min_by_key(|c| (c.fret.abs_diff(anchor), c.string))
                        .unwrap()
                })
                .collect();
            Example { chain, human }
        })
        .collect()
}

#[test]
fn perceptron_learns_an_anchor_tie_break() {
    let train = anchored_examples(&lcg_lines(60, 8, 45, 64), 0);
    let trained = train_secondary(
        &train,
        &PerceptronConfig {
            epochs: 30,
            margin: 1,
        },
    );
    let (mut agree, mut notes) = (0, 0);
    for ex in anchored_examples(&lcg_lines(12, 10, 45, 64), 5) {
        let path = lexicographic_path(&ex.chain, &trained.weights, None);
        agree += path_matches(&ex.chain, &path, &ex.human).unwrap();
        notes += ex.chain.len();
    }
    assert!(
        agree * 10 >= notes * 9,
        "held-out anchored agreement {agree}/{notes} below 90%"
    );
}

#[test]
fn perceptron_makes_no_update_when_the_target_is_already_chosen() {
    // With the production tie-break as the "author", zero weights already agree.
    let tuning = Tuning::standard_e();
    let w = weights(0, -3, 1, 0);
    let examples: Vec<Example> = lcg_lines(20, 8, 40, 70)
        .into_iter()
        .map(|pitches| {
            let chain = Chain::v1(&pitches, &tuning, &w, STANDARD_MAX_FRET).unwrap();
            let human = production_path(&chain, &pitches, &w);
            Example { chain, human }
        })
        .collect();
    let trained = train_secondary(
        &examples,
        &PerceptronConfig {
            epochs: 5,
            margin: 0,
        },
    );
    assert_eq!(trained.updates, 0);
    assert_eq!(trained.epochs, 1);
    assert_eq!(trained.weights, [0; FEATURES]);
}
