#![allow(
    clippy::pedantic,
    clippy::restriction,
    clippy::nursery,
    clippy::complexity
)]
//! TonalContext diagnostic scan (read-only, throwaway experiment tooling).
//!
//! Delegates all evidence and key inference to the shared core API
//! (`griff_core::tonal::{PitchEvidence::measure, estimate_key}`) — it does NOT
//! implement its own key estimator (no KK profiles, Pearson, or 24-key ranking
//! here). It adds only two explicitly named local **diagnostics** that the core
//! evidence deliberately omits: a metric-accent-weighted histogram
//! (`onset_accent_pc`, on-beat notes ×2) and first/last-bar histograms.
//!
//! Emits one `type:"scope"` line per WholeScore / Track / Voice, then a `top5`
//! line ranking the five most-confident (scope,key) hypotheses (not verified
//! keys). No cadence, chord inference, or model.
//! Run: tonal_scan <input>
use griff_core::import::import_score_auto;
use griff_core::score::{AtomEvent, MasterBar, Score};
use griff_core::tonal::{estimate_key, EvidenceScope, PitchEvidence};
use std::path::Path;

const PCN: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

/// (pitch, onset) of a scope's notes — for the LOCAL diagnostics only; the core
/// histograms come from `PitchEvidence::measure`.
fn scope_notes(score: &Score, scope: EvidenceScope) -> Vec<(u8, u32)> {
    let mut out = Vec::new();
    let mut push_voice = |v: &griff_core::score::Voice| {
        for a in v.event_groups.iter().flat_map(|g| &g.atoms) {
            if let AtomEvent::Note(n) = a {
                out.push((n.pitch.0, n.absolute_start.0));
            }
        }
    };
    match scope {
        EvidenceScope::WholeScore => {
            for t in &score.tracks {
                for v in &t.voices {
                    push_voice(v);
                }
            }
        }
        EvidenceScope::Track(i) => {
            if let Some(t) = score.tracks.get(i) {
                for v in &t.voices {
                    push_voice(v);
                }
            }
        }
        EvidenceScope::Voice { track, voice } => {
            if let Some(v) = score.tracks.get(track).and_then(|t| t.voices.get(voice)) {
                push_voice(v);
            }
        }
    }
    out
}

fn onset_accent(notes: &[(u8, u32)], tpq: u32) -> [u32; 12] {
    let mut h = [0u32; 12];
    for &(p, o) in notes {
        h[(p % 12) as usize] += if tpq > 0 && o % tpq == 0 { 2 } else { 1 };
    }
    h
}

fn bar_index(bars: &[MasterBar], onset: u32) -> Option<usize> {
    bars.iter()
        .position(|b| onset >= b.tick_range.start.0 && onset < b.tick_range.end.0)
}

fn bar_hist(notes: &[(u8, u32)], bars: &[MasterBar], target: Option<usize>) -> [u32; 12] {
    let mut h = [0u32; 12];
    for &(p, o) in notes {
        if bar_index(bars, o) == target {
            h[(p % 12) as usize] += 1;
        }
    }
    h
}

fn u32h(h: &[u32; 12]) -> String {
    format!(
        "[{}]",
        h.iter().map(u32::to_string).collect::<Vec<_>>().join(",")
    )
}
fn u64h(h: &[u64; 12]) -> String {
    format!(
        "[{}]",
        h.iter().map(u64::to_string).collect::<Vec<_>>().join(",")
    )
}

