#![allow(clippy::pedantic, clippy::restriction, clippy::nursery)]
//! Structural analysis of a generated riff (throwaway corpus-experiment tooling).
//! Imports a generated MIDI and reports, as one JSON line: rhythm axes (distinct
//! durations, distinct PER-BAR rhythm signatures, notes/bar spread), closure axes
//! (ADR-0017), novelty axes vs the corpus, density, and register (incl. the INPUT
//! tab's own pitch range). Novelty is surfaced explicitly — an object when
//! computed, else `null` plus a `novelty_error` reason (never a silent empty).
//!
//! Run: analyze <generated.mid> --input <tab> [--corpus <dir>]
use griff_core::corpus::ChunkMeta;
use griff_core::event::Pitch;
use griff_core::generate::PitchMaterial;
use griff_core::import::import_score_auto;
use griff_core::score::{AtomEvent, Score};
use griff_core::{closure, novelty, slice};
use std::collections::HashSet;
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

fn track_notes(score: &Score, track: usize) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    if let Some(v) = score.tracks.get(track).and_then(|t| t.voices.first()) {
        for a in v.event_groups.iter().flat_map(|g| &g.atoms) {
            if let AtomEvent::Note(n) = a {
                out.push((n.absolute_start.0, n.duration.0));
            }
        }
    }
    out
}

fn per_bar_rhythm(score: &Score, track: usize) -> (Vec<usize>, usize, usize) {
    let notes = track_notes(score, track);
    let mut per_bar_notes = Vec::new();
    let mut sigs: HashSet<String> = HashSet::new();
    let mut sounding = 0usize;
    for bar in &score.master_bars {
        let (s, e) = (bar.tick_range.start.0, bar.tick_range.end.0);
        let mut in_bar: Vec<(u32, u32)> = notes
            .iter()
            .filter(|(o, _)| *o >= s && *o < e)
            .copied()
            .collect();
        in_bar.sort_by_key(|(o, _)| *o);
        per_bar_notes.push(in_bar.len());
        if !in_bar.is_empty() {
            sounding += 1;
            let sig: Vec<(u32, u32)> = in_bar.iter().map(|(o, d)| (o - s, *d)).collect();
            sigs.insert(format!("{sig:?}"));
        }
    }
    (per_bar_notes, sigs.len(), sounding)
}

fn load_references(dir: &Path) -> Vec<Score> {
    let mut refs = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else {
        return refs;
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
        refs.push(sliced);
    }
    refs
}

fn axes_json(axes: &griff_core::scoring::Axes) -> String {
    axes.iter()
        .map(|a| format!("\"{}\":{:.4}", a.label, a.value))
        .collect::<Vec<_>>()
        .join(",")
}

fn jstr(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "'"))
}

