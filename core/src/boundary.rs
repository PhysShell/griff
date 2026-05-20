//! Phrase boundary detection (S4).
//!
//! Detects musically meaningful phrase boundaries in a track using six
//! heuristic signals: pause, cadence, rhythm-reset, motif-boundary (reserved),
//! register-jump, and density-change.

use crate::{
    event::Ticks,
    score::{AtomEvent, AtomRest, MasterBar, Score},
};

// ── public types ──────────────────────────────────────────────────────────────

/// Which heuristic signals fired at a boundary.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct BoundaryReason {
    /// A rest of sufficient length preceded the boundary.
    pub pause: bool,
    /// The boundary coincides with a master-bar downbeat.
    pub cadence: bool,
    /// Inter-onset interval changed significantly across the boundary.
    pub rhythm_reset: bool,
    /// Motif boundary detected by corpus analysis (always false in S4; reserved for S5+).
    pub motif_boundary: bool,
    /// Average register jumped across the boundary.
    pub register_jump: bool,
    /// Note density changed significantly across the boundary.
    pub density_change: bool,
    /// The boundary was supplied as a manual override and always emitted.
    pub manual_override: bool,
}

/// A detected phrase boundary.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PhraseBoundary {
    /// Tick at which the gap/rest begins (or the cut point if instant).
    pub start_tick: Ticks,
    /// Tick at which the next phrase resumes (`== start_tick` for instant cuts).
    pub end_tick: Ticks,
    /// Aggregate weighted score in `[0.0, 1.0]`.
    pub score: f64,
    /// Which signals contributed.
    pub reason: BoundaryReason,
}

/// Configuration for the boundary detector.
#[derive(Debug, Clone)]
pub struct BoundaryConfig {
    /// Weights for `[pause, cadence, rhythm_reset, motif_boundary, register_jump, density_change]`.
    pub weights: [f64; 6],
    /// Minimum aggregate score to emit a boundary.
    pub threshold: f64,
    /// Minimum tick gap between adjacent accepted boundaries.
    pub min_gap: Ticks,
    /// Snap boundary ticks to this grid (0 = no snapping).
    pub quantize_ticks: Ticks,
    /// Manual override ticks — always emitted regardless of score.
    pub manual_overrides: Vec<Ticks>,
}

impl Default for BoundaryConfig {
    fn default() -> Self {
        Self {
            // Equal weight across the six signals; each is 1/6.
            weights: [1.0 / 6.0; 6],
            threshold: 0.35,
            // 2 quarter notes at PPQN 960.
            min_gap: Ticks(1920),
            // 1/16 at PPQN 960.
            quantize_ticks: Ticks(240),
            manual_overrides: Vec::new(),
        }
    }
}

// ── public detection entry point ──────────────────────────────────────────────

/// Detects phrase boundaries in the first voice of `track_idx`.
///
/// Returns an empty `Vec` when the track index is out of range, the track has
/// no voices, or no candidates pass the threshold.
pub fn detect_phrase_boundaries(
    score: &Score,
    track_idx: usize,
    config: &BoundaryConfig,
) -> Vec<PhraseBoundary> {
    let ppqn = u32::from(score.ticks_per_quarter);

    // ── Step 1: collect atoms from the first voice, sorted by onset ───────────
    let atoms = collect_atoms(score, track_idx);

    // ── Step 2: build candidate tick list ────────────────────────────────────
    let candidates = build_candidates(&atoms, &score.master_bars, ppqn);

    // ── Step 3: score each candidate ─────────────────────────────────────────
    let mut boundaries: Vec<PhraseBoundary> = candidates
        .into_iter()
        .filter_map(|tick| score_candidate(tick, &atoms, &score.master_bars, ppqn, config))
        .collect();

    // ── Step 4: sort and apply min-gap suppression ────────────────────────────
    boundaries.sort_unstable_by_key(|b| b.start_tick.0);
    boundaries = apply_min_gap(boundaries, config.min_gap);

    // ── Step 5: snap ticks to quantize grid ──────────────────────────────────
    let grid = config.quantize_ticks;
    for b in &mut boundaries {
        b.start_tick = quantize_tick(b.start_tick, grid);
        b.end_tick = quantize_tick(b.end_tick, grid);
    }

    // ── Step 6: append manual overrides ──────────────────────────────────────
    for &tick in &config.manual_overrides {
        let snapped = quantize_tick(tick, grid);
        boundaries.push(PhraseBoundary {
            start_tick: snapped,
            end_tick: snapped,
            score: 1.0,
            reason: BoundaryReason {
                manual_override: true,
                ..BoundaryReason::default()
            },
        });
    }

    // ── Step 7: final sort ────────────────────────────────────────────────────
    boundaries.sort_unstable_by_key(|b| b.start_tick.0);
    boundaries
}

