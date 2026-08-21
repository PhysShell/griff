//! SWG-4A-10: `griff swang dump` — the exact writer's CLI edge.
//!
//! The command is a transport, not a second formatter. Its whole job is:
//! import through the existing adapters, hand the canonical `Score` to
//! `griff_swang::exact::write_score`, and put the result on stdout. Two
//! surfaces, kept apart on purpose:
//!
//! - **stdout** carries the canonical level-2 document and nothing else;
//! - **stderr** carries diagnostics and human-facing import warnings.
//!
//! A warning that reached `Score.loss` is *not* dropped from the exact text
//! because a human already saw it on stderr. The loss report is a canonical
//! fact; the stderr line is a courtesy. Conflating them would undo what
//! SWG-4A-05 spent twenty mutations establishing.

// Reason: integration-test code. `unwrap`/`expect`/`panic` abort loudly with
// a clear message, which is exactly what a test harness wants.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::missing_assert_message
)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::{env, fs};

use griff_cli::swang_dump::dump_score;
use griff_core::import::import_score_auto;
use griff_core::score::{LossReport, Score};
use griff_swang::exact::write_score;

// ── fixtures ────────────────────────────────────────────────────────────────
//
// Both builders are deliberately *independent encoders*: `midly` and
// `guitarpro`'s own writer, never griff's export path. A fixture produced by
// the code under test would agree with it by construction.

/// One sounding note in a MIDI fixture, in absolute ticks.
struct Note {
    start: u32,
    dur: u32,
    key: u8,
    vel: u8,
}

/// A single-track SMF whose track name is written as raw bytes, so a caller
/// can hand it something that is not valid UTF-8.
fn midi_with_name(name: &'static [u8], notes: &[Note], ppqn: u16) -> Vec<u8> {
    use midly::{
        num::{u15, u24, u28, u4, u7},
        Format, Header, MetaMessage, MidiMessage, Smf, Timing, TrackEvent, TrackEventKind,
    };

    let mut abs: Vec<(u32, TrackEventKind<'static>)> = vec![
        (0, TrackEventKind::Meta(MetaMessage::TrackName(name))),
        (
            0,
            TrackEventKind::Meta(MetaMessage::TimeSignature(4, 2, 24, 8)),
        ),
        (
            0,
            TrackEventKind::Meta(MetaMessage::Tempo(u24::from_int_lossy(500_000))),
        ),
    ];
    let mut end = 0;
    for n in notes {
        abs.push((
            n.start,
            TrackEventKind::Midi {
                channel: u4::new(0),
                message: MidiMessage::NoteOn {
                    key: u7::new(n.key),
                    vel: u7::new(n.vel),
                },
            },
        ));
        abs.push((
            n.start.saturating_add(n.dur),
            TrackEventKind::Midi {
                channel: u4::new(0),
                message: MidiMessage::NoteOff {
                    key: u7::new(n.key),
                    vel: u7::new(0),
                },
            },
        ));
        end = end.max(n.start.saturating_add(n.dur));
    }
    abs.push((end, TrackEventKind::Meta(MetaMessage::EndOfTrack)));
    abs.sort_by_key(|&(tick, _)| tick);

    let mut track = Vec::new();
    let mut prev = 0;
    for (tick, kind) in abs {
        track.push(TrackEvent {
            delta: u28::from_int_lossy(tick.saturating_sub(prev)),
            kind,
        });
        prev = tick;
    }

    let mut smf = Smf::new(Header {
        format: Format::SingleTrack,
        timing: Timing::Metrical(u15::new(ppqn)),
    });
    smf.tracks = vec![track];
    let mut bytes = Vec::new();
    smf.write_std(&mut bytes).expect("fixture must serialise");
    bytes
}

/// A plain, lossless MIDI fixture: one named track, four quarter notes.
fn clean_midi() -> Vec<u8> {
    midi_with_name(
        b"Rhythm Gtr",
        &[
            Note {
                start: 0,
                dur: 480,
                key: 40,
                vel: 96,
            },
            Note {
                start: 480,
                dur: 480,
                key: 43,
                vel: 90,
            },
            Note {
                start: 960,
                dur: 480,
                key: 45,
                vel: 88,
            },
            Note {
                start: 1440,
                dur: 480,
                key: 40,
                vel: 84,
            },
        ],
        480,
    )
}

/// The same shape, but the track name is not valid UTF-8 — the importer
/// records `ImportWarning::TrackNameInvalidUtf8` and drops the name.
fn midi_with_a_lossy_name() -> Vec<u8> {
    // 0xFF is never a valid UTF-8 leading byte.
    midi_with_name(
        b"Gtr \xff\xfe",
        &[
            Note {
                start: 0,
                dur: 480,
                key: 40,
                vel: 96,
            },
            Note {
                start: 480,
                dur: 480,
                key: 43,
                vel: 90,
            },
        ],
        480,
    )
}

