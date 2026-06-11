//! `griff-preview` — interactive terminal piano-roll for a `.mid` file (S8).
//!
//! Usage:
//!
//! ```text
//! griff-preview <file.mid>                    # interactive TUI
//! griff-preview <file.mid> --snapshot=WxH     # print one headless frame and exit
//! griff-preview <file.mid> --record=<chunk>   # persist a/x curation into the record
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
use griff_preview::curation::{decide_record, summarize_record};
use griff_preview::tui::{self, App};
use griff_preview::view::build_view;
use griff_preview::viewport::CurationDecision;

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let Some(path) = args.next() else {
        eprintln!(
            "usage: griff-preview <file.mid> [--snapshot=WIDTHxHEIGHT] [--record=CHUNK_JSON]"
        );
        return ExitCode::FAILURE;
    };

    let mut snapshot: Option<(u16, u16)> = None;
    let mut record: Option<String> = None;
    for arg in args {
        if arg == "-h" || arg == "--help" {
            println!(
                "usage: griff-preview <file.mid> [--snapshot=WIDTHxHEIGHT] [--record=CHUNK_JSON]"
            );
            return ExitCode::SUCCESS;
        } else if let Some(rec) = arg.strip_prefix("--record=") {
            record = Some(rec.to_owned());
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

    // Surface the record's current curation state in the inspector. Best
    // effort: an unreadable record still fails loudly at quit-time persist,
    // so here a warning suffices (printed before the TUI owns the screen).
    if let Some(record_path) = record.as_deref() {
        match fs::read_to_string(record_path).map(|json| summarize_record(&json)) {
            Ok(Ok(summary)) => app.set_record(summary),
            Ok(Err(err)) => {
                eprintln!("warning: cannot summarize record {record_path}: {err:?}");
            }
            Err(err) => eprintln!("warning: cannot read record {record_path}: {err}"),
        }
    }

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
            Ok(decision) => persist_decision(record.as_deref(), decision),
            Err(err) => {
                eprintln!("tui error: {err}");
                ExitCode::FAILURE
            }
        },
    }
}

/// Writes the pending curation decision into the `--record` chunk file, if
/// both are present; everything except the `reviewer` field is untouched.
fn persist_decision(record: Option<&str>, decision: Option<CurationDecision>) -> ExitCode {
    let (Some(path), Some(decision)) = (record, decision) else {
        return ExitCode::SUCCESS;
    };
    let json = match fs::read_to_string(path) {
        Ok(json) => json,
        Err(err) => {
            eprintln!("cannot read record {path}: {err}");
            return ExitCode::FAILURE;
        }
    };
    let updated = match decide_record(&json, decision) {
        Ok(updated) => updated,
        Err(err) => {
            eprintln!("cannot update record {path}: {err:?}");
            return ExitCode::FAILURE;
        }
    };
    match fs::write(path, updated) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("cannot write record {path}: {err}");
            ExitCode::FAILURE
        }
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