// ── internal helpers ──────────────────────────────────────────────────────────

/// Collects all `AtomEvent`s from the first voice of `track_idx`, sorted by onset.
fn collect_atoms(score: &Score, track_idx: usize) -> Vec<AtomEvent> {
    let Some(track) = score.tracks.get(track_idx) else {
        return Vec::new();
    };
    let Some(voice) = track.voices.first() else {
        return Vec::new();
    };

    let mut atoms: Vec<AtomEvent> = voice
        .event_groups
        .iter()
        .flat_map(|g| g.atoms.iter().copied())
        .collect();
    atoms.sort_unstable_by_key(|a| a.absolute_start().0);
    atoms
}

/// Builds the deduplicated, sorted list of candidate boundary ticks.
///
/// Candidates are:
/// - Rests with duration > ppqn/8.
/// - Master-bar downbeats.
fn build_candidates(atoms: &[AtomEvent], master_bars: &[MasterBar], ppqn: u32) -> Vec<Ticks> {
    // ppqn/8 threshold for rest candidacy.
    #[allow(clippy::arithmetic_side_effects)]
    // ppqn is a musical resolution constant; dividing by 8 is safe.
    let rest_min = ppqn / 8;

    let mut ticks: Vec<u32> = Vec::new();

    for atom in atoms {
        if let AtomEvent::Rest(r) = atom {
            if r.duration.0 > rest_min {
                ticks.push(r.absolute_start.0);
            }
        }
    }

    for mb in master_bars {
        ticks.push(mb.tick_range.start.0);
    }

    ticks.sort_unstable();
    ticks.dedup();
    ticks.into_iter().map(Ticks).collect()
}

/// Evaluates a single candidate tick and returns a `PhraseBoundary` if it
/// passes the threshold or qualifies as a hard boundary.
fn score_candidate(
    tick: Ticks,
    atoms: &[AtomEvent],
    master_bars: &[MasterBar],
    ppqn: u32,
    config: &BoundaryConfig,
) -> Option<PhraseBoundary> {
    let s_pause = signal_pause(tick, atoms, ppqn);
    let s_cadence = signal_cadence(tick, master_bars);
    let s_rhythm = signal_rhythm_reset(tick, atoms);
    let s_motif = signal_motif_boundary();
    let s_register = signal_register_jump(tick, atoms);
    let s_density = signal_density_change(tick, atoms, ppqn);

    let signals = [s_pause, s_cadence, s_rhythm, s_motif, s_register, s_density];

    // Weighted sum.
    let mut weighted_score = 0.0_f64;
    for (w, s) in config.weights.iter().zip(signals.iter()) {
        weighted_score += w * s;
    }

    let reason = BoundaryReason {
        pause: s_pause > 0.0,
        cadence: s_cadence > 0.0,
        rhythm_reset: s_rhythm > 0.0,
        motif_boundary: s_motif > 0.0,
        register_jump: s_register > 0.0,
        density_change: s_density > 0.0,
        manual_override: false,
    };

    // Hard boundary: rest duration > 2 * ppqn.
    #[allow(clippy::arithmetic_side_effects)]
    // 2 * ppqn is a small musical constant well within u32 range.
    let hard_threshold = ppqn * 2;
    let is_hard = find_rest_at(tick, atoms, ppqn).is_some_and(|r| r.duration.0 > hard_threshold);

    if !is_hard && weighted_score <= config.threshold {
        return None;
    }

    let (final_score, final_reason) = if is_hard {
        let hard_reason = BoundaryReason {
            pause: true,
            ..reason
        };
        (1.0_f64, hard_reason)
    } else {
        (weighted_score.clamp(0.0, 1.0), reason)
    };

    // Determine end_tick: start of rest + rest duration, or same as start.
    let end_tick = find_rest_at(tick, atoms, ppqn).map_or(tick, |r| {
        Ticks(r.absolute_start.0.saturating_add(r.duration.0))
    });

    Some(PhraseBoundary {
        start_tick: tick,
        end_tick,
        score: final_score,
        reason: final_reason,
    })
}

