//! `griff-preview` — interactive terminal piano-roll for a `.mid` file (S8).
//!
//! Usage:
//!
//! ```text
//! griff-preview <file.mid>                    # interactive TUI
//! griff-preview <file.mid> --snapshot=WxH     # print one headless frame and exit
//! griff-preview <file.mid> --record=<chunk>   # persist a/x, t/T, r, s curation into the record
//! griff-preview <file.mid> --record=<chunk> --merge=<next>  # unlock m (merge with <next>)
//! ```
//!
//! It imports the file through the core MIDI importer, builds a
//! [`griff_preview::view::PianoRollView`] and its [`griff_preview::analysis`],
//! then either launches the `ratatui` front-end or renders a single frame to
//! stdout via a headless backend (useful in CI / over a pipe).

use std::path::Path;
use std::process::ExitCode;
use std::{env, fs};

use griff_core::midi::import_score;
use griff_preview::analysis::analyze;
use griff_preview::curation::{
    decide_record, merge_records, rename_record, set_tags, split_record_at_tick, summarize_record,
};
use griff_preview::tui::{self, App, CurationOutcome};
use griff_preview::view::build_view;

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
    let mut merge: Option<String> = None;
    for arg in args {
        if arg == "-h" || arg == "--help" {
            println!(
                "usage: griff-preview <file.mid> [--snapshot=WIDTHxHEIGHT] [--record=CHUNK_JSON] \
[--merge=PARTNER_JSON]"
            );
            return ExitCode::SUCCESS;
        } else if let Some(rec) = arg.strip_prefix("--record=") {
            record = Some(rec.to_owned());
        } else if let Some(partner) = arg.strip_prefix("--merge=") {
            merge = Some(partner.to_owned());
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

    attach_records(&mut app, record.as_deref(), merge.as_deref());

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
            Ok(outcome) => persist_outcome(record.as_deref(), merge.as_deref(), &outcome),
            Err(err) => {
                eprintln!("tui error: {err}");
                ExitCode::FAILURE
            }
        },
    }
}

/// Surfaces the record's current curation state in the inspector and, with
/// a partner attached, unlocks the merge intent. Best effort: an unreadable
/// record still fails loudly at quit-time persist, so here a warning
/// suffices (printed before the TUI owns the screen).
fn attach_records(app: &mut App, record: Option<&str>, merge: Option<&str>) {
    if let Some(record_path) = record {
        match fs::read_to_string(record_path).map(|json| summarize_record(&json)) {
            Ok(Ok(summary)) => app.set_record(summary),
            Ok(Err(err)) => {
                eprintln!("warning: cannot summarize record {record_path}: {err:?}");
            }
            Err(err) => eprintln!("warning: cannot read record {record_path}: {err}"),
        }
    }
    if let Some(partner_path) = merge {
        if record.is_none() {
            eprintln!("warning: --merge needs --record; ignoring {partner_path}");
        } else {
            match fs::read_to_string(partner_path).map(|json| summarize_record(&json)) {
                Ok(Ok(summary)) => app.set_merge_partner(summary.title),
                Ok(Err(err)) => {
                    eprintln!("warning: cannot summarize merge partner {partner_path}: {err:?}");
                }
                Err(err) => eprintln!("warning: cannot read merge partner {partner_path}: {err}"),
            }
        }
    }
}

/// Writes the pending curation outcome into the `--record` chunk file, if
/// both are present: decision, tags, and title rewrite the record in place;
/// a pending split or merge (mutually exclusive by the reducer) then
/// restructures the record file(s).
fn persist_outcome(
    record: Option<&str>,
    merge: Option<&str>,
    outcome: &CurationOutcome,
) -> ExitCode {
    let Some(path) = record else {
        return ExitCode::SUCCESS;
    };
    if outcome.decision.is_none()
        && outcome.tags.is_none()
        && outcome.title.is_none()
        && outcome.split_tick.is_none()
        && !outcome.merge
    {
        return ExitCode::SUCCESS;
    }
    let json = match fs::read_to_string(path) {
        Ok(json) => json,
        Err(err) => {
            eprintln!("cannot read record {path}: {err}");
            return ExitCode::FAILURE;
        }
    };
    let mut updated = json;
    if let Some(decision) = outcome.decision {
        updated = match decide_record(&updated, decision) {
            Ok(updated) => updated,
            Err(err) => {
                eprintln!("cannot update record {path}: {err:?}");
                return ExitCode::FAILURE;
            }
        };
    }
    if let Some(tags) = &outcome.tags {
        updated = match set_tags(&updated, tags) {
            Ok(updated) => updated,
            Err(err) => {
                eprintln!("cannot retag record {path}: {err:?}");
                return ExitCode::FAILURE;
            }
        };
    }
    if let Some(title) = &outcome.title {
        updated = match rename_record(&updated, title) {
            Ok(updated) => updated,
            Err(err) => {
                eprintln!("cannot rename record {path}: {err:?}");
                return ExitCode::FAILURE;
            }
        };
    }
    if let Some(tick) = outcome.split_tick {
        return persist_split(path, &updated, tick);
    }
    if outcome.merge {
        if let Some(partner_path) = merge {
            return persist_merge(path, &updated, partner_path);
        }
    }
    match fs::write(path, updated) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("cannot write record {path}: {err}");
            ExitCode::FAILURE
        }
    }
}

