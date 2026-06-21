//! `griff-cockpit` — native egui window rendering a score's piano-roll `Scene`
//! (ADR-0027, Slice 1).
//!
//! Reads a MIDI or Guitar Pro file, imports it through the shared core
//! importer, builds the renderer-agnostic view + analysis, and hands them to
//! the egui [`CockpitApp`]. The web (wasm) entry point follows in Slice 2.

use std::process::ExitCode;
use std::{env, fs};

use griff_cockpit::CockpitApp;
use griff_core::import::import_score_auto;
use griff_ui_core::{analyze, build_view};

fn main() -> ExitCode {
    let Some(path) = env::args().nth(1) else {
        eprintln!("usage: griff-cockpit <file.mid|.gp3|.gp4|.gp5|.gpx>");
        return ExitCode::FAILURE;
    };
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(err) => {
            eprintln!("cannot read {path}: {err}");
            return ExitCode::FAILURE;
        }
    };
    let score = match import_score_auto(&bytes) {
        Ok(score) => score,
        Err(err) => {
            eprintln!("cannot import {path}: {err}");
            return ExitCode::FAILURE;
        }
    };

    let app = CockpitApp::new(build_view(&score), analyze(&score), path);
    let options = eframe::NativeOptions::default();
    match eframe::run_native(
        "griff · cockpit",
        options,
        Box::new(|_cc| Ok(Box::new(app))),
    ) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("cockpit error: {err}");
            ExitCode::FAILURE
        }
    }
}
