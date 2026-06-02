//! `griff-preview` — render a `.mid` file as a piano-roll to stdout (S8, slice 1).
//!
//! Usage:
//!
//! ```text
//! griff-preview <file.mid> [--size=WIDTHxHEIGHT]
//! ```
//!
//! This first slice is a one-shot, dependency-free renderer: it imports the file
//! through the core MIDI importer, builds a [`PianoRollView`], and prints one
//! rasterised frame. An interactive `ratatui` front-end (scroll/zoom) and MIDI
//! playback build on the same view/render layers in later increments.

use std::process::ExitCode;
use std::{env, fs};

use griff_core::midi::import_score;
use griff_preview::render::{lane_glyph, pitch_name, render_frame};
use griff_preview::view::{build_view, PianoRollView};

/// Default frame size when `--size` is not given.
const DEFAULT_WIDTH: usize = 100;
const DEFAULT_HEIGHT: usize = 32;

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let Some(path) = args.next() else {
        eprintln!("usage: griff-preview <file.mid> [--size=WIDTHxHEIGHT]");
        return ExitCode::FAILURE;
    };

    let mut width = DEFAULT_WIDTH;
    let mut height = DEFAULT_HEIGHT;
    for arg in args {
        if arg == "-h" || arg == "--help" {
            println!("usage: griff-preview <file.mid> [--size=WIDTHxHEIGHT]");
            return ExitCode::SUCCESS;
        } else if let Some(spec) = arg.strip_prefix("--size=") {
            let Some((w, h)) = parse_size(spec) else {
                eprintln!("invalid --size '{spec}', expected e.g. --size=120x40");
                return ExitCode::FAILURE;
            };
            width = w;
            height = h;
        } else {
            eprintln!("unknown argument '{arg}'");
            return ExitCode::FAILURE;
        }
    }

    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(err) => {
            eprintln!("cannot read {path}: {err}");
            return ExitCode::FAILURE;
        }
    };
    let score = match import_score(&bytes) {
        Ok(score) => score,
        Err(err) => {
            eprintln!("cannot import {path}: {err}");
            return ExitCode::FAILURE;
        }
    };

    let view = build_view(&score);
    print_header(&path, &view);
    for line in render_frame(&view, width, height) {
        println!("{line}");
    }
    print_legend(&view);
    ExitCode::SUCCESS
}

/// Parses a `WIDTHxHEIGHT` spec, clamped to sane bounds. Returns `None` on
/// malformed input or a zero dimension.
fn parse_size(spec: &str) -> Option<(usize, usize)> {
    let (w, h) = spec.split_once(['x', 'X'])?;
    let w = w.trim().parse::<usize>().ok()?;
    let h = h.trim().parse::<usize>().ok()?;
    if w == 0 || h == 0 {
        return None;
    }
    Some((w.min(400), h.min(200)))
}

fn print_header(path: &str, view: &PianoRollView) {
    let span = view.tick_end.saturating_sub(view.tick_start);
    println!(
        "griff-preview │ {path} │ {bars} bar(s) │ {bpm:.0} BPM │ {ppq} ppq │ pitch {lo}–{hi} │ {span} ticks",
        bars = view.bar_count,
        bpm = view.tempo_bpm,
        ppq = view.ppq,
        lo = pitch_name(view.low_pitch),
        hi = pitch_name(view.high_pitch),
    );
}

fn print_legend(view: &PianoRollView) {
    if view.lanes.is_empty() {
        println!("(no note-bearing tracks)");
        return;
    }
    let parts: Vec<String> = view
        .lanes
        .iter()
        .enumerate()
        .map(|(i, lane)| format!("{} {}", lane_glyph(i), lane.name))
        .collect();
    println!("lanes: {}", parts.join("   "));
}
