//! Structural A/B analysis of a generated riff (throwaway tooling). Imports a
//! generated MIDI and reports, as one JSON line: closure axes (ADR-0017),
//! novelty axes against the corpus, density (notes/bar, rests, distinct
//! durations, non-quarter fraction), and register (pitch span/mean/distinct).
//! Pitch material and novelty references mirror how `griff generate` scores, so
//! the numbers are comparable across corpus vs no-corpus outputs.
//!
//! Run: analyze <generated.mid> --input <tab> [--corpus <dir>]
use griff_core::corpus::ChunkMeta;
use griff_core::event::{Pitch, Ticks};
use griff_core::generate::PitchMaterial;
use griff_core::import::import_score_auto;
use griff_core::score::{AtomEvent, Score};
use griff_core::{closure, novelty, slice};
use std::path::Path;

fn first_note_track(score: &Score) -> Option<usize> {
    score.tracks.iter().position(|t| {
        t.voices
            .first()
            .is_some_and(|v| v.event_groups.iter().flat_map(|g| &g.atoms).any(|a| matches!(a, AtomEvent::Note(_))))
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
    PitchMaterial { root: Pitch(lo), intervals }
}

/// Notes + rests of the chosen track's primary voice, for density/register.
fn track_atoms(score: &Score, track: usize) -> (Vec<u8>, Vec<u32>, usize) {
    let mut pitches = Vec::new();
    let mut durs = Vec::new();
    let mut rests = 0usize;
    if let Some(v) = score.tracks.get(track).and_then(|t| t.voices.first()) {
        for a in v.event_groups.iter().flat_map(|g| &g.atoms) {
            match a {
                AtomEvent::Note(n) => {
                    pitches.push(n.pitch.0);
                    durs.push(n.duration.0);
                }
                AtomEvent::Rest(_) => rests += 1,
            }
        }
    }
    (pitches, durs, rests)
}

fn load_references(dir: &Path) -> Vec<Score> {
    let mut refs = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else { return refs };
    let mut names: Vec<String> = rd
        .filter_map(Result::ok)
        .filter_map(|e| e.file_name().to_str().map(ToOwned::to_owned))
        .filter(|n| n.ends_with(".chunk.json"))
        .collect();
    names.sort_unstable();
    for name in names {
        let Ok(txt) = std::fs::read_to_string(dir.join(&name)) else { continue };
        let Ok(meta) = serde_json::from_str::<ChunkMeta>(&txt) else { continue };
        let Ok(bytes) = std::fs::read(dir.join(&meta.source.filename)) else { continue };
        let Ok(src) = import_score_auto(&bytes) else { continue };
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

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let midi = args.get(1).expect("usage: analyze <mid> --input <tab> [--corpus <dir>]").clone();
    let input = args.iter().position(|a| a == "--input").and_then(|i| args.get(i + 1)).expect("--input <tab> required").clone();
    let corpus = args.iter().position(|a| a == "--corpus").and_then(|i| args.get(i + 1)).cloned();

    let gen = import_score_auto(&std::fs::read(&midi).expect("read midi")).expect("import midi");
    let src = import_score_auto(&std::fs::read(&input).expect("read input")).expect("import input");
    let material = material_from(&all_pitches(&src)); // input's tonal frame, as generation used
    let track = first_note_track(&gen).expect("no note-bearing track");

    let (pitches, durs, rests) = track_atoms(&gen, track);
    let bars = gen.master_bars.len().max(1);
    let tpq = u32::from(gen.ticks_per_quarter);
    let notes = pitches.len();
    let mut distinct_dur: Vec<u32> = durs.clone();
    distinct_dur.sort_unstable();
    distinct_dur.dedup();
    let nonquarter = durs.iter().filter(|&&d| d != tpq).count();
    let lo = pitches.iter().copied().min().unwrap_or(0);
    let hi = pitches.iter().copied().max().unwrap_or(0);
    let mean = if notes > 0 { pitches.iter().map(|&p| p as f64).sum::<f64>() / notes as f64 } else { 0.0 };
    let mut distinct_p: Vec<u8> = pitches.clone();
    distinct_p.sort_unstable();
    distinct_p.dedup();

    let closure_axes = closure::closure_axes(&gen, track, &material).map(|a| axes_json(&a)).unwrap_or_default();
    let novelty_axes = corpus
        .as_ref()
        .map(|d| {
            let refs = load_references(Path::new(d));
            novelty::measure_novelty(&gen, track, &refs).map(|r| axes_json(&novelty::novelty_axes(&r))).unwrap_or_default()
        })
        .unwrap_or_default();

    println!(
        "{{\"midi\":\"{}\",\"notes\":{},\"bars\":{},\"notes_per_bar\":{:.2},\"rests\":{},\"distinct_dur\":{},\"nonquarter\":{},\"nonquarter_frac\":{:.2},\"pitch_lo\":{},\"pitch_hi\":{},\"pitch_span\":{},\"pitch_mean\":{:.1},\"distinct_pitch\":{},\"closure\":{{{}}},\"novelty\":{{{}}}}}",
        Path::new(&midi).file_name().and_then(|s| s.to_str()).unwrap_or(""),
        notes, bars, notes as f64 / bars as f64, rests, distinct_dur.len(), nonquarter,
        if notes > 0 { nonquarter as f64 / notes as f64 } else { 0.0 },
        lo, hi, hi.saturating_sub(lo), mean, distinct_p.len(),
        closure_axes, novelty_axes
    );
}