fn emit(
    iname: &str,
    label: &str,
    score: &Score,
    scope: EvidenceScope,
    tpq: u32,
    bars: &[MasterBar],
) -> Option<(String, u8, &'static str, f64, f64)> {
    let ev = PitchEvidence::measure(score, scope);
    if ev.note_count == 0 {
        return None;
    }
    let est = estimate_key(&ev)?;
    let w = est.winner()?;
    let mode = match w.mode {
        griff_core::tonal::KeyMode::Major => "major",
        griff_core::tonal::KeyMode::Minor => "minor",
    };
    // local diagnostics
    let notes = scope_notes(score, scope);
    let bidx: Vec<usize> = notes
        .iter()
        .filter_map(|&(_, o)| bar_index(bars, o))
        .collect();
    let (fb, lb) = (bidx.iter().copied().min(), bidx.iter().copied().max());
    let (lo, hi) = ev.pitch_range.map_or((0, 0), |r| (r.lowest.0, r.highest.0));
    let distinct_pc = ev.onset_counts.iter().filter(|&&c| c > 0).count();

    println!(
        "{{\"input\":\"{iname}\",\"scope\":\"{label}\",\"note_count\":{},\"sounding_ticks\":{},\"pitch_lo\":{lo},\"pitch_hi\":{hi},\"distinct_pc\":{distinct_pc},\"raw_pc\":{},\"dur_pc\":{},\"onset_accent_pc\":{},\"first_bar_pc\":{},\"last_bar_pc\":{},\"key_tonic_pc\":{},\"key_tonic\":\"{}\",\"key_mode\":\"{mode}\",\"key_correlation\":{:.6},\"confidence_margin\":{:.6},\"n_candidates\":{}}}",
        ev.note_count, ev.duration_mass.iter().sum::<u64>(),
        u32h(&ev.onset_counts), u64h(&ev.duration_mass), u32h(&onset_accent(&notes, tpq)),
        u32h(&bar_hist(&notes, bars, fb)), u32h(&bar_hist(&notes, bars, lb)),
        w.tonic, PCN[w.tonic as usize], w.correlation, est.confidence_margin, est.candidates.len()
    );
    Some((
        label.to_string(),
        w.tonic,
        mode,
        w.correlation,
        est.confidence_margin,
    ))
}

fn main() -> Result<(), String> {
    let input = std::env::args().nth(1).ok_or("usage: tonal_scan <input>")?;
    let bytes = std::fs::read(&input).map_err(|e| format!("read '{input}': {e}"))?;
    let score: Score = import_score_auto(&bytes).map_err(|e| format!("import '{input}': {e}"))?;
    let iname = Path::new(&input)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .replace('"', "'");
    let tpq = u32::from(score.ticks_per_quarter);
    let bars = &score.master_bars;

    let mut best: Vec<(String, u8, &'static str, f64, f64)> = Vec::new();
    if let Some(b) = emit(
        &iname,
        "WholeScore",
        &score,
        EvidenceScope::WholeScore,
        tpq,
        bars,
    ) {
        best.push(b);
    }
    for i in 0..score.tracks.len() {
        if let Some(b) = emit(
            &iname,
            &format!("Track_{i}"),
            &score,
            EvidenceScope::Track(i),
            tpq,
            bars,
        ) {
            best.push(b);
        }
    }
    for (i, t) in score.tracks.iter().enumerate() {
        for v in 0..t.voices.len() {
            if let Some(b) = emit(
                &iname,
                &format!("Voice_{i}_{v}"),
                &score,
                EvidenceScope::Voice { track: i, voice: v },
                tpq,
                bars,
            ) {
                best.push(b);
            }
        }
    }

    best.sort_by(|x, y| y.4.partial_cmp(&x.4).unwrap_or(std::cmp::Ordering::Equal));
    let top: Vec<String> = best.iter().take(5).map(|(sc, t, m, c, mg)| {
        format!("{{\"scope\":\"{sc}\",\"tonic\":\"{}\",\"mode\":\"{m}\",\"correlation\":{c:.6},\"margin\":{mg:.6}}}", PCN[*t as usize])
    }).collect();
    let all = PitchEvidence::measure(&score, EvidenceScope::WholeScore);
    let all_distinct = all.onset_counts.iter().filter(|&&c| c > 0).count();
    println!(
        "{{\"type\":\"top5\",\"input\":\"{iname}\",\"all_tracks_distinct_pc\":{all_distinct},\"scopes\":{},\"top5\":[{}]}}",
        best.len(), top.join(",")
    );
    Ok(())
}
