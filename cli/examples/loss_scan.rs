//! Corpus-prep import scan (ADR-0020 LossReport-based). Throwaway tooling: walks
//! a directory for Guitar Pro tabs, imports each via `griff_core::import`, and
//! emits one JSON line per file — import status (clean / lossy / error), the
//! LossReport warning kinds, and coarse size (bars / tracks / notes) for later
//! subset selection. Prints nothing derived that embeds corpus note content.
//!
//! Run: `cargo run --release -p griff-cli --example loss_scan -- <dir>`
use griff_core::import::import_score_auto;
use griff_core::score::{AtomEvent, ImportWarning, Score};
use std::path::{Path, PathBuf};

fn warn_kind(w: &ImportWarning) -> String {
    match w {
        ImportWarning::TrackNameInvalidUtf8 { .. } => "TrackNameInvalidUtf8".into(),
        ImportWarning::SmpteTimingUnsupported => "SmpteTimingUnsupported".into(),
        ImportWarning::Other(s) => format!("Other:{s}"),
    }
}

fn note_count(score: &Score) -> usize {
    score
        .tracks
        .iter()
        .flat_map(|t| &t.voices)
        .flat_map(|v| &v.event_groups)
        .flat_map(|g| &g.atoms)
        .filter(|a| matches!(a, AtomEvent::Note(_)))
        .count()
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                collect(&p, out);
            } else if let Some(ext) = p.extension().and_then(|s| s.to_str()) {
                let ext = ext.to_ascii_lowercase();
                if matches!(ext.as_str(), "gp3" | "gp4" | "gp5" | "gpx" | "gp") {
                    out.push(p);
                }
            }
        }
    }
}

fn jesc(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn main() {
    let dir = std::env::args().nth(1).expect("usage: loss_scan <dir>");
    let mut files = Vec::new();
    collect(Path::new(&dir), &mut files);
    files.sort();
    for p in &files {
        let ext = p
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
        let data = match std::fs::read(p) {
            Ok(d) => d,
            Err(e) => {
                println!(
                    "{{\"file\":\"{}\",\"ext\":\"{}\",\"status\":\"error\",\"err\":\"read:{}\"}}",
                    jesc(name),
                    ext,
                    jesc(&e.to_string())
                );
                continue;
            }
        };
        match import_score_auto(&data) {
            Err(e) => {
                println!(
                    "{{\"file\":\"{}\",\"ext\":\"{}\",\"status\":\"error\",\"err\":\"{}\"}}",
                    jesc(name),
                    ext,
                    jesc(&e.to_string())
                );
            }
            Ok(score) => {
                let warnings: Vec<String> = score.loss.warnings.iter().map(warn_kind).collect();
                let status = if warnings.is_empty() { "clean" } else { "lossy" };
                let kinds = warnings
                    .iter()
                    .map(|k| format!("\"{}\"", jesc(k)))
                    .collect::<Vec<_>>()
                    .join(",");
                println!(
                    "{{\"file\":\"{}\",\"ext\":\"{}\",\"status\":\"{}\",\"bars\":{},\"tracks\":{},\"notes\":{},\"warnings\":[{}]}}",
                    jesc(name),
                    ext,
                    status,
                    score.master_bars.len(),
                    score.tracks.len(),
                    note_count(&score),
                    kinds
                );
            }
        }
    }
    eprintln!("scanned {} files", files.len());
}