/// Finds a rest atom at or within ppqn/8 ticks of `tick`.
fn find_rest_at(tick: Ticks, atoms: &[AtomEvent], ppqn: u32) -> Option<AtomRest> {
    #[allow(clippy::arithmetic_side_effects)]
    // ppqn / 8 is a small constant; no overflow possible.
    let window = ppqn / 8;
    atoms.iter().find_map(|a| {
        if let AtomEvent::Rest(r) = a {
            let diff = r.absolute_start.0.abs_diff(tick.0);
            if diff <= window {
                return Some(*r);
            }
        }
        None
    })
}

/// Greedy min-gap suppression: keep the first boundary in any run of boundaries
/// that are closer together than `min_gap`.
fn apply_min_gap(boundaries: Vec<PhraseBoundary>, min_gap: Ticks) -> Vec<PhraseBoundary> {
    let mut kept: Vec<PhraseBoundary> = Vec::new();
    let mut last_kept_tick: Option<u32> = None;

    for b in boundaries {
        let accept = last_kept_tick.map_or(true, |last| {
            b.start_tick.0.saturating_sub(last) >= min_gap.0
        });
        if accept {
            last_kept_tick = Some(b.start_tick.0);
            kept.push(b);
        }
    }
    kept
}

/// Snaps `tick` to the nearest multiple of `grid`.
///
/// Division and multiplication on raw u32 values (the grid is a musical
/// constant within u32 range).
#[allow(clippy::arithmetic_side_effects)]
const fn quantize_tick(tick: Ticks, grid: Ticks) -> Ticks {
    if grid.0 == 0 {
        return tick;
    }
    // Round to nearest grid point using saturating arithmetic.
    let half = grid.0 / 2;
    let rounded = tick.0.saturating_add(half) / grid.0 * grid.0;
    Ticks(rounded)
}

// ── the six heuristic signals ─────────────────────────────────────────────────

/// Returns a pause signal in `[0.0, 1.0]`.
///
/// Finds a rest at or near `tick` (within ppqn/8).  If the rest is longer than
/// a quarter note (ppqn/4), scales the score up to 1.0 over four quarter notes.
fn signal_pause(tick: Ticks, atoms: &[AtomEvent], ppqn: u32) -> f64 {
    #[allow(clippy::arithmetic_side_effects)]
    // ppqn / 4 is a small constant.
    let quarter = ppqn / 4;

    let Some(rest) = find_rest_at(tick, atoms, ppqn) else {
        return 0.0;
    };

    if rest.duration.0 <= quarter {
        return 0.0;
    }

    // Scale: (duration / (4 * ppqn)).min(1.0)
    let four_quarters = f64::from(ppqn) * 4.0;
    (f64::from(rest.duration.0) / four_quarters).min(1.0)
}

/// Returns a cadence signal: 0.6 if `tick` is a master-bar downbeat, else 0.0.
fn signal_cadence(tick: Ticks, master_bars: &[MasterBar]) -> f64 {
    let on_bar = master_bars.iter().any(|mb| mb.tick_range.start.0 == tick.0);
    if on_bar {
        0.6
    } else {
        0.0
    }
}

