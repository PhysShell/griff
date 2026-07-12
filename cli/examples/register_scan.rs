#![allow(clippy::pedantic, clippy::restriction, clippy::nursery)]
//! Candidate-level register baseline (throwaway corpus-experiment tooling).
//!
//! `griff generate` writes only the reranked winner, so winner-only metrics say
//! nothing about the strategies that lose. This harness calls the pre-rerank
//! `generate_candidate_set` seam directly and measures the pitch register of
//! EVERY candidate (5 strategies x N variants), for the same inputs/seeds, so a
//! per-strategy octave-confinement picture emerges. It also reports each input's
//! pitch-class material (absolute + relative-to-min) to check whether different
//! band inputs collapse to one relative palette.
//!
//! Emits JSONL: one `type:"input"` row per input, then one `type:"candidate"`
//! row per (seed x gesture x strategy x variant).
//!
//! Run: register_scan <input.tab> --corpus <dir> [--seeds 1,2,3,4,5] [--variants 10] [--gesture both|on|off]
use griff_core::corpus::ChunkMeta;
use griff_core::event::{Pitch, Ticks};
use griff_core::generate::{GenerationConstraints, GenerationSeed, PitchMaterial, RhythmTemplate};
use griff_core::gesture::{GestureControl, GestureStats};
use griff_core::import::import_score_auto;
use griff_core::rerank::{generate_candidate_set, SetRequest};
use griff_core::score::{AtomEvent, Score};
use griff_core::slice;
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

fn material_from(pitches: &[u8]) -> PitchMaterial {
    let lo = pitches.iter().copied().min().unwrap_or(0);
    let mut intervals: Vec<u8> = pitches.iter().map(|&p| p.saturating_sub(lo) % 12).collect();
    intervals.sort_unstable();
    intervals.dedup();
    if intervals.is_empty() {
        intervals.push(0);
    }
    PitchMaterial {
        root: Pitch(lo),
        intervals,
    }
}

/// Ordered (onset, pitch) of the score's first note-bearing track.
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

fn bar_rhythms(score: &Score, track: usize) -> Vec<Vec<Ticks>> {
    let Some(v) = score.tracks.get(track).and_then(|t| t.voices.first()) else {
        return Vec::new();
    };
    let mut notes: Vec<(u32, Ticks)> = v
        .event_groups
        .iter()
        .flat_map(|g| &g.atoms)
        .filter_map(|a| match a {
            AtomEvent::Note(n) => Some((n.absolute_start.0, n.duration)),
            AtomEvent::Rest(_) => None,
        })
        .collect();
    notes.sort_by_key(|(o, _)| *o);
    let mut templates: Vec<Vec<Ticks>> = Vec::new();
    for bar in &score.master_bars {
        let (s, e) = (bar.tick_range.start.0, bar.tick_range.end.0);
        let durs: Vec<Ticks> = notes
            .iter()
            .filter(|(o, _)| *o >= s && *o < e)
            .map(|(_, d)| *d)
            .collect();
        if !durs.is_empty() && !templates.contains(&durs) {
            templates.push(durs);
        }
    }
    templates
}

fn load_corpus(dir: &Path) -> (Vec<RhythmTemplate>, Vec<GestureStats>) {
    let mut rhythms: Vec<Vec<Ticks>> = Vec::new();
    let mut gestures: Vec<GestureStats> = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else {
        return (Vec::new(), Vec::new());
    };
    let mut names: Vec<String> = rd
        .filter_map(Result::ok)
        .filter_map(|e| e.file_name().to_str().map(ToOwned::to_owned))
        .filter(|n| n.ends_with(".chunk.json"))
        .collect();
    names.sort_unstable();
    for name in names {
        let Ok(txt) = std::fs::read_to_string(dir.join(&name)) else {
            continue;
        };
        let Ok(meta) = serde_json::from_str::<ChunkMeta>(&txt) else {
            continue;
        };
        if let Some(g) = meta.gesture {
            gestures.push(g);
        }
        let Ok(bytes) = std::fs::read(dir.join(&meta.source.filename)) else {
            continue;
        };
        let Ok(src) = import_score_auto(&bytes) else {
            continue;
        };
        let sliced = match meta.source.bar_range {
            Some((f, l)) => slice::extract_bars(&src, (f as usize)..(l as usize + 1)),
            None => src,
        };
        let Some(track) = first_note_track(&sliced) else {
            continue;
        };
        for t in bar_rhythms(&sliced, track) {
            if !rhythms.contains(&t) {
                rhythms.push(t);
            }
        }
    }
    let templates = rhythms
        .iter()
        .map(|d| RhythmTemplate::from_durations(d))
        .collect();
    (templates, gestures)
}

fn agg_gesture(stats: &[GestureStats]) -> Option<GestureControl> {
    let ctrls: Vec<GestureControl> = stats.iter().map(GestureControl::from_stats).collect();
    if ctrls.is_empty() {
        return None;
    }
    let n = ctrls.len() as f64;
    let mb = ctrls.iter().map(|c| c.burst_notes as f64).sum::<f64>() / n;
    let mr = ctrls.iter().map(|c| c.rest_quarters).sum::<f64>() / n;
    Some(GestureControl {
        burst_notes: (mb.round().max(1.0)) as usize,
        rest_quarters: mr.max(1.0),
    })
}

