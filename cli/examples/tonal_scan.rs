#![allow(
    clippy::pedantic,
    clippy::restriction,
    clippy::nursery,
    clippy::complexity
)]
//! TonalContext diagnostic scan (read-only, throwaway experiment tooling).
//!
//! Imports a tab/MIDI and, at three evidence scopes — all tracks combined, each
//! track, each track's primary voice — reports pitch-class evidence (raw /
//! duration-weighted / onset-accent-weighted / first-bar / last-bar histograms)
//! plus two deterministic tonal baselines: a Krumhansl-Schmuckler major/minor
//! key-profile correlation over the duration-weighted histogram, and the
//! confidence margin (best score − second-best). Emits one JSON line per scope;
//! a final `type:"top5"` line ranks the five most-confident (scope,key) findings.
//!
//! NOT a key detector for production: no cadence, chord inference, or model.
//! Run: tonal_scan <input>
use griff_core::import::import_score_auto;
use griff_core::score::{AtomEvent, MasterBar, Score, Track};
use std::path::Path;

// Krumhansl-Kessler profiles.
const MAJOR: [f64; 12] = [
    6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88,
];
const MINOR: [f64; 12] = [
    6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17,
];
const PCN: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

fn pearson(a: &[f64], b: &[f64]) -> f64 {
    let n = a.len() as f64;
    let ma = a.iter().sum::<f64>() / n;
    let mb = b.iter().sum::<f64>() / n;
    let mut num = 0.0;
    let mut da = 0.0;
    let mut db = 0.0;
    for i in 0..a.len() {
        let (x, y) = (a[i] - ma, b[i] - mb);
        num += x * y;
        da += x * x;
        db += y * y;
    }
    if da <= 0.0 || db <= 0.0 {
        return 0.0;
    }
    num / (da.sqrt() * db.sqrt())
}

/// Ranked (tonic, mode, score) over the 24 keys on the duration-weighted hist.
fn score_keys(hist: &[f64; 12]) -> Vec<(usize, &'static str, f64)> {
    let mut scored = Vec::with_capacity(24);
    for t in 0..12 {
        let rot: Vec<f64> = (0..12).map(|i| hist[(i + t) % 12]).collect();
        scored.push((t, "major", pearson(&rot, &MAJOR)));
        scored.push((t, "minor", pearson(&rot, &MINOR)));
    }
    scored.sort_by(|x, y| y.2.partial_cmp(&x.2).unwrap_or(std::cmp::Ordering::Equal));
    scored
}

fn note_iter(track: &Track, voice_only_first: bool) -> Vec<(u8, u32, u32)> {
    let voices: Vec<_> = if voice_only_first {
        track.voices.iter().take(1).collect()
    } else {
        track.voices.iter().collect()
    };
    voices
        .iter()
        .flat_map(|v| &v.event_groups)
        .flat_map(|g| &g.atoms)
        .filter_map(|a| match a {
            AtomEvent::Note(n) => Some((n.pitch.0, n.absolute_start.0, n.duration.0)),
            AtomEvent::Rest(_) => None,
        })
        .collect()
}

fn bar_index(bars: &[MasterBar], onset: u32) -> Option<usize> {
    bars.iter()
        .position(|b| onset >= b.tick_range.start.0 && onset < b.tick_range.end.0)
}

fn hist_json(h: &[f64; 12]) -> String {
    let parts: Vec<String> = (0..12).map(|i| format!("{:.0}", h[i])).collect();
    format!("[{}]", parts.join(","))
}