/// A Guitar Pro (GP7 `.gp`) fixture, written by the `guitarpro` crate's own
/// serializer — an encoder independent of griff's importer.
fn guitar_pro_bytes() -> Vec<u8> {
    use guitarpro::io::gpx::write_gp_bytes;
    use guitarpro::model::legacy::beat::{Beat, Voice as GpVoice};
    use guitarpro::model::legacy::enums::NoteType;
    use guitarpro::model::legacy::headers::MeasureHeader;
    use guitarpro::model::legacy::key_signature::{Duration as GpDuration, TimeSignature as GpTs};
    use guitarpro::model::legacy::measure::Measure;
    use guitarpro::model::legacy::note::Note as GpNote;
    use guitarpro::model::legacy::track::Track as GpTrack;

    let header = MeasureHeader {
        number: 1,
        start: 0,
        tempo: 120,
        time_signature: GpTs::default(),
        ..MeasureHeader::default()
    };
    let note = GpNote {
        value: 5,
        string: 1,
        velocity: 95,
        kind: NoteType::Normal,
        ..GpNote::default()
    };
    let beat = Beat {
        duration: GpDuration::default(),
        notes: vec![note],
        ..Beat::default()
    };
    let voice = GpVoice {
        measure_index: 0,
        beats: vec![beat],
        ..GpVoice::default()
    };
    let measure = Measure {
        number: 1,
        start: 0,
        track_index: 0,
        header_index: 0,
        voices: vec![voice],
        ..Measure::default()
    };
    let track = GpTrack {
        name: String::from("Probe Gtr"),
        measures: vec![measure],
        ..GpTrack::default()
    };
    let song = guitarpro::Song {
        name: String::from("dump fixture"),
        tempo: 120,
        measure_headers: vec![header],
        tracks: vec![track],
        ..guitarpro::Song::default()
    };

    write_gp_bytes(&song).expect("the guitarpro writer must produce bytes")
}

// ── harness ─────────────────────────────────────────────────────────────────

/// Writes `bytes` to a uniquely named temp file and returns its path.
fn input(name: &str, bytes: &[u8]) -> PathBuf {
    let path = env::temp_dir().join(format!("griff_swg_4a10_{name}"));
    fs::write(&path, bytes).expect("temp input must write");
    path
}

/// Runs the binary raw, for byte-exact stdout assertions.
fn dump(path: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_griff"))
        .args(["swang", "dump", path.to_str().unwrap()])
        .output()
        .expect("griff binary must run")
}

fn stdout_of(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).expect("the document is UTF-8")
}

fn stderr_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

// ── 1, 2: both importers reach the writer ───────────────────────────────────

#[test]
fn a_guitar_pro_file_dumps_a_canonical_level_two_document() {
    let path = input("gp_happy.gp", &guitar_pro_bytes());
    let out = dump(&path);
    fs::remove_file(&path).ok();
    assert!(
        out.status.success(),
        "a valid Guitar Pro file must dump: {}",
        stderr_of(&out)
    );
    let text = stdout_of(&out);
    assert!(
        text.starts_with("swang 2\n"),
        "the document opens with the frozen level-2 header: {text:?}"
    );
    assert!(
        text.contains("\"Probe Gtr\""),
        "the imported track name is in the document: {text}"
    );
}

#[test]
fn a_midi_file_dumps_a_canonical_level_two_document() {
    let path = input("midi_happy.mid", &clean_midi());
    let out = dump(&path);
    fs::remove_file(&path).ok();
    assert!(
        out.status.success(),
        "a valid MIDI file must dump: {}",
        stderr_of(&out)
    );
    let text = stdout_of(&out);
    assert!(
        text.starts_with("swang 2\n"),
        "the document opens with the frozen level-2 header: {text:?}"
    );
    assert!(
        text.contains("\"Rhythm Gtr\""),
        "the imported track name is in the document: {text}"
    );
}

// ── 3: stdout is the document and only the document ─────────────────────────

#[test]
fn stdout_carries_the_document_and_no_cli_chatter() {
    let path = input("no_chatter.mid", &clean_midi());
    let out = dump(&path);
    let text = stdout_of(&out);
    let leaked = path.to_str().unwrap().to_owned();
    fs::remove_file(&path).ok();

    for noise in ["Loaded", "warning:", "error:", "PPQN:", "Tracks:", "Bars:"] {
        assert!(
            !text.contains(noise),
            "stdout must not carry CLI chatter ({noise}): {text}"
        );
    }
    assert!(
        !text.contains(&leaked),
        "the input path is not part of the canonical document: {text}"
    );
    // The strongest form of the same claim: stdout is exactly the writer's
    // output for the score the importer built, and holds nothing else.
    let score = import_score_auto(&clean_midi()).expect("the fixture imports");
    assert_eq!(
        text,
        write_score(&score).expect("the fixture is inside the writer's domain"),
        "stdout is the exact document, with nothing added or removed"
    );
}

