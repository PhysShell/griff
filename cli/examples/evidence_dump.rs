#![allow(
    clippy::pedantic,
    clippy::restriction,
    clippy::nursery,
    clippy::complexity
)]
//! Phase-1 evidence/inference dump via the NEW core tonal API (after arm only).
//!
//! For WholeScore / each Track / each Voice, prints `PitchEvidence::measure`
//! fields (note_count, pitch_range, onset_counts, duration_mass) and the
//! `estimate_key` winner (tonic, mode, correlation, confidence_margin) — with
//! exact float bits — so it can be checked against the frozen prototype
//! (`tonal_evidence.jsonl`) and for histogram additivity / 24 candidates.
use griff_core::import::import_score_auto;
use griff_core::tonal::{estimate_key, EvidenceScope, PitchEvidence};
use std::path::Path;

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

fn emit(iname: &str, label: &str, ev: &PitchEvidence) {
    let (lo, hi) = ev.pitch_range.map_or((0, 0), |r| (r.lowest.0, r.highest.0));
    let has_range = ev.pitch_range.is_some();
    match estimate_key(ev) {
        Some(est) => {
            let w = est.winner().expect("non-empty candidates");
            println!(
                "{{\"input\":\"{iname}\",\"scope\":\"{label}\",\"note_count\":{},\"has_range\":{has_range},\"pitch_lo\":{lo},\"pitch_hi\":{hi},\"onset_counts\":{},\"duration_mass\":{},\"winner_tonic\":{},\"winner_mode\":\"{:?}\",\"winner_correlation\":{:.6},\"winner_correlation_bits\":{},\"confidence_margin\":{:.6},\"confidence_margin_bits\":{},\"n_candidates\":{}}}",
                ev.note_count, u32h(&ev.onset_counts), u64h(&ev.duration_mass),
                w.tonic, w.mode, w.correlation, w.correlation.to_bits(),
                est.confidence_margin, est.confidence_margin.to_bits(), est.candidates.len()
            );
        }
        None => println!(
            "{{\"input\":\"{iname}\",\"scope\":\"{label}\",\"note_count\":{},\"has_range\":{has_range},\"onset_counts\":{},\"duration_mass\":{},\"key\":\"none\"}}",
            ev.note_count, u32h(&ev.onset_counts), u64h(&ev.duration_mass)
        ),
    }
}

fn main() -> Result<(), String> {
    let input = std::env::args()
        .nth(1)
        .ok_or("usage: evidence_dump <input>")?;
    let bytes = std::fs::read(&input).map_err(|e| format!("read '{input}': {e}"))?;
    let score = import_score_auto(&bytes).map_err(|e| format!("import '{input}': {e}"))?;
    let iname = Path::new(&input)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .replace('"', "'");

    emit(
        &iname,
        "WholeScore",
        &PitchEvidence::measure(&score, EvidenceScope::WholeScore),
    );
    for i in 0..score.tracks.len() {
        emit(
            &iname,
            &format!("Track_{i}"),
            &PitchEvidence::measure(&score, EvidenceScope::Track(i)),
        );
    }
    for (i, t) in score.tracks.iter().enumerate() {
        for v in 0..t.voices.len() {
            emit(
                &iname,
                &format!("Voice_{i}_{v}"),
                &PitchEvidence::measure(&score, EvidenceScope::Voice { track: i, voice: v }),
            );
        }
    }
    Ok(())
}