/// Splits the record at the marked tick: the first half replaces the record
/// file, the second half lands in the first vacant `.N` sibling — never
/// over an existing record, and with its `ChunkId` derived from that same
/// slot, so the id and the filename cannot disagree (Codex P2, PR #45).
fn persist_split(path: &str, json: &str, tick: u32) -> ExitCode {
    let Some((slot, second_path)) = vacant_sibling_path(path) else {
        eprintln!("cannot split record {path}: every sibling slot is taken");
        return ExitCode::FAILURE;
    };
    let (first, second) = match split_record_at_tick(json, tick, slot) {
        Ok(halves) => halves,
        Err(err) => {
            eprintln!("cannot split record {path}: {err:?}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(err) = fs::write(&second_path, second) {
        eprintln!("cannot write {second_path}: {err}");
        return ExitCode::FAILURE;
    }
    match fs::write(path, first) {
        Ok(()) => {
            eprintln!("split: the second half is in {second_path}");
            ExitCode::SUCCESS
        }
        Err(err) => {
            // The sibling next to the unsplit original would double-cover
            // the span — remove it before failing (the merge rollback
            // precedent; Codex P2, PR #45).
            eprintln!("cannot write record {path}: {err}");
            match fs::remove_file(&second_path) {
                Ok(()) => eprintln!("split rolled back: removed {second_path}"),
                Err(rm_err) => eprintln!(
                    "cannot remove {second_path}: {rm_err}; resolve the two files manually"
                ),
            }
            ExitCode::FAILURE
        }
    }
}

/// Merges the `--merge` partner into the record file and removes the
/// absorbed partner: its extent now lives in the merged record, and a
/// leftover file would double-cover the span.
fn persist_merge(path: &str, json: &str, partner_path: &str) -> ExitCode {
    let partner = match fs::read_to_string(partner_path) {
        Ok(partner) => partner,
        Err(err) => {
            eprintln!("cannot read merge partner {partner_path}: {err}");
            return ExitCode::FAILURE;
        }
    };
    let merged = match merge_records(json, &partner) {
        Ok(merged) => merged,
        Err(err) => {
            eprintln!("cannot merge {partner_path} into {path}: {err:?}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(err) = fs::write(path, merged) {
        eprintln!("cannot write record {path}: {err}");
        return ExitCode::FAILURE;
    }
    match fs::remove_file(partner_path) {
        Ok(()) => {
            eprintln!("merge: absorbed {partner_path} into {path}");
            ExitCode::SUCCESS
        }
        Err(err) => {
            // The merged record plus the leftover partner would double-cover
            // the span — exactly what the merge path must prevent. Roll the
            // record back and fail (Codex P2, PR #45).
            eprintln!("cannot remove absorbed partner {partner_path}: {err}");
            match fs::write(path, json) {
                Ok(()) => eprintln!("merge rolled back: {path} restored"),
                Err(restore_err) => {
                    eprintln!(
                        "cannot restore record {path}: {restore_err}; resolve the two files manually"
                    );
                }
            }
            ExitCode::FAILURE
        }
    }
}

/// The first vacant `.N` sibling next to the record (`chunk.json` →
/// `chunk.2.json`, then `chunk.3.json`, …) with its slot number; `None`
/// when every slot up to `.99` is taken. A split must never overwrite a
/// neighboring record, and the slot feeds the second half's `ChunkId`.
fn vacant_sibling_path(path: &str) -> Option<(u32, String)> {
    let make = |n: u32| {
        let sibling = path
            .strip_suffix(".json")
            .map_or_else(|| format!("{path}.{n}"), |stem| format!("{stem}.{n}.json"));
        (n, sibling)
    };
    (2..=99).map(make).find(|(_, p)| !Path::new(p).exists())
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