/// Returns a rhythm-reset signal based on IOI change across `tick`.
///
/// Collects up to 4 note onsets before and after `tick`, computes average IOI
/// for each group, and returns a normalised change score.
fn signal_rhythm_reset(tick: Ticks, atoms: &[AtomEvent]) -> f64 {
    let note_onsets_before: Vec<u32> = atoms
        .iter()
        .filter(|a| matches!(a, AtomEvent::Note(_)) && a.absolute_start().0 < tick.0)
        .rev()
        .take(4)
        .map(|a| a.absolute_start().0)
        .collect();

    let note_onsets_after: Vec<u32> = atoms
        .iter()
        .filter(|a| matches!(a, AtomEvent::Note(_)) && a.absolute_start().0 >= tick.0)
        .take(4)
        .map(|a| a.absolute_start().0)
        .collect();

    let avg_ioi_before = avg_ioi(&note_onsets_before);
    let avg_ioi_after = avg_ioi(&note_onsets_after);

    match (avg_ioi_before, avg_ioi_after) {
        (Some(b), Some(a)) => {
            let diff = (b - a).abs();
            let denom = b.max(a).max(f64::EPSILON);
            (diff / denom).clamp(0.0, 1.0)
        }
        _ => 0.0,
    }
}

/// Computes the average inter-onset interval from a slice of onset ticks.
///
/// Returns `None` if there are fewer than two onsets.
fn avg_ioi(onsets: &[u32]) -> Option<f64> {
    if onsets.len() < 2 {
        return None;
    }
    // onsets may be in reverse order (before-tick group); sort a copy.
    let mut sorted = onsets.to_vec();
    sorted.sort_unstable();

    let mut total = 0_u64;
    let mut count = 0_u64;
    for pair in sorted.windows(2) {
        // windows always yields slices of exactly 2 here.
        let a = u64::from(*pair.first().unwrap_or(&0));
        let b = u64::from(*pair.get(1).unwrap_or(&0));
        total = total.saturating_add(b.saturating_sub(a));
        count = count.saturating_add(1);
    }
    if count == 0 {
        None
    } else {
        #[allow(clippy::cast_precision_loss)]
        Some(total as f64 / count as f64)
    }
}

/// Returns a register-jump signal: normalised absolute pitch-average difference.
fn signal_register_jump(tick: Ticks, atoms: &[AtomEvent]) -> f64 {
    let pitches_before: Vec<u8> = atoms
        .iter()
        .filter(|a| matches!(a, AtomEvent::Note(_)) && a.absolute_start().0 < tick.0)
        .rev()
        .take(4)
        .filter_map(|a| {
            if let AtomEvent::Note(n) = a {
                Some(n.pitch.0)
            } else {
                None
            }
        })
        .collect();

    let pitches_after: Vec<u8> = atoms
        .iter()
        .filter(|a| matches!(a, AtomEvent::Note(_)) && a.absolute_start().0 >= tick.0)
        .take(4)
        .filter_map(|a| {
            if let AtomEvent::Note(n) = a {
                Some(n.pitch.0)
            } else {
                None
            }
        })
        .collect();

    if pitches_before.is_empty() || pitches_after.is_empty() {
        return 0.0;
    }

    let avg_before = avg_pitch(&pitches_before);
    let avg_after = avg_pitch(&pitches_after);

    ((avg_before - avg_after).abs() / 127.0).clamp(0.0, 1.0)
}

/// Computes the average pitch from a slice of MIDI pitch values.
fn avg_pitch(pitches: &[u8]) -> f64 {
    if pitches.is_empty() {
        return 0.0;
    }
    let sum: u64 = pitches.iter().map(|&p| u64::from(p)).sum();
    // pitches.len() is at most 4 (capped by take(4)); precision loss is negligible.
    #[allow(clippy::cast_precision_loss)]
    let len = pitches.len() as f64;
    #[allow(clippy::cast_precision_loss)]
    let result = sum as f64 / len;
    result
}

