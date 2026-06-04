#![no_main]

//! Fuzz target: structure-aware score → structure-metrics pass (P2, ADR-0010 / S14).
//!
//! Builds a typed `Score` from `arbitrary` input and runs `measure_structure`.
//!
//! Oracle (normalised invariants):
//!   * No panic / hang / unbounded allocation (libFuzzer limits).
//!   * On `Ok(m)`:
//!     - `m.bar_count == score.master_bars.len()`.
//!     - every score is a finite value in `[0, 1]`.
//!     - `variation_score == 1.0 - repeatability_score`.
//!     - period bars and ticks are `Some` together or `None` together; when
//!       `Some`, the period is in `1..=bar_count/2`.
//!   * On `Err(_)`: a declared typed error — no panic.

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;

use griff_core::{
    event::{Pitch, Tempo, Ticks, TimeSignature, Tuning, Velocity},
    score::{
        AtomEvent, AtomNote, EventGroup, EventGroupKind, LossReport, MasterBar, Score, Track, Voice,
    },
    slice::TickRange,
    structure::measure_structure,
};

#[derive(Debug, Arbitrary)]
struct FuzzInput {
    /// Mapped to 1..=8 bars.
    bar_count: u8,
    /// Mapped to 1..=960 ticks per quarter.
    ppqn: u16,
    /// Pitches laid out as quarter notes, round-robin across the bars.
    pitches: Vec<u8>,
}

fn build_score(bar_count: usize, ppqn: u16, pitches: &[u8]) -> Option<Score> {
    let quarter = u32::from(ppqn);
    let bar = quarter.checked_mul(4)?;

    let mut master_bars = Vec::with_capacity(bar_count);
    for i in 0..bar_count {
        let start = u32::try_from(i).ok()?.checked_mul(bar)?;
        let end = start.checked_add(bar)?;
        master_bars.push(MasterBar {
            index: i,
            tick_range: TickRange::new(Ticks(start), Ticks(end)).ok()?,
            time_signature: TimeSignature::new(4, 4).ok()?,
            tempo: Tempo::new(120.0).ok()?,
        });
    }

    let velocity = Velocity::new(90).ok()?;
    let mut groups = Vec::new();
    // Up to 4 quarter notes per bar; pitches consumed round-robin.
    for (idx, &p) in pitches.iter().take(bar_count.checked_mul(4)?).enumerate() {
        let bar_idx = idx.checked_div(4)?;
        let slot = idx.checked_rem(4)?;
        let bar_start = u32::try_from(bar_idx).ok()?.checked_mul(bar)?;
        let onset = bar_start.checked_add(u32::try_from(slot).ok()?.checked_mul(quarter)?)?;
        groups.push(EventGroup {
            kind: EventGroupKind::Single,
            atoms: vec![AtomEvent::Note(AtomNote {
                absolute_start: Ticks(onset),
                duration: Ticks(quarter),
                pitch: Pitch::new(p.min(127)).ok()?,
                velocity,
                articulation: None,
                position: None,
            })],
            technique_spans: Vec::new(),
        });
    }

    Some(Score {
        ticks_per_quarter: ppqn,
        master_bars,
        tracks: vec![Track {
            name: None,
            channel: 0,
            voices: vec![Voice {
                id: 0,
                event_groups: groups,
            }],
            tuning: Tuning::standard_e(),
        }],
        source_meta: None,
        loss: LossReport::new(),
    })
}

fn in_unit(x: f64) -> bool {
    x.is_finite() && (0.0..=1.0).contains(&x)
}

fuzz_target!(|input: FuzzInput| {
    let bar_count = usize::from(input.bar_count % 8) + 1;
    let ppqn = input.ppqn % 960 + 1;

    let Some(score) = build_score(bar_count, ppqn, &input.pitches) else {
        return;
    };

    let Ok(m) = measure_structure(&score, 0) else {
        return; // typed error — oracle satisfied.
    };

    assert_eq!(
        m.bar_count,
        score.master_bars.len(),
        "bar_count must match the score",
    );

    for (name, v) in [
        ("repeatability", m.repeatability_score),
        ("variation", m.variation_score),
        ("loopability", m.loopability_score),
        ("structural_complexity", m.structural_complexity),
    ] {
        assert!(in_unit(v), "{name} out of [0,1]: {v}");
    }

    assert_eq!(
        m.variation_score,
        1.0 - m.repeatability_score,
        "variation must be the complement of repeatability",
    );

    assert_eq!(
        m.detected_pattern_period_bars.is_some(),
        m.detected_pattern_period_ticks.is_some(),
        "period bars and ticks must agree on presence",
    );

    if let Some(period) = m.detected_pattern_period_bars {
        assert!(
            period >= 1 && period <= m.bar_count / 2,
            "period {period} must be in 1..=bar_count/2 ({})",
            m.bar_count / 2,
        );
    }
});