/// Emits one scope record and returns its best (scope, tonic, mode, score, margin).
fn emit_scope(
    input: &str,
    scope: &str,
    name: &str,
    notes: &[(u8, u32, u32)],
    tpq: u32,
    bars: &[MasterBar],
) -> Option<(String, usize, &'static str, f64, f64)> {
    if notes.is_empty() {
        return None;
    }
    let mut raw = [0.0f64; 12];
    let mut dur = [0.0f64; 12];
    let mut onset = [0.0f64; 12]; // metric-accent weight (on-beat notes count double)
    for &(p, o, d) in notes {
        let pc = (p % 12) as usize;
        raw[pc] += 1.0;
        dur[pc] += f64::from(d);
        onset[pc] += if tpq > 0 && o % tpq == 0 { 2.0 } else { 1.0 };
    }
    // first / last sounding bar histograms
    let bidx: Vec<usize> = notes
        .iter()
        .filter_map(|&(_, o, _)| bar_index(bars, o))
        .collect();
    let (first_b, last_b) = (bidx.iter().copied().min(), bidx.iter().copied().max());
    let mut firstbar = [0.0f64; 12];
    let mut lastbar = [0.0f64; 12];
    for &(p, o, _) in notes {
        if let Some(bi) = bar_index(bars, o) {
            if Some(bi) == first_b {
                firstbar[(p % 12) as usize] += 1.0;
            }
            if Some(bi) == last_b {
                lastbar[(p % 12) as usize] += 1.0;
            }
        }
    }
    let note_count = notes.len();
    let sounding: u64 = notes.iter().map(|&(_, _, d)| u64::from(d)).sum();
    let lo = notes.iter().map(|&(p, _, _)| p).min().unwrap();
    let hi = notes.iter().map(|&(p, _, _)| p).max().unwrap();
    let distinct_pc = raw.iter().filter(|&&c| c > 0.0).count();

    let ranked = score_keys(&dur);
    let (tonic, mode, score) = ranked[0];
    let margin = score - ranked[1].2;

    println!(
        "{{\"type\":\"scope\",\"input\":\"{}\",\"scope\":\"{}\",\"name\":\"{}\",\"note_count\":{},\"sounding_ticks\":{},\"pitch_lo\":{},\"pitch_hi\":{},\"pitch_range\":{},\"distinct_pc\":{},\"raw_pc\":{},\"dur_pc\":{},\"onset_pc\":{},\"first_bar_pc\":{},\"last_bar_pc\":{},\"key_tonic_pc\":{},\"key_tonic\":\"{}\",\"key_mode\":\"{}\",\"key_score\":{:.4},\"confidence_margin\":{:.4}}}",
        input.replace('"', "'"), scope, name.replace('"', "'"), note_count, sounding, lo, hi, hi - lo, distinct_pc,
        hist_json(&raw), hist_json(&dur), hist_json(&onset), hist_json(&firstbar), hist_json(&lastbar),
        tonic, PCN[tonic], mode, score, margin
    );
    Some((scope.to_string(), tonic, mode, score, margin))
}

fn main() -> Result<(), String> {
    let input = std::env::args().nth(1).ok_or("usage: tonal_scan <input>")?;
    let bytes = std::fs::read(&input).map_err(|e| format!("read '{input}': {e}"))?;
    let score: Score = import_score_auto(&bytes).map_err(|e| format!("import '{input}': {e}"))?;
    let iname = Path::new(&input)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    let tpq = u32::from(score.ticks_per_quarter);
    let bars = &score.master_bars;

    let mut best: Vec<(String, usize, &'static str, f64, f64)> = Vec::new();

    // scope 1 — all tracks combined
    let all: Vec<(u8, u32, u32)> = score
        .tracks
        .iter()
        .flat_map(|t| note_iter(t, false))
        .collect();
    if let Some(b) = emit_scope(iname, "all_tracks", "all", &all, tpq, bars) {
        best.push(b);
    }
    // scope 2/3 — each track, each primary voice
    for (i, t) in score.tracks.iter().enumerate() {
        let nm = format!("t{i}:{}", t.name.as_deref().unwrap_or("<unnamed>"));
        if let Some(b) = emit_scope(
            iname,
            &format!("track_{i}"),
            &nm,
            &note_iter(t, false),
            tpq,
            bars,
        ) {
            best.push(b);
        }
        if let Some(b) = emit_scope(
            iname,
            &format!("track_{i}_v0"),
            &nm,
            &note_iter(t, true),
            tpq,
            bars,
        ) {
            best.push(b);
        }
    }

    // top-5 most-confident (scope,key) findings
    best.sort_by(|x, y| y.4.partial_cmp(&x.4).unwrap_or(std::cmp::Ordering::Equal));
    let top: Vec<String> = best.iter().take(5).map(|(sc, t, m, s, mg)| {
        format!("{{\"scope\":\"{sc}\",\"tonic\":\"{}\",\"mode\":\"{m}\",\"score\":{s:.4},\"margin\":{mg:.4}}}", PCN[*t])
    }).collect();
    let all_distinct = {
        let mut pc = [false; 12];
        for &(p, _, _) in &all {
            pc[(p % 12) as usize] = true;
        }
        pc.iter().filter(|&&b| b).count()
    };
    println!(
        "{{\"type\":\"top5\",\"input\":\"{}\",\"all_tracks_distinct_pc\":{},\"scopes\":{},\"top5\":[{}]}}",
        iname.replace('"', "'"), all_distinct, best.len(), top.join(",")
    );
    Ok(())
}