/// Returns a density-change signal: normalised absolute note-density difference.
fn signal_density_change(tick: Ticks, atoms: &[AtomEvent], ppqn: u32) -> f64 {
    let window = u64::from(ppqn).saturating_mul(4);

    let tick_u64 = u64::from(tick.0);
    let window_start = tick_u64.saturating_sub(window);
    let window_end = tick_u64.saturating_add(window);

    let notes_before = atoms
        .iter()
        .filter(|a| {
            matches!(a, AtomEvent::Note(_)) && {
                let t = u64::from(a.absolute_start().0);
                t >= window_start && t < tick_u64
            }
        })
        .count();

    let notes_after = atoms
        .iter()
        .filter(|a| {
            matches!(a, AtomEvent::Note(_)) && {
                let t = u64::from(a.absolute_start().0);
                t >= tick_u64 && t < window_end
            }
        })
        .count();

    let four_ppqn = f64::from(ppqn) * 4.0;
    // note counts are small (bounded by score length); precision loss is negligible.
    #[allow(clippy::cast_precision_loss)]
    let density_before = notes_before as f64 / four_ppqn;
    #[allow(clippy::cast_precision_loss)]
    let density_after = notes_after as f64 / four_ppqn;

    let diff = (density_before - density_after).abs();
    let denom = density_before + density_after + f64::EPSILON;
    (diff / denom).clamp(0.0, 1.0)
}

/// Placeholder — always returns 0.0. Needs S5 corpus integration.
const fn signal_motif_boundary() -> f64 {
    0.0
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::missing_assert_message
)]
mod tests {
    use super::{detect_phrase_boundaries, quantize_tick, signal_cadence, BoundaryConfig};
    use crate::{
        event::{Tempo, Ticks, TimeSignature},
        score::{
            AtomEvent, AtomRest, EventGroup, EventGroupKind, LossReport, MasterBar, Score, Track,
            Voice,
        },
        slice::TickRange,
    };

    // ── helpers ───────────────────────────────────────────────────────────────

    fn ts_4_4() -> TimeSignature {
        TimeSignature::new(4, 4).expect("4/4 valid")
    }

    fn tempo_120() -> Tempo {
        Tempo::new(120.0).expect("120 BPM valid")
    }

    fn tick_range(start: u32, end: u32) -> TickRange {
        TickRange::new(Ticks(start), Ticks(end)).expect("ordered range")
    }

    fn atom_rest(start: u32, dur: u32) -> AtomEvent {
        AtomEvent::Rest(AtomRest {
            absolute_start: Ticks(start),
            duration: Ticks(dur),
        })
    }

    /// Build a minimal one-bar score with given atoms.
    fn one_bar_score(atoms: Vec<AtomEvent>) -> Score {
        let group = EventGroup {
            kind: EventGroupKind::Single,
            atoms,
            technique_spans: Vec::new(),
        };
        let voice = Voice {
            id: 0,
            event_groups: vec![group],
        };
        let track = Track {
            name: None,
            channel: 0,
            voices: vec![voice],
        };
        let mb = MasterBar {
            index: 0,
            tick_range: tick_range(0, 3840),
            time_signature: ts_4_4(),
            tempo: tempo_120(),
        };
        Score {
            ticks_per_quarter: 960,
            master_bars: vec![mb],
            tracks: vec![track],
            source_meta: None,
            loss: LossReport::new(),
        }
    }

    /// Build a two-bar score with atoms in bar 0.
    fn two_bar_score(atoms: Vec<AtomEvent>) -> Score {
        let group = EventGroup {
            kind: EventGroupKind::Single,
            atoms,
            technique_spans: Vec::new(),
        };
        let voice = Voice {
            id: 0,
            event_groups: vec![group],
        };
        let track = Track {
            name: None,
            channel: 0,
            voices: vec![voice],
        };
        Score {
            ticks_per_quarter: 960,
            master_bars: vec![
                MasterBar {
                    index: 0,
                    tick_range: tick_range(0, 3840),
                    time_signature: ts_4_4(),
                    tempo: tempo_120(),
                },
                MasterBar {
                    index: 1,
                    tick_range: tick_range(3840, 7680),
                    time_signature: ts_4_4(),
                    tempo: tempo_120(),
                },
            ],
            tracks: vec![track],
            source_meta: None,
            loss: LossReport::new(),
        }
    }

