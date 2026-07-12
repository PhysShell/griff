//! Structural analysis of a generated riff (throwaway tooling). Imports a
//! generated MIDI and reports, as one JSON line: rhythm axes (distinct durations
//! across the piece, distinct PER-BAR rhythm signatures, notes/bar spread across
//! bars), closure axes (ADR-0017), novelty axes vs the corpus, density, and
//! register — including the INPUT tab's own pitch range, to tell a narrow
//! generator slice (bug) from a genuinely narrow tab (data). Pitch material and
//! novelty references mirror `griff generate`, so corpus-vs-no-corpus outputs
//! compare.
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

/// (onset,duration) of every note in the chosen track's primary voice.
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

/// Per-bar rhythm diversity of the generated piece: notes/bar over ALL bars
/// (min/max/mean/std, so rests-as-gaps show as spread), and the count of
/// DISTINCT per-bar rhythm signatures over sounding bars (a bar's signature is
/// its notes' bar-relative (onset,duration) sequence). Both collapse to 1 when
/// one template drives every bar — the hole template rotation must widen.
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

#[allow(clippy::cast_precision_loss)]
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let midi = args
        .get(1)
        .expect("usage: analyze <mid> --input <tab> [--corpus <dir>]")
        .clone();
    let input = args
        .iter()
        .position(|a| a == "--input")
        .and_then(|i| args.get(i + 1))
        .expect("--input <tab> required")
        .clone();
    let corpus = args
        .iter()
        .position(|a| a == "--corpus")
        .and_then(|i| args.get(i + 1))
        .cloned();

    let generated =
        import_score_auto(&std::fs::read(&midi).expect("read midi")).expect("import midi");
    let src = import_score_auto(&std::fs::read(&input).expect("read input")).expect("import input");
    let in_pitches = all_pitches(&src);
    let material = material_from(&in_pitches); // input's tonal frame, as generation used
    let in_lo = in_pitches.iter().copied().min().unwrap_or(0);
    let in_hi = in_pitches.iter().copied().max().unwrap_or(0);
    let track = first_note_track(&generated).expect("no note-bearing track");

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
    let novelty_axes = corpus
        .as_ref()
        .map(|d| {
            let refs = load_references(Path::new(d));
            novelty::measure_novelty(&generated, track, &refs)
                .map(|r| axes_json(&novelty::novelty_axes(&r)))
                .unwrap_or_default()
        })
        .unwrap_or_default();

    println!(
        "{{\"midi\":\"{}\",\"notes\":{},\"bars\":{},\"distinct_dur\":{},\"distinct_bar_rhythms\":{},\"sounding_bars\":{},\"npb_min\":{},\"npb_max\":{},\"npb_mean\":{:.2},\"npb_std\":{:.2},\"nonquarter\":{},\"pitch_lo\":{},\"pitch_hi\":{},\"pitch_span\":{},\"pitch_mean\":{:.1},\"distinct_pitch\":{},\"in_lo\":{},\"in_hi\":{},\"in_span\":{},\"closure\":{{{}}},\"novelty\":{{{}}}}}",
        Path::new(&midi).file_name().and_then(|s| s.to_str()).unwrap_or(""),
        notes, bars, distinct_dur.len(), distinct_bar_rhythms, sounding_bars,
        npb_min, npb_max, npb_mean, npb_std, nonquarter,
        lo, hi, hi.saturating_sub(lo), mean, distinct_p.len(),
        in_lo, in_hi, in_hi.saturating_sub(in_lo),
        closure_axes, novelty_axes
    );
}
