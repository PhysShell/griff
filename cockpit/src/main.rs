//! `griff-cockpit` — native egui window rendering a score's piano-roll `Scene`
//! (ADR-0027, Slice 1).
//!
//! Reads a MIDI or Guitar Pro file, imports it through the shared core
//! importer, builds the renderer-agnostic view + analysis, and hands them to
//! the egui `CockpitApp`. The browser (wasm) entry point is
//! `griff_cockpit::web::start` (see `lib.rs`, Slice 2).

#[cfg(not(target_arch = "wasm32"))]
use std::process::ExitCode;

#[cfg(not(target_arch = "wasm32"))]
fn main() -> ExitCode {
    use std::{env, fs};

    use griff_cockpit::CockpitApp;
    use griff_core::import::import_score_auto;

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

    let app = CockpitApp::from_score(score, path);
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

// On wasm there is no native window or filesystem: the browser entry point is
// `griff_cockpit::web::start` (lib.rs). This stub keeps the bin target compiling for
// wasm32 so a plain `cargo build --target wasm32-unknown-unknown` over the whole
// package stays clean (the web build itself uses `--lib`; see `build-web.sh`).
#[cfg(target_arch = "wasm32")]
fn main() {}