    // ── test 1 ────────────────────────────────────────────────────────────────

    #[test]
    fn default_config_has_equal_weights() {
        let cfg = BoundaryConfig::default();
        let sum: f64 = cfg.weights.iter().sum();
        // Should be very close to 1.0 (6 × 1/6).
        assert!(
            (sum - 1.0_f64).abs() < 1e-10,
            "weights must sum to 1.0, got {sum}"
        );
        for &w in &cfg.weights {
            let expected = 1.0_f64 / 6.0_f64;
            assert!((w - expected).abs() < 1e-10, "each weight must equal 1/6");
        }
    }

    // ── test 2 ────────────────────────────────────────────────────────────────

    #[test]
    fn empty_score_returns_no_boundaries() {
        let score = one_bar_score(Vec::new());
        let cfg = BoundaryConfig::default();
        // Bar 0 downbeat (tick 0) will be a candidate; with no atoms all
        // signals except cadence are 0.  Cadence score = 0.6 * (1/6) = 0.1,
        // below threshold 0.35 → no boundary emitted.
        let result = detect_phrase_boundaries(&score, 0, &cfg);
        assert!(result.is_empty(), "empty voice should yield no boundaries");
    }

    // ── test 3 ────────────────────────────────────────────────────────────────

    #[test]
    fn manual_override_always_emitted() {
        let score = one_bar_score(Vec::new());
        let cfg = BoundaryConfig {
            manual_overrides: vec![Ticks(500)],
            ..BoundaryConfig::default()
        };
        let result = detect_phrase_boundaries(&score, 0, &cfg);
        assert!(
            !result.is_empty(),
            "manual override must produce at least one boundary"
        );
        let has_manual = result.iter().any(|b| b.reason.manual_override);
        assert!(
            has_manual,
            "result must contain a boundary with manual_override = true"
        );
    }

    // ── test 4 ────────────────────────────────────────────────────────────────

    #[test]
    fn long_rest_triggers_hard_boundary() {
        // Rest of 2*960 + 1 = 1921 ticks > 2*ppqn → hard boundary.
        let rest_dur = 960_u32 * 2 + 1;
        let score = one_bar_score(vec![atom_rest(240, rest_dur)]);
        let cfg = BoundaryConfig {
            threshold: 0.99, // very high threshold — only hard boundary passes
            ..BoundaryConfig::default()
        };
        let result = detect_phrase_boundaries(&score, 0, &cfg);
        let has_pause = result
            .iter()
            .any(|b| b.reason.pause && (b.score - 1.0).abs() < 1e-10);
        assert!(
            has_pause,
            "hard rest must produce a boundary with pause=true and score=1.0"
        );
    }

    // ── test 5 ────────────────────────────────────────────────────────────────

    #[test]
    fn short_rest_below_threshold_no_boundary() {
        // Rest of 100 ticks — ppqn/8 = 120; 100 < 120 so it won't even be a
        // candidate; even if it were, the pause signal would be 0.
        let score = one_bar_score(vec![atom_rest(240, 100)]);
        let cfg = BoundaryConfig {
            threshold: 0.99,
            ..BoundaryConfig::default()
        };
        let result = detect_phrase_boundaries(&score, 0, &cfg);
        // No candidate passes the high threshold.
        assert!(
            result.is_empty(),
            "short rest with high threshold should yield no boundaries"
        );
    }

    // ── test 6 ────────────────────────────────────────────────────────────────

