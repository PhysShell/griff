#![allow(clippy::pedantic, clippy::restriction, clippy::nursery)]
//! Candidate-level register baseline (throwaway corpus-experiment tooling).
//!
//! `griff generate` writes only the reranked winner, so winner-only metrics say
//! nothing about the strategies that lose. This harness calls the pre-rerank
//! `generate_candidate_set` seam directly and measures the pitch register of
//! EVERY candidate (5 strategies x N variants). It is **production-faithful**: the
//! generation request and corpus material come from the shared
//! `griff_cli::generation_input` seam (the same compiler `griff generate` uses),
//! not a duplicated loader. It also reports each input's pitch-class material.
//!
//! Emits JSONL: one `type:"input"` row per input, then one `type:"candidate"`
//! row per (seed x gesture x strategy x variant).
//!
//! Run: register_scan <input.tab> --corpus <dir> [--seeds 1,2,3,4,5] [--variants 10] [--gesture both|on|off]
use griff_cli::generation_input::{generation_request_from_score, load_corpus_material};
use griff_core::event::Ticks;
use griff_core::generate::RhythmTemplate;
use griff_core::import::import_score_auto;
use griff_core::rerank::{generate_candidate_set, SetRequest};
use griff_core::score::{AtomEvent, Score};
use std::path::Path;