// ── 4: the two surfaces are independent ─────────────────────────────────────

#[test]
fn an_import_warning_is_reported_to_stderr_and_kept_in_the_document() {
    let bytes = midi_with_a_lossy_name();
    let score = import_score_auto(&bytes).expect("the fixture imports");
    assert!(
        !score.loss.warnings.is_empty(),
        "the fixture must actually be lossy, or this test proves nothing"
    );

    let path = input("lossy_name.mid", &bytes);
    let out = dump(&path);
    fs::remove_file(&path).ok();
    assert!(out.status.success(), "a lossy import still dumps");

    let text = stdout_of(&out);
    let errs = stderr_of(&out);
    assert!(
        text.contains("track_name_invalid_utf8"),
        "the loss stays a canonical fact in the exact text: {text}"
    );
    assert!(
        errs.contains("track"),
        "the human is told on stderr too: {errs:?}"
    );
    assert!(
        !errs.contains("swang 2"),
        "stderr never carries the document: {errs:?}"
    );
}

// ── 5: determinism ──────────────────────────────────────────────────────────

#[test]
fn two_runs_of_one_file_produce_byte_identical_stdout() {
    let path = input("determinism.mid", &clean_midi());
    let first = dump(&path);
    let second = dump(&path);
    fs::remove_file(&path).ok();
    assert!(first.status.success() && second.status.success());
    assert_eq!(
        first.stdout, second.stdout,
        "dump is a function of its input, byte for byte"
    );
}

// ── 6: import failure ───────────────────────────────────────────────────────

#[test]
fn an_unimportable_file_fails_with_a_diagnostic_and_no_document() {
    let path = input("garbage.mid", b"definitely not a music file");
    let out = dump(&path);
    fs::remove_file(&path).ok();
    assert!(
        !out.status.success(),
        "an import failure is a non-zero exit"
    );
    assert!(
        out.stdout.is_empty(),
        "nothing reaches stdout when there is no document: {:?}",
        stdout_of(&out)
    );
    assert!(
        !stderr_of(&out).is_empty(),
        "the failure is explained on stderr"
    );
}

// ── 7: a refusal yields no partial document ─────────────────────────────────

#[test]
fn a_score_outside_the_writer_domain_yields_no_document_at_all() {
    // The composition returns the whole document or nothing: there is no
    // half-written value for a caller to print. Proved at the seam the CLI
    // calls, because the importer sanitises its own output — no file reaches
    // `dump` that the writer would refuse (asserted below).
    let refused = Score {
        ticks_per_quarter: 0,
        master_bars: Vec::new(),
        tracks: Vec::new(),
        source_meta: None,
        loss: LossReport::new(),
    };
    let verdict = dump_score(&refused);
    assert!(
        verdict.is_err(),
        "ppqn 0 is outside the writer's domain, so there is no document"
    );
}

#[test]
fn a_dumped_score_carries_its_document_and_its_warnings_together() {
    // The other half of the same seam: on success both surfaces are built
    // before either is written, so neither can be emitted without the other
    // having been computed.
    let score = import_score_auto(&midi_with_a_lossy_name()).expect("the fixture imports");
    let dumped = dump_score(&score).expect("the fixture is inside the writer's domain");
    assert_eq!(
        dumped.document,
        write_score(&score).expect("same writer, same bytes"),
        "the document surface is the exact writer's output verbatim"
    );
    assert_eq!(
        dumped.warnings.len(),
        score.loss.warnings.len(),
        "one stderr line per canonical warning — rendered, never consumed"
    );
}

// ── 8: no hidden normalization ──────────────────────────────────────────────

#[test]
fn the_cli_prints_exactly_what_the_writer_produces() {
    // A fixture whose exact facts are easy to lose or reorder: a track name
    // the importer had to drop (so `loss` is non-empty and ordered) alongside
    // real notes. The comparison is against `write_score` of the imported
    // score — not a hand-copied golden, which would let the CLI and the
    // writer drift together into agreement about something wrong.
    let bytes = midi_with_a_lossy_name();
    let path = input("no_normalization.mid", &bytes);
    let out = dump(&path);
    fs::remove_file(&path).ok();
    assert!(out.status.success(), "{}", stderr_of(&out));

    let score = import_score_auto(&bytes).expect("the fixture imports");
    let expected = write_score(&score).expect("the fixture is inside the writer's domain");
    assert_eq!(
        stdout_of(&out),
        expected,
        "the CLI is a transport: no sorting, no tidying, no second formatter"
    );
}