    #[test]
    fn boundaries_sorted_by_start_tick() {
        // Two long rests, one early and one later.
        let rest_dur = 960_u32 * 3; // 3 quarter notes > 2*ppqn → hard boundaries
        let atoms = vec![atom_rest(500, rest_dur), atom_rest(4000, rest_dur)];
        let score = two_bar_score(atoms);
        let cfg = BoundaryConfig {
            min_gap: Ticks(0),
            ..BoundaryConfig::default()
        };
        let result = detect_phrase_boundaries(&score, 0, &cfg);
        let ticks: Vec<u32> = result.iter().map(|b| b.start_tick.0).collect();
        let mut sorted = ticks.clone();
        sorted.sort_unstable();
        assert_eq!(ticks, sorted, "boundaries must be sorted by start_tick");
    }

    // ── test 7 ────────────────────────────────────────────────────────────────

    #[test]
    fn min_gap_suppresses_close_boundaries() {
        // Two long rests very close together (300 ticks apart); min_gap = 1920.
        let rest_dur = 960_u32 * 3;
        let atoms = vec![atom_rest(500, rest_dur), atom_rest(800, rest_dur)];
        let score = one_bar_score(atoms);
        let cfg = BoundaryConfig {
            min_gap: Ticks(1920),
            quantize_ticks: Ticks(0), // no snapping so positions are exact
            ..BoundaryConfig::default()
        };
        let result = detect_phrase_boundaries(&score, 0, &cfg);
        // The two rests are within min_gap of each other; only the first is kept.
        let rest_boundaries: Vec<_> = result
            .iter()
            .filter(|b| b.reason.pause && !b.reason.manual_override)
            .collect();
        assert!(
            rest_boundaries.len() <= 1,
            "close boundaries must be suppressed; got {}",
            rest_boundaries.len()
        );
    }

    // ── test 8 ────────────────────────────────────────────────────────────────

    #[test]
    fn quantize_snaps_ticks() {
        // Multiples of 240: 0, 240, 480, ...
        // Tick = 350 → nearer to 240 (diff 110) than 480 (diff 130) → 240.
        assert_eq!(quantize_tick(Ticks(350), Ticks(240)), Ticks(240));
        // Tick = 100 → nearer to 0 (diff 100) than 240 (diff 140) → 0.
        assert_eq!(quantize_tick(Ticks(100), Ticks(240)), Ticks(0));
        // Tick = 120 → exactly halfway between 0 and 240; saturating_add rounds up → 240.
        assert_eq!(quantize_tick(Ticks(120), Ticks(240)), Ticks(240));
        // Grid = 0 → no snapping.
        assert_eq!(quantize_tick(Ticks(350), Ticks(0)), Ticks(350));
    }

    // ── test 9 ────────────────────────────────────────────────────────────────

    #[test]
    fn bar_boundary_cadence_signal() {
        // signal_cadence should return 0.6 when tick is a MasterBar start.
        let mb = MasterBar {
            index: 0,
            tick_range: tick_range(0, 3840),
            time_signature: ts_4_4(),
            tempo: tempo_120(),
        };
        let bars = vec![mb];
        // Tick == bar start → cadence signal fires.
        assert!(
            (signal_cadence(Ticks(0), &bars) - 0.6_f64).abs() < 1e-10,
            "cadence signal must be 0.6 on a bar downbeat"
        );
        // Tick != any bar start → cadence signal is 0.
        assert!(
            (signal_cadence(Ticks(123), &bars) - 0.0_f64).abs() < 1e-10,
            "cadence signal must be 0.0 away from bar downbeats"
        );
    }

    // ── test 10 ───────────────────────────────────────────────────────────────

    #[test]
    fn reason_has_pause_flag_for_rest() {
        // A long rest should produce a boundary with reason.pause = true.
        let rest_dur = 960_u32 * 5; // well above 2*ppqn
        let score = one_bar_score(vec![atom_rest(0, rest_dur)]);
        let cfg = BoundaryConfig::default();
        let result = detect_phrase_boundaries(&score, 0, &cfg);
        assert!(
            result.iter().any(|b| b.reason.pause),
            "a long rest must produce at least one boundary with reason.pause = true"
        );
    }
}