fn arg_after(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn first_note_track(score: &Score) -> Option<usize> {
    score.tracks.iter().position(|t| {
        t.voices.first().is_some_and(|v| {
            v.event_groups
                .iter()
                .flat_map(|g| &g.atoms)
                .any(|a| matches!(a, AtomEvent::Note(_)))
        })
    })
}

fn all_pitches(score: &Score) -> Vec<u8> {
    score
        .tracks
        .iter()
        .flat_map(|t| &t.voices)
        .flat_map(|v| &v.event_groups)
        .flat_map(|g| &g.atoms)
        .filter_map(|a| match a {
            AtomEvent::Note(n) => Some(n.pitch.0),
            AtomEvent::Rest(_) => None,
        })
        .collect()
}

/// Ordered (by onset) output pitches of the score's first note-bearing track.
fn ordered_pitches(score: &Score) -> Vec<u8> {
    let Some(track) = first_note_track(score) else {
        return Vec::new();
    };
    let Some(v) = score.tracks.get(track).and_then(|t| t.voices.first()) else {
        return Vec::new();
    };
    let mut op: Vec<(u32, u8)> = v
        .event_groups
        .iter()
        .flat_map(|g| &g.atoms)
        .filter_map(|a| match a {
            AtomEvent::Note(n) => Some((n.absolute_start.0, n.pitch.0)),
            AtomEvent::Rest(_) => None,
        })
        .collect();
    op.sort_by_key(|(o, _)| *o);
    op.into_iter().map(|(_, p)| p).collect()
}

/// Bar-structure metrics for RepeatVariation, separating three interval kinds so
/// a deliberate figure-reset at the bar boundary is not conflated with an
/// in-bar/variation wrap:
/// - `grid` — first sounding bar's note count (the repeated figure length);
/// - `var_prev` — largest in-bar penultimate->varied-last interval;
/// - `intra_max` — largest interval between consecutive notes WITHIN a bar;
/// - `inter_reset` — largest interval at a bar boundary (last-of-bar ->
///   first-of-next), report-only (a conscious figure return is allowed);
/// - `last_distinct` — distinct bar-final pitches (variation present when > 1).
fn bar_metrics(score: &Score) -> (usize, u32, u32, u32, usize) {
    let Some(track) = first_note_track(score) else {
        return (0, 0, 0, 0, 0);
    };
    let Some(v) = score.tracks.get(track).and_then(|t| t.voices.first()) else {
        return (0, 0, 0, 0, 0);
    };
    let notes: Vec<(u32, u8)> = v
        .event_groups
        .iter()
        .flat_map(|g| &g.atoms)
        .filter_map(|a| match a {
            AtomEvent::Note(n) => Some((n.absolute_start.0, n.pitch.0)),
            AtomEvent::Rest(_) => None,
        })
        .collect();
    let mut grid = 0usize;
    let mut var_prev = 0u32;
    let mut intra_max = 0u32;
    let mut inter_reset = 0u32;
    let mut last_notes: Vec<u8> = Vec::new();
    let mut prev_bar_last: Option<u8> = None;
    for bar in &score.master_bars {
        let (s, e) = (bar.tick_range.start.0, bar.tick_range.end.0);
        let mut bn: Vec<(u32, u8)> = notes
            .iter()
            .copied()
            .filter(|(o, _)| *o >= s && *o < e)
            .collect();
        bn.sort_by_key(|(o, _)| *o);
        if bn.is_empty() {
            continue;
        }
        if grid == 0 {
            grid = bn.len();
        }
        for w in bn.windows(2) {
            intra_max = intra_max.max((i32::from(w[1].1) - i32::from(w[0].1)).unsigned_abs());
        }
        if bn.len() >= 2 {
            let last = bn[bn.len() - 1].1;
            let penult = bn[bn.len() - 2].1;
            var_prev = var_prev.max((i32::from(last) - i32::from(penult)).unsigned_abs());
        }
        if let (Some(pl), Some(first)) = (prev_bar_last, bn.first()) {
            inter_reset = inter_reset.max((i32::from(first.1) - i32::from(pl)).unsigned_abs());
        }
        let bar_last = bn[bn.len() - 1].1;
        last_notes.push(bar_last);
        prev_bar_last = Some(bar_last);
    }
    last_notes.sort_unstable();
    last_notes.dedup();
    (grid, var_prev, intra_max, inter_reset, last_notes.len())
}

/// Highest / lowest in-class pitch inside `[in_lo, in_hi]` — the ladder's top /
/// bottom rung; falls back to `in_lo` when the range misses the palette (mirrors
/// `ScaleLadder::build`, so saturation lands on the same rung the generator uses).
fn ladder_ends(in_lo: u8, in_hi: u8, palette: &[u8]) -> (u8, u8) {
    let rungs: Vec<u8> = (in_lo..=in_hi)
        .filter(|p| palette.contains(&(p % 12)))
        .collect();
    (
        *rungs.first().unwrap_or(&in_lo),
        *rungs.last().unwrap_or(&in_lo),
    )
}

/// Register + saturation + jump metrics of one candidate's line (ordered
/// pitches), as embeddable JSON fields. `palette` = the input's pitch classes.
fn register_fields(pitches: &[u8], in_lo: u8, in_hi: u8, palette: &[u8]) -> String {
    if pitches.is_empty() {
        return "\"output_min\":0,\"output_max\":0,\"output_span\":0,\"range_utilization\":0.0,\"distinct_pitch\":0,\"distinct_pitch_classes\":0,\"lowest_octave_share\":0.0,\"highest_octave_share\":0.0,\"edge_low_share\":0.0,\"edge_high_share\":0.0,\"exact_low_share\":0.0,\"exact_high_share\":0.0,\"mode_pitch_share\":0.0,\"longest_same_pitch_run\":0,\"mean_abs_interval\":0.0,\"max_abs_interval\":0,\"largest_upward_interval\":0,\"largest_downward_interval\":0,\"octave_leap_count\":0,\"octave_leap_share\":0.0,\"at_least_octave_share\":0.0,\"exact_octave_share\":0.0,\"over_octave_share\":0.0,\"alternation_rate\":0.0,\"boundary_reversal_count\":0,\"boundary_plateau_run\":0,\"pitch_hash\":\"0\",\"pitch_stddev\":0.0,\"in_bounds_rate\":0.0,\"in_class_rate\":0.0".to_string();
    }
    let n = pitches.len() as f64;
    let omin = *pitches.iter().min().unwrap();
    let omax = *pitches.iter().max().unwrap();
    let ospan = omax.saturating_sub(omin);
    let ispan = in_hi.saturating_sub(in_lo);
    let util = if ispan == 0 {
        0.0
    } else {
        f64::from(ospan) / f64::from(ispan)
    };
    let (lad_lo, lad_hi) = ladder_ends(in_lo, in_hi, palette);
    let mut dp: Vec<u8> = pitches.to_vec();
    dp.sort_unstable();
    dp.dedup();
    let mut dpc: Vec<u8> = pitches.iter().map(|p| p % 12).collect();
    dpc.sort_unstable();
    dpc.dedup();
    let share = |pred: &dyn Fn(u8) -> bool| pitches.iter().filter(|&&p| pred(p)).count() as f64 / n;
    let low_share = share(&|p| p <= omin.saturating_add(11));
    let high_share = share(&|p| p >= omax.saturating_sub(11));
    let edge_low = share(&|p| p <= in_lo.saturating_add(11));
    let edge_high = share(&|p| p >= in_hi.saturating_sub(11));
    let exact_low = share(&|p| p == lad_lo);
    let exact_high = share(&|p| p == lad_hi);
    let in_bounds = share(&|p| p >= in_lo && p <= in_hi);
    let in_class = share(&|p| palette.contains(&(p % 12)));
    let mut counts = std::collections::HashMap::new();
    for &p in pitches {
        *counts.entry(p).or_insert(0u32) += 1;
    }
    let mode_share = f64::from(counts.values().copied().max().unwrap_or(0)) / n;
    let mut longest = 1u32;
    let mut cur = 1u32;
    for w in pitches.windows(2) {
        if w[0] == w[1] {
            cur += 1;
            longest = longest.max(cur);
        } else {
            cur = 1;
        }
    }
    let signed: Vec<i32> = pitches
        .windows(2)
        .map(|w| i32::from(w[1]) - i32::from(w[0]))
        .collect();
    let absint: Vec<u32> = signed.iter().map(|i| i.unsigned_abs()).collect();
    let mean_int = if absint.is_empty() {
        0.0
    } else {
        absint.iter().map(|&i| f64::from(i)).sum::<f64>() / absint.len() as f64
    };
    let max_int = absint.iter().copied().max().unwrap_or(0);
    let up = signed.iter().copied().max().unwrap_or(0).max(0);
    let down = signed
        .iter()
        .copied()
        .min()
        .unwrap_or(0)
        .min(0)
        .unsigned_abs();
    let oct_leaps = absint.iter().filter(|&&i| i >= 12).count();
    let idenom = if absint.is_empty() {
        1.0
    } else {
        absint.len() as f64
    };
    let oct_leap_share = oct_leaps as f64 / idenom; // == at_least_octave (>=12)
    let exact_octave = absint.iter().filter(|&&i| i == 12).count() as f64 / idenom;
    let over_octave = absint.iter().filter(|&&i| i > 12).count() as f64 / idenom;
    // low<->high bouncing: interior notes whose consecutive interval signs reverse
    let alternation = if signed.len() < 2 {
        0.0
    } else {
        let z = signed
            .windows(2)
            .filter(|w| w[0] != 0 && w[1] != 0 && (w[0] > 0) != (w[1] > 0))
            .count();
        z as f64 / (signed.len() - 1) as f64
    };
    // FNV-1a of the ordered pitch bytes — for before/after regression comparison
    let mut phash: u64 = 0xcbf2_9ce4_8422_2325;
    for &p in pitches {
        phash ^= u64::from(p);
        phash = phash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    // boundary behaviour — reflecting traversal reverses AT a rung end; a clamp
    // saturates INTO a run at a rung end. Both distinguish wrap-free fixes.
    let at_bound = |p: u8| p == lad_lo || p == lad_hi;
    let boundary_reversals = if pitches.len() < 3 {
        0
    } else {
        (1..pitches.len() - 1)
            .filter(|&i| {
                at_bound(pitches[i])
                    && signed[i - 1] != 0
                    && signed[i] != 0
                    && (signed[i - 1] > 0) != (signed[i] > 0)
            })
            .count()
    };
    let mut boundary_plateau = 0u32;
    let mut bcur = 0u32;
    for &p in pitches {
        if at_bound(p) {
            bcur += 1;
            boundary_plateau = boundary_plateau.max(bcur);
        } else {
            bcur = 0;
        }
    }
    let pmean = pitches.iter().map(|&p| f64::from(p)).sum::<f64>() / n;
    let pstd = (pitches
        .iter()
        .map(|&p| (f64::from(p) - pmean).powi(2))
        .sum::<f64>()
        / n)
        .sqrt();
    format!(
        "\"output_min\":{omin},\"output_max\":{omax},\"output_span\":{ospan},\"range_utilization\":{util:.3},\"distinct_pitch\":{},\"distinct_pitch_classes\":{},\"lowest_octave_share\":{low_share:.3},\"highest_octave_share\":{high_share:.3},\"edge_low_share\":{edge_low:.3},\"edge_high_share\":{edge_high:.3},\"exact_low_share\":{exact_low:.3},\"exact_high_share\":{exact_high:.3},\"mode_pitch_share\":{mode_share:.3},\"longest_same_pitch_run\":{longest},\"mean_abs_interval\":{mean_int:.2},\"max_abs_interval\":{max_int},\"largest_upward_interval\":{up},\"largest_downward_interval\":{down},\"octave_leap_count\":{oct_leaps},\"octave_leap_share\":{oct_leap_share:.3},\"at_least_octave_share\":{oct_leap_share:.3},\"exact_octave_share\":{exact_octave:.3},\"over_octave_share\":{over_octave:.3},\"alternation_rate\":{alternation:.3},\"boundary_reversal_count\":{boundary_reversals},\"boundary_plateau_run\":{boundary_plateau},\"pitch_hash\":\"{phash:016x}\",\"pitch_stddev\":{pstd:.2},\"in_bounds_rate\":{in_bounds:.3},\"in_class_rate\":{in_class:.3}",
        dp.len(), dpc.len()
    )
}

fn jstr(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "'"))
}

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    let input = args.get(1).filter(|s| !s.starts_with("--")).ok_or("usage: register_scan <input> --corpus <dir> [--seeds ..] [--variants N] [--gesture both|on|off]")?.clone();
    let corpus = arg_after(&args, "--corpus").ok_or("missing required --corpus <dir>")?;
    let seeds: Vec<u64> = arg_after(&args, "--seeds")
        .unwrap_or_else(|| "1,2,3,4,5".to_string())
        .split(',')
        .filter_map(|s| s.trim().parse().ok())
        .collect();
    let variants: usize = arg_after(&args, "--variants")
        .and_then(|s| s.parse().ok())
        .unwrap_or(10);
    let gmode = arg_after(&args, "--gesture").unwrap_or_else(|| "both".to_string());

    let bytes = std::fs::read(&input).map_err(|e| format!("read input '{input}': {e}"))?;
    let src = import_score_auto(&bytes).map_err(|e| format!("import input '{input}': {e}"))?;
    let in_pitches = all_pitches(&src);
    if in_pitches.is_empty() {
        return Err("input has no notes".to_string());
    }
    let in_lo = in_pitches.iter().copied().min().unwrap();
    let in_hi = in_pitches.iter().copied().max().unwrap();

    // PRODUCTION material seam (placed templates + median gesture) — not a copy.
    let material = load_corpus_material(Path::new(&corpus))
        .map_err(|e| format!("load_corpus_material: {e:?}"))?;

    let iname = Path::new(&input)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    let mut pc_abs: Vec<u8> = in_pitches.iter().map(|p| p % 12).collect();
    pc_abs.sort_unstable();
    pc_abs.dedup();
    let mut pc_rel: Vec<u8> = in_pitches
        .iter()
        .map(|p| p.saturating_sub(in_lo) % 12)
        .collect();
    pc_rel.sort_unstable();
    pc_rel.dedup();
    println!(
        "{{\"type\":\"input\",\"input\":{},\"min_pitch\":{in_lo},\"max_pitch\":{in_hi},\"input_span\":{},\"pitch_class_count\":{},\"pitch_classes_absolute\":{:?},\"pitch_classes_relative_to_min\":{:?},\"corpus_templates\":{},\"gesture_on\":{},\"variants_per_strategy\":{variants}}}",
        jstr(iname), in_hi - in_lo, pc_abs.len(), pc_abs, pc_rel, material.rhythms.len(), material.gesture.is_some()
    );

    // --synth-grid N overrides corpus rhythm with a single N-note uniform bar,
    // for the focused RepeatVariation long-grid gate (grids the corpus lacks).
    let synth_grid: Option<usize> = arg_after(&args, "--synth-grid").and_then(|s| s.parse().ok());
    let bar_ticks = src.master_bars.first().map_or(1920, |b| {
        b.tick_range.end.0.saturating_sub(b.tick_range.start.0)
    });

    for &seed in &seeds {
        let base = generation_request_from_score(&src, seed, 8)
            .map_err(|e| format!("generation_request_from_score: {e:?}"))?;
        // synthetic grid > corpus rhythms > first-bar fallback (as cmd_generate does)
        let source_rhythms = if let Some(g) = synth_grid.filter(|&g| g > 0) {
            let dur = (bar_ticks / g as u32).max(1);
            vec![RhythmTemplate::from_durations(&vec![Ticks(dur); g])]
        } else if material.rhythms.is_empty() {
            base.source_rhythms.clone()
        } else {
            material.rhythms.clone()
        };
        for glabel in match gmode.as_str() {
            "on" => vec!["on"],
            "off" => vec!["off"],
            _ => vec!["on", "off"],
        } {
            let gesture = if glabel == "on" {
                material.gesture
            } else {
                None
            };
            let req = SetRequest {
                seed: base.seed,
                pitch_material: base.pitch_material.clone(),
                constraints: base.constraints,
                source_rhythms: source_rhythms.clone(),
                variants_per_strategy: variants,
                gesture,
            };
            match generate_candidate_set(&req) {
                Ok(cands) => {
                    let set_size = cands.len();
                    for c in &cands {
                        let line = ordered_pitches(&c.score);
                        let (grid_nc, var_prev, intra_max, inter_reset, last_distinct) =
                            bar_metrics(&c.score);
                        println!(
                            "{{\"type\":\"candidate\",\"input\":{},\"seed\":{seed},\"gesture\":\"{glabel}\",\"strategy\":\"{:?}\",\"variant_seed\":\"{}\",\"variants_per_strategy\":{variants},\"candidate_set_size\":{set_size},\"grid_note_count\":{grid_nc},\"variation_prev_interval\":{var_prev},\"intra_bar_max_interval\":{intra_max},\"inter_bar_reset_interval\":{inter_reset},\"last_note_distinct\":{last_distinct},\"pitch_lo_constraint\":{in_lo},\"pitch_hi_constraint\":{in_hi},\"input_span\":{},{}}}",
                            jstr(iname), c.strategy, c.seed.0, in_hi - in_lo, register_fields(&line, in_lo, in_hi, &pc_abs)
                        );
                    }
                }
                Err(e) => eprintln!("candidate set failed (seed {seed}, gesture {glabel}): {e:?}"),
            }
        }
    }
    Ok(())
}