/// Register metrics of one candidate's line, as embeddable JSON fields.
fn register_fields(pitches: &[u8], in_lo: u8, in_hi: u8) -> String {
    if pitches.is_empty() {
        return "\"output_min\":0,\"output_max\":0,\"output_span\":0,\"range_utilization\":0.0,\"distinct_pitch\":0,\"distinct_pitch_classes\":0,\"lowest_octave_share\":0.0,\"highest_octave_share\":0.0,\"edge_low_share\":0.0,\"edge_high_share\":0.0,\"mean_abs_interval\":0.0,\"max_abs_interval\":0".to_string();
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
    let mut dp: Vec<u8> = pitches.to_vec();
    dp.sort_unstable();
    dp.dedup();
    let mut dpc: Vec<u8> = pitches.iter().map(|p| p % 12).collect();
    dpc.sort_unstable();
    dpc.dedup();
    let low_share = pitches
        .iter()
        .filter(|&&p| p <= omin.saturating_add(11))
        .count() as f64
        / n;
    let high_share = pitches
        .iter()
        .filter(|&&p| p >= omax.saturating_sub(11))
        .count() as f64
        / n;
    let edge_low = pitches
        .iter()
        .filter(|&&p| p <= in_lo.saturating_add(11))
        .count() as f64
        / n;
    let edge_high = pitches
        .iter()
        .filter(|&&p| p >= in_hi.saturating_sub(11))
        .count() as f64
        / n;
    let intervals: Vec<u32> = pitches
        .windows(2)
        .map(|w| u32::from(w[0].abs_diff(w[1])))
        .collect();
    let mean_int = if intervals.is_empty() {
        0.0
    } else {
        intervals.iter().map(|&i| f64::from(i)).sum::<f64>() / intervals.len() as f64
    };
    let max_int = intervals.iter().copied().max().unwrap_or(0);
    format!(
        "\"output_min\":{omin},\"output_max\":{omax},\"output_span\":{ospan},\"range_utilization\":{util:.3},\"distinct_pitch\":{},\"distinct_pitch_classes\":{},\"lowest_octave_share\":{low_share:.3},\"highest_octave_share\":{high_share:.3},\"edge_low_share\":{edge_low:.3},\"edge_high_share\":{edge_high:.3},\"mean_abs_interval\":{mean_int:.2},\"max_abs_interval\":{max_int}",
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
    let material = material_from(&in_pitches);
    let in_lo = in_pitches.iter().copied().min().unwrap();
    let in_hi = in_pitches.iter().copied().max().unwrap();
    let first_bar = src.master_bars.first().ok_or("input has no master bars")?;
    let constraints = GenerationConstraints {
        bar_count: 8,
        time_signature: first_bar.time_signature,
        tempo: first_bar.tempo,
        ticks_per_quarter: Ticks(u32::from(src.ticks_per_quarter)),
        pitch_lo: Pitch(in_lo),
        pitch_hi: Pitch(in_hi),
    };
    let (rhythms, gesture_stats) = load_corpus(Path::new(&corpus));
    let gesture_ctrl = agg_gesture(&gesture_stats);

    let iname = Path::new(&input)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("");

    // point 7 — input pitch-class material row
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
        "{{\"type\":\"input\",\"input\":{},\"min_pitch\":{in_lo},\"max_pitch\":{in_hi},\"input_span\":{},\"pitch_class_count\":{},\"pitch_classes_absolute\":{:?},\"pitch_classes_relative_to_min\":{:?},\"corpus_templates\":{},\"gesture_on\":{}}}",
        jstr(iname), in_hi - in_lo, pc_abs.len(), pc_abs, pc_rel, rhythms.len(), gesture_ctrl.is_some()
    );

    let gests: Vec<(&str, Option<GestureControl>)> = match gmode.as_str() {
        "on" => vec![("on", gesture_ctrl)],
        "off" => vec![("off", None)],
        _ => vec![("on", gesture_ctrl), ("off", None)],
    };

    for &seed in &seeds {
        for (glabel, gesture) in &gests {
            let req = SetRequest {
                seed: GenerationSeed(seed),
                pitch_material: material.clone(),
                constraints,
                source_rhythms: rhythms.clone(),
                variants_per_strategy: variants,
                gesture: *gesture,
            };
            match generate_candidate_set(&req) {
                Ok(cands) => {
                    for c in &cands {
                        let line = ordered_pitches(&c.score);
                        println!(
                            "{{\"type\":\"candidate\",\"input\":{},\"seed\":{seed},\"gesture\":\"{glabel}\",\"strategy\":\"{:?}\",\"variant_seed\":\"{}\",\"pitch_lo_constraint\":{in_lo},\"pitch_hi_constraint\":{in_hi},\"input_span\":{},{}}}",
                            jstr(iname), c.strategy, c.seed.0, in_hi - in_lo, register_fields(&line, in_lo, in_hi)
                        );
                    }
                }
                Err(e) => eprintln!("candidate set failed (seed {seed}, gesture {glabel}): {e:?}"),
            }
        }
    }
    Ok(())
}
