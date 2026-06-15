#![no_main]

//! Fuzz target: structure-aware complement request → ComplementArranger (P2, ADR-0010 / S13).
//!
//! Builds a typed part-A `Score` plus a `ComplementSpec` and seed from
//! `arbitrary` input and runs `arrange_complement` (and `validate_pair` on any
//! produced pair).
//!
//! Oracle (normalised invariants):
//!   * No panic / hang / unbounded allocation (libFuzzer limits).
//!   * On `Ok(candidate)`:
//!     - B shares A's `ticks_per_quarter` and master-bar count.
//!     - Every note pitch of B lies in the register derived from A ± offset.
//!     - Fixed seed is deterministic: arranging twice yields the same B voice.
//!     - `validate_pair` over the produced (A, B) pair does not panic.
//!   * On `Err(_)`: the typed error is one of the declared variants — no panic.

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;

use griff_core::{
    complement::{analyze_part, arrange_complement, validate_pair, ComplementSpec, RelationMode},
    event::{NoteMarks, Pitch, Tempo, Ticks, TimeSignature, Tuning, Velocity},
    generate::GenerationSeed,
    score::{
        AtomEvent, AtomNote, EventGroup, EventGroupKind, LossReport, MasterBar, RepeatMarker, Score,
        Track, Voice,
    },
    slice::TickRange,
};

#[derive(Debug, Arbitrary)]
struct FuzzInput {
    seed: u64,
    /// Mapped to 1..=6 bars.
    bar_count: u8,
    /// Mapped to 1..=960 ticks per quarter.
    ppqn: u16,
    /// Part-A pitches, laid out as quarter notes (first 16 used).
    pitches: Vec<u8>,
    /// B's register offset relative to A.
    register_offset: i8,
    /// Relation-mode selector.
    mode_idx: u8,
}

const MODES: [RelationMode; 6] = [
    RelationMode::RhythmLock,
    RelationMode::RegisterContrast,
    RelationMode::CallResponse,
    RelationMode::SupportLayer,
    RelationMode::OctaveDouble,
    RelationMode::CounterMelody,
];

/// Builds a uniform 4/4 part-A score with quarter notes laid out per bar.
fn build_part_a(bar_count: usize, ppqn: u16, pitches: &[u8]) -> Option<Score> {
    let quarter = u32::from(ppqn);
    let bar = quarter.checked_mul(4)?;

    let mut master_bars = Vec::with_capacity(bar_count);
    for i in 0..bar_count {
        let start = u32::try_from(i).ok()?.checked_mul(bar)?;
        let end = start.checked_add(bar)?;
        let range = TickRange::new(Ticks(start), Ticks(end)).ok()?;
        master_bars.push(MasterBar {
            index: i,
            tick_range: range,
            time_signature: TimeSignature::new(4, 4).ok()?,
            tempo: Tempo::new(120.0).ok()?,
            repeat: RepeatMarker::default(),
        });
    }

    let velocity = Velocity::new(90).ok()?;
    let mut groups = Vec::new();
    for b in 0..bar_count {
        let bar_start = u32::try_from(b).ok()?.checked_mul(bar)?;
        for (i, &p) in pitches.iter().take(16).enumerate() {
            let offset = u32::try_from(i).ok()?.checked_mul(quarter)?;
            let onset = bar_start.checked_add(offset)?;
            let pitch = Pitch::new(p.min(127)).ok()?;
            groups.push(EventGroup {
                kind: EventGroupKind::Single,
                atoms: vec![AtomEvent::Note(AtomNote {
                    absolute_start: Ticks(onset),
                    duration: Ticks(quarter),
                    pitch,
                    velocity,
                    marks: NoteMarks::empty(),
                    position: None,
                })],
                technique_spans: Vec::new(),
            });
        }
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

fn b_pitches(score: &Score, idx: usize) -> Vec<u8> {
    score.tracks[idx].voices[0]
        .event_groups
        .iter()
        .flat_map(|g| &g.atoms)
        .filter_map(|a| match a {
            AtomEvent::Note(n) => Some(n.pitch.0),
            AtomEvent::Rest(_) => None,
        })
        .collect()
}

fuzz_target!(|input: FuzzInput| {
    let bar_count = usize::from(input.bar_count % 6) + 1;
    let ppqn = input.ppqn % 960 + 1;

    let Some(score) = build_part_a(bar_count, ppqn, &input.pitches) else {
        return;
    };

    let mode = MODES[usize::from(input.mode_idx) % MODES.len()];
    let spec = ComplementSpec {
        mode,
        register_offset: input.register_offset,
    };
    let seed = GenerationSeed(input.seed);

    let Ok(candidate) = arrange_complement(&score, 0, spec, seed) else {
        // Typed error (e.g. ModeNotImplemented, PartHasNoNotes) — oracle satisfied.
        return;
    };

    // Invariant: B shares A's PPQN and master-bar count.
    assert_eq!(
        candidate.score.ticks_per_quarter, score.ticks_per_quarter,
        "complement must share A's ticks_per_quarter",
    );
    assert_eq!(
        candidate.score.master_bars.len(),
        score.master_bars.len(),
        "complement must share A's master bars",
    );

    // Invariant: B's notes lie in the register derived from A ± offset.
    let profile = analyze_part(&score, 0).expect("A analysed");
    if let Some(reg) = profile.register {
        let shift = |p: u8| -> u8 {
            i32::from(p)
                .saturating_add(i32::from(input.register_offset))
                .clamp(0, 127) as u8
        };
        let lo = shift(reg.lowest.0).min(shift(reg.highest.0));
        let hi = shift(reg.lowest.0).max(shift(reg.highest.0));
        for p in b_pitches(&candidate.score, candidate.part_b_index) {
            assert!(
                p >= lo && p <= hi,
                "B pitch {p} out of derived register [{lo}, {hi}]",
            );
        }
    }

    // Invariant: deterministic for a fixed seed.
    let again = arrange_complement(&score, 0, spec, seed).expect("re-arrange must succeed");
    assert_eq!(
        candidate.score.tracks[candidate.part_b_index].voices[0],
        again.score.tracks[again.part_b_index].voices[0],
        "fixed seed must produce identical B voice",
    );

    // The pair validator must not panic on the produced pair.
    let _ = validate_pair(&candidate.score, 0, candidate.part_b_index);
});
