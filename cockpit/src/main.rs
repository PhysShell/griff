//! `griff-cockpit` — native egui window rendering a score's piano-roll `Scene`
//! (ADR-0027, Slice 1) and, with a corpus, ranking generation candidates over it
//! (S8).
//!
//! Reads a MIDI or Guitar Pro file, imports it through the shared core
//! importer, builds the renderer-agnostic view + analysis, and hands them to
//! the egui `CockpitApp`. With `--corpus DIR` it also loads the curated corpus:
//! its chunks supply the Generate panel's rhythm templates, novelty references
//! and gesture ask, and its source tabs become the seed pick-list — so a whole
//! session runs without touching the CLI. The browser (wasm) entry point is
//! `griff_cockpit::web::start` (see `lib.rs`, Slice 2).

#[cfg(not(target_arch = "wasm32"))]
use std::process::ExitCode;

#[cfg(not(target_arch = "wasm32"))]
const USAGE: &str = "usage: griff-cockpit [file.mid|.gp3|.gp4|.gp5|.gpx] \
                     [--corpus DIR] [--out DIR]\n  \
                     a file, a corpus, or both — with only a corpus the cockpit \
                     opens on its first tab";

#[cfg(not(target_arch = "wasm32"))]
fn main() -> ExitCode {
    use std::{env, fs, path::PathBuf};

    use griff_cockpit::generation::load_corpus_dir;
    use griff_cockpit::CockpitApp;
    use griff_core::import::import_score_auto;

    // A three-flag hand-rolled parse: the cockpit is a window, not a CLI, and a
    // clap dependency here would be the tail wagging the dog.
    let (mut input, mut corpus, mut out) = (None::<String>, None::<PathBuf>, None::<PathBuf>);
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--corpus" => match args.next() {
                Some(dir) => corpus = Some(PathBuf::from(dir)),
                None => {
                    eprintln!("--corpus needs a directory\n{USAGE}");
                    return ExitCode::FAILURE;
                }
            },
            "--out" => match args.next() {
                Some(dir) => out = Some(PathBuf::from(dir)),
                None => {
                    eprintln!("--out needs a directory\n{USAGE}");
                    return ExitCode::FAILURE;
                }
            },
            "-h" | "--help" => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            other => input = Some(other.to_owned()),
        }
    }

    let loaded = match corpus.as_deref().map(load_corpus_dir).transpose() {
        Ok(loaded) => loaded,
        Err(err) => {
            eprintln!("{err}");
            return ExitCode::FAILURE;
        }
    };

    // The score to open on: the named file, else the corpus's first source tab.
    let opening = match (input.as_deref(), loaded.as_ref()) {
        (Some(path), _) => match fs::read(path) {
            Ok(bytes) => Some((path.to_owned(), bytes)),
            Err(err) => {
                eprintln!("cannot read {path}: {err}");
                return ExitCode::FAILURE;
            }
        },
        (None, Some(l)) => l
            .sources
            .first()
            .map(|tab| (tab.name.clone(), tab.bytes.clone())),
        (None, None) => None,
    };
    let Some((title, bytes)) = opening else {
        eprintln!("nothing to open: pass a file, a --corpus with source tabs, or both\n{USAGE}");
        return ExitCode::FAILURE;
    };
    let score = match import_score_auto(&bytes) {
        Ok(score) => score,
        Err(err) => {
            eprintln!("cannot import {title}: {err}");
            return ExitCode::FAILURE;
        }
    };

    let mut app = CockpitApp::from_score(score, title);
    if let Some(dir) = out {
        app.set_out_dir(dir);
    }
    if let Some(l) = loaded {
        let (chunks, templates) = (l.material.references.len(), l.material.rhythms.len());
        let skipped = l.material.skipped.len();
        println!("corpus: {chunks} chunks ({templates} rhythm templates), {skipped} skipped");
        app.attach_corpus(l.material, l.sources);
    }

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