/// Ordered (by onset) output pitches of the chosen track.
fn ordered_out_pitches(score: &Score, track: usize) -> Vec<u8> {
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

fn ladder_ends(in_lo: u8, in_hi: u8, palette: &[u8]) -> (u8, u8) {
    let rungs: Vec<u8> = (in_lo..=in_hi)
        .filter(|p| palette.contains(&(p % 12)))
        .collect();
    (
        *rungs.first().unwrap_or(&in_lo),
        *rungs.last().unwrap_or(&in_lo),
    )
}

/// Register + saturation + jump metrics (same fields as register_scan), for the
/// winner-level production A/B. `palette` = the input's pitch classes.
fn register_fields(pitches: &[u8], in_lo: u8, in_hi: u8, palette: &[u8]) -> String {
    if pitches.is_empty() {
        return "\"output_span\":0,\"exact_high_share\":0.0,\"mode_pitch_share\":0.0,\"mean_abs_interval\":0.0,\"max_abs_interval\":0,\"largest_upward_interval\":0,\"largest_downward_interval\":0,\"octave_leap_share\":0.0,\"pitch_stddev\":0.0,\"in_class_rate\":0.0".to_string();
    }
    let n = pitches.len() as f64;
    let omin = *pitches.iter().min().unwrap();
    let omax = *pitches.iter().max().unwrap();
    let (_lad_lo, lad_hi) = ladder_ends(in_lo, in_hi, palette);
    let exact_high = pitches.iter().filter(|&&p| p == lad_hi).count() as f64 / n;
    let in_class = pitches
        .iter()
        .filter(|&&p| palette.contains(&(p % 12)))
        .count() as f64
        / n;
    let mut counts = std::collections::HashMap::new();
    for &p in pitches {
        *counts.entry(p).or_insert(0u32) += 1;
    }
    let mode_share = f64::from(counts.values().copied().max().unwrap_or(0)) / n;
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
    let oct_leap_share = if absint.is_empty() {
        0.0
    } else {
        absint.iter().filter(|&&i| i >= 12).count() as f64 / absint.len() as f64
    };
    let pmean = pitches.iter().map(|&p| f64::from(p)).sum::<f64>() / n;
    let pstd = (pitches
        .iter()
        .map(|&p| (f64::from(p) - pmean).powi(2))
        .sum::<f64>()
        / n)
        .sqrt();
    format!(
        "\"output_span\":{},\"exact_high_share\":{exact_high:.3},\"mode_pitch_share\":{mode_share:.3},\"mean_abs_interval\":{mean_int:.2},\"max_abs_interval\":{max_int},\"largest_upward_interval\":{up},\"largest_downward_interval\":{down},\"octave_leap_share\":{oct_leap_share:.3},\"pitch_stddev\":{pstd:.2},\"in_class_rate\":{in_class:.3}",
        omax.saturating_sub(omin)
    )
}

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    let midi = args
        .get(1)
        .filter(|s| !s.starts_with("--"))
        .ok_or("usage: analyze <mid> --input <tab> [--corpus <dir>]")?
        .clone();
    let input = arg_after(&args, "--input").ok_or("missing required --input <tab>")?;
    let corpus = arg_after(&args, "--corpus");

    let gbytes = std::fs::read(&midi).map_err(|e| format!("read midi '{midi}': {e}"))?;
    let generated = import_score_auto(&gbytes).map_err(|e| format!("import midi '{midi}': {e}"))?;
    let sbytes = std::fs::read(&input).map_err(|e| format!("read input '{input}': {e}"))?;
    let src = import_score_auto(&sbytes).map_err(|e| format!("import input '{input}': {e}"))?;

    let in_pitches = all_pitches(&src);
    let material = material_from(&in_pitches);
    let in_lo = in_pitches.iter().copied().min().unwrap_or(0);
    let in_hi = in_pitches.iter().copied().max().unwrap_or(0);
    let track = first_note_track(&generated).ok_or("generated MIDI has no note-bearing track")?;

    let notes_od = track_notes(&generated, track);
    let bars = generated.master_bars.len().max(1);
    let tpq = u32::from(generated.ticks_per_quarter);
    let notes = notes_od.len();
    let durs: Vec<u32> = notes_od.iter().map(|(_, d)| *d).collect();
    let mut distinct_dur: Vec<u32> = durs.clone();
    distinct_dur.sort_unstable();
    distinct_dur.dedup();
    let nonquarter = durs.iter().filter(|&&d| d != tpq).count();

    let (per_bar_notes, distinct_bar_rhythms, sounding_bars) = per_bar_rhythm(&generated, track);
    let npb_min = per_bar_notes.iter().copied().min().unwrap_or(0);
    let npb_max = per_bar_notes.iter().copied().max().unwrap_or(0);
    let npb_mean = per_bar_notes.iter().map(|&n| n as f64).sum::<f64>() / bars as f64;
    let npb_std = (per_bar_notes
        .iter()
        .map(|&n| (n as f64 - npb_mean).powi(2))
        .sum::<f64>()
        / bars as f64)
        .sqrt();

    let out_pitches = all_pitches(&generated);
    let lo = out_pitches.iter().copied().min().unwrap_or(0);
    let hi = out_pitches.iter().copied().max().unwrap_or(0);
    let mean = if out_pitches.is_empty() {
        0.0
    } else {
        out_pitches.iter().map(|&p| p as f64).sum::<f64>() / out_pitches.len() as f64
    };
    let mut distinct_p: Vec<u8> = out_pitches.clone();
    distinct_p.sort_unstable();
    distinct_p.dedup();

    let closure_axes = closure::closure_axes(&generated, track, &material)
        .map(|a| axes_json(&a))
        .unwrap_or_default();

    // Novelty surfaced explicitly — object when computed, else null + reason.
    let (novelty_json, novelty_err) = match &corpus {
        None => ("null".to_string(), Some("no --corpus given".to_string())),
        Some(d) => {
            let refs = load_references(Path::new(d));
            if refs.is_empty() {
                (
                    "null".to_string(),
                    Some(format!("no chunk references loaded from '{d}'")),
                )
            } else {
                match novelty::measure_novelty(&generated, track, &refs) {
                    Ok(r) => (
                        format!("{{{}}}", axes_json(&novelty::novelty_axes(&r))),
                        None,
                    ),
                    Err(e) => ("null".to_string(), Some(format!("{e:?}"))),
                }
            }
        }
    };
    let nerr = novelty_err.map_or_else(|| "null".to_string(), |e| jstr(&e));

    // register block (point-5 metrics) over the ordered output line
    let mut palette: Vec<u8> = in_pitches.iter().map(|p| p % 12).collect();
    palette.sort_unstable();
    palette.dedup();
    let out_line = ordered_out_pitches(&generated, track);
    let register = register_fields(&out_line, in_lo, in_hi, &palette);

    println!(
        "{{\"midi\":{},\"notes\":{},\"bars\":{},\"distinct_dur\":{},\"distinct_bar_rhythms\":{},\"sounding_bars\":{},\"npb_min\":{},\"npb_max\":{},\"npb_mean\":{:.2},\"npb_std\":{:.2},\"nonquarter\":{},\"pitch_lo\":{},\"pitch_hi\":{},\"pitch_span\":{},\"pitch_mean\":{:.1},\"distinct_pitch\":{},\"in_lo\":{},\"in_hi\":{},\"in_span\":{},\"register\":{{{}}},\"closure\":{{{}}},\"novelty\":{},\"novelty_error\":{}}}",
        jstr(Path::new(&midi).file_name().and_then(|s| s.to_str()).unwrap_or("")),
        notes, bars, distinct_dur.len(), distinct_bar_rhythms, sounding_bars,
        npb_min, npb_max, npb_mean, npb_std, nonquarter,
        lo, hi, hi.saturating_sub(lo), mean, distinct_p.len(),
        in_lo, in_hi, in_hi.saturating_sub(in_lo),
        register, closure_axes, novelty_json, nerr
    );
    Ok(())
}
