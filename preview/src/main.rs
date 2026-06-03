//! `griff-preview` — interactive terminal piano-roll for a `.mid` file (S8).
//!
//! Usage:
//!
//! ```text
//! griff-preview <file.mid>                 # interactive TUI
//! griff-preview <file.mid> --snapshot=WxH  # print one headless frame and exit
//! ```
//!
//! It imports the file through the core MIDI importer, builds a
//! [`griff_preview::view::PianoRollView`] and its [`griff_preview::analysis`],
//! then either launches the `ratatui` front-end or renders a single frame to
//! stdout via a headless backend (useful in CI / over a pipe).

use std::process::ExitCode;
use std::{env, fs};

use griff_core::midi::import_score;
use griff_preview::analysis::analyze;
use griff_preview::tui::{self, App};
use griff_preview::view::build_view;

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let Some(path) = args.next() else {
        eprintln!("usage: griff-preview <file.mid> [--snapshot=WIDTHxHEIGHT]");
        return ExitCode::FAILURE;
    };

    let mut snapshot: Option<(u16, u16)> = None;
    for arg in args {
        if arg == "-h" || arg == "--help" {
            println!("usage: griff-preview <file.mid> [--snapshot=WIDTHxHEIGHT]");
            return ExitCode::SUCCESS;
        } else if let Some(spec) = arg.strip_prefix("--snapshot=") {
            let Some(size) = parse_size(spec) else {
                eprintln!("invalid --snapshot '{spec}', expected e.g. --snapshot=120x40");
                return ExitCode::FAILURE;
            };
            snapshot = Some(size);
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

    let mut app = App::new(build_view(&score), analyze(&score), path);

    match snapshot {
        Some((w, h)) => match app.snapshot(w, h) {
            Ok(lines) => {
                for line in lines {
                    println!("{line}");
                }
                ExitCode::SUCCESS
            }
            Err(err) => {
                eprintln!("snapshot failed: {err}");
                ExitCode::FAILURE
            }
        },
        None => match tui::run(app) {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                eprintln!("tui error: {err}");
                ExitCode::FAILURE
            }
        },
    }
}

/// Parses a `WIDTHxHEIGHT` spec into clamped terminal dimensions.
fn parse_size(spec: &str) -> Option<(u16, u16)> {
    let (w, h) = spec.split_once(['x', 'X'])?;
    let w = w.trim().parse::<u16>().ok()?;
    let h = h.trim().parse::<u16>().ok()?;
    if w == 0 || h == 0 {
        return None;
    }
    Some((w.min(400), h.min(200)))
}
