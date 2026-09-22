//! Contract tests for deterministic diagnostic-only corpus summaries.

#![allow(clippy::unwrap_used, clippy::missing_assert_message)]

use griff_constraint_lab::forensics::{distribution, top_n_longest, ExactRatio};

#[test]
fn nearest_rank_distribution_is_hand_computable_and_exact() {
    let summary = distribution(&[10_u64, 1, 9, 2, 8, 3, 7, 4, 6, 5]).unwrap();
    assert_eq!(summary.count, 10);
    assert_eq!(summary.min, 1);
    assert_eq!(summary.median, 5);
    assert_eq!(summary.p90, 9);
    assert_eq!(summary.p95, 10);
    assert_eq!(summary.p99, 10);
    assert_eq!(summary.max, 10);

    let quarters = distribution(&[
        ExactRatio::new(3, 2),
        ExactRatio::new(1, 4),
        ExactRatio::new(1, 1),
    ])
    .unwrap();
    assert_eq!(quarters.min, ExactRatio::new(1, 4));
    assert_eq!(quarters.median, ExactRatio::new(1, 1));
    assert_eq!(quarters.max, ExactRatio::new(3, 2));
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Case {
    gap: u32,
    identity: (&'static str, usize, u8, u32, usize),
}

#[test]
fn longest_records_use_identity_order_to_break_ties() {
    let cases = vec![
        Case { gap: 7, identity: ("b", 0, 0, 1, 1) },
        Case { gap: 9, identity: ("z", 0, 0, 1, 1) },
        Case { gap: 7, identity: ("a", 2, 0, 3, 4) },
        Case { gap: 7, identity: ("a", 1, 0, 3, 4) },
    ];
    let top = top_n_longest(cases, 3, |case| case.gap, |case| case.identity);
    assert_eq!(
        top,
        vec![
            Case { gap: 9, identity: ("z", 0, 0, 1, 1) },
            Case { gap: 7, identity: ("a", 1, 0, 3, 4) },
            Case { gap: 7, identity: ("a", 2, 0, 3, 4) },
        ]
    );
}
