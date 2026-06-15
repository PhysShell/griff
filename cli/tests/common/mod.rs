//! Shared support for the S0 CLI characterization suite.
//!
//! Builds minimal, fully synthetic `.mid` fixtures (no licensing concerns)
//! directly with `midly` — deliberately *not* through griff's own export path,
//! so the import golden tests exercise a real, independent encoder.
//!
//! Snapshots are plain golden text files compared in-process. Regenerate them
//! intentionally with `GRIFF_BLESS=1 cargo test -p griff-cli`.

// Reason: integration-test support code. `unwrap`/`expect`/`panic` make a
// failed fixture build or golden mismatch abort loudly with a clear message,
// which is exactly what a test harness wants.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::missing_assert_message
)]

use std::{env, fmt::Write as _, fs, path::PathBuf, process::Command};

use midly::{
    num::{u15, u24, u28, u4, u7},
    Format, Header, MetaMessage, MidiMessage, Smf, Timing, TrackEvent, TrackEventKind,
};

const INFALLIBLE: &str = "writing to a String is infallible";

/// One sounding note in a fixture, in absolute ticks.
#[derive(Debug, Clone, Copy)]
struct NoteSpec {
    start: u32,
    dur: u32,
    key: u8,
    vel: u8,
}

/// Build evenly-spaced monophonic notes, cycling through `keys`.
///
/// Monophonic + non-coincident starts keep `sort_unstable` in the importer
/// deterministic, so the golden snapshots are stable.
fn run(count: u32, step: u32, dur: u32, keys: &[u8], vel: u8) -> Vec<NoteSpec> {
    keys.iter()
        .copied()
        .cycle()
        .zip(0..count)
        .map(|(key, i)| NoteSpec {
            start: i.saturating_mul(step),
            dur,
            key,
            vel,
        })
        .collect()
}

fn meta_track(
    tempos: &[(u32, u32)],
    ts_num: u8,
    ts_den_pow: u8,
    end: u32,
) -> Vec<TrackEvent<'static>> {
    let mut abs: Vec<(u32, TrackEventKind<'static>)> = vec![(
        0,
        TrackEventKind::Meta(MetaMessage::TimeSignature(ts_num, ts_den_pow, 24, 8)),
    )];
    for &(tick, micros) in tempos {
        abs.push((
            tick,
            TrackEventKind::Meta(MetaMessage::Tempo(u24::from_int_lossy(micros))),
        ));
    }
    abs.push((end, TrackEventKind::Meta(MetaMessage::EndOfTrack)));
    to_delta(abs)
}

fn note_track(
    name: &'static [u8],
    channel: u8,
    notes: &[NoteSpec],
    end: u32,
) -> Vec<TrackEvent<'static>> {
    let ch = u4::new(channel);
    let mut abs: Vec<(u32, TrackEventKind<'static>)> =
        vec![(0, TrackEventKind::Meta(MetaMessage::TrackName(name)))];
    for n in notes {
        abs.push((
            n.start,
            TrackEventKind::Midi {
                channel: ch,
                message: MidiMessage::NoteOn {
                    key: u7::new(n.key),
                    vel: u7::new(n.vel),
                },
            },
        ));
        abs.push((
            n.start.saturating_add(n.dur),
            TrackEventKind::Midi {
                channel: ch,
                message: MidiMessage::NoteOff {
                    key: u7::new(n.key),
                    vel: u7::new(0),
                },
            },
        ));
    }
    abs.push((end, TrackEventKind::Meta(MetaMessage::EndOfTrack)));
    to_delta(abs)
}

/// Stable absolute→delta conversion; ties order Off, then On, then Meta.
fn to_delta(mut abs: Vec<(u32, TrackEventKind<'static>)>) -> Vec<TrackEvent<'static>> {
    const fn rank(kind: &TrackEventKind<'_>) -> u8 {
        match kind {
            TrackEventKind::Midi {
                message: MidiMessage::NoteOff { .. },
                ..
            } => 0,
            TrackEventKind::Midi { .. } => 1,
            _ => 2,
        }
    }
    abs.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| rank(&a.1).cmp(&rank(&b.1))));
    let mut prev = 0_u32;
    abs.into_iter()
        .map(|(tick, kind)| {
            let delta = tick.saturating_sub(prev);
            prev = tick;
            TrackEvent {
                delta: u28::from_int_lossy(delta),
                kind,
            }
        })
        .collect()
}

fn encode(format: Format, ppqn: u16, tracks: Vec<Vec<TrackEvent<'static>>>) -> Vec<u8> {
    let mut smf = Smf::new(Header {
        format,
        timing: Timing::Metrical(u15::new(ppqn)),
    });
    smf.tracks = tracks;
    let mut out = Vec::new();
    smf.write_std(&mut out).expect("fixture must serialise");
    out
}

/// The canonical fixture set characterized by S0, in a fixed order.
pub(crate) fn fixtures() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("simple_4_4", simple_4_4()),
        ("seven_eight", seven_eight()),
        ("multi_track", multi_track()),
        ("tempo_change", tempo_change()),
        ("two_phrases", two_phrases()),
    ]
}

/// 4/4 @ 120 BPM: a heavy 4-note bar then a light 8-note bar.
fn simple_4_4() -> Vec<u8> {
    let mut notes = run(4, 480, 480, &[40, 52, 64, 40], 100);
    let bar1 = run(8, 240, 240, &[60, 62, 63, 65, 67, 65, 63, 62], 55)
        .into_iter()
        .map(|mut n| {
            n.start = n.start.saturating_add(1920);
            n
        });
    notes.extend(bar1);
    encode(
        Format::Parallel,
        480,
        vec![
            meta_track(&[(0, 500_000)], 4, 2, 3840),
            note_track(b"Lead", 0, &notes, 3840),
        ],
    )
}

/// 7/8 @ 140 BPM: one odd-meter rhythm bar.
fn seven_eight() -> Vec<u8> {
    let notes = run(7, 240, 240, &[38, 38, 40, 38, 43, 41, 38], 96);
    encode(
        Format::Parallel,
        480,
        vec![
            meta_track(&[(0, 428_571)], 7, 3, 1680),
            note_track(b"Rhythm", 1, &notes, 1680),
        ],
    )
}

/// 4/4 @ 90 BPM, two channels: a clean track and a wide-range solo track.
fn multi_track() -> Vec<u8> {
    let clean = run(8, 240, 240, &[55, 57, 58, 60, 62, 60, 58, 57], 50);
    let solo = run(6, 320, 320, &[40, 52, 64, 76, 64, 52], 90);
    encode(
        Format::Parallel,
        480,
        vec![
            meta_track(&[(0, 666_667)], 4, 2, 1920),
            note_track(b"Clean", 2, &clean, 1920),
            note_track(b"Solo", 3, &solo, 1920),
        ],
    )
}

/// 4/4 with a tempo change at the bar-1 boundary (100 → 150 BPM).
fn tempo_change() -> Vec<u8> {
    let notes = run(12, 480, 480, &[45, 47, 48, 50], 88);
    encode(
        Format::Parallel,
        480,
        vec![
            meta_track(&[(0, 600_000), (1920, 400_000)], 4, 2, 5760),
            note_track(b"Lead", 0, &notes, 5760),
        ],
    )
}

/// 4/4 @ 120 BPM, five bars: two low-register quarter-note bars, a silent bar,
/// then two high-register eighth-note bars. The silence plus the register and
/// rhythm change make a clear phrase break, so the S4 detector emits at least
/// one boundary — pinning the per-boundary output format.
fn two_phrases() -> Vec<u8> {
    let mut notes = run(8, 480, 480, &[40, 43, 45, 47], 90);
    let phrase_two = run(16, 240, 240, &[72, 74, 76, 77], 80)
        .into_iter()
        .map(|mut n| {
            n.start = n.start.saturating_add(5760);
            n
        });
    notes.extend(phrase_two);
    encode(
        Format::Parallel,
        480,
        vec![
            meta_track(&[(0, 500_000)], 4, 2, 9600),
            note_track(b"Lead", 0, &notes, 9600),
        ],
    )
}

// ── paths ──────────────────────────────────────────────────────────────────

fn tests_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests")
}

pub(crate) fn fixture_path(name: &str) -> PathBuf {
    tests_dir().join("fixtures").join(format!("{name}.mid"))
}

fn snapshot_path(name: &str) -> PathBuf {
    tests_dir().join("snapshots").join(format!("{name}.txt"))
}

// ── running the binary ─────────────────────────────────────────────────────

/// Run the `griff` binary, returning normalized stdout + stderr.
///
/// Any occurrence of `scrub` in the output is replaced with `<OUT>` so the
/// temp export path does not leak into the golden file.
pub(crate) fn griff(args: &[&str], scrub: Option<&str>) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_griff"))
        .args(args)
        .output()
        .expect("griff binary must run");
    let mut text = String::new();
    writeln!(text, "$ griff {}", args.join(" ")).expect(INFALLIBLE);
    writeln!(text, "exit: {}", output.status.code().unwrap_or(-1)).expect(INFALLIBLE);
    text.push_str("--- stdout ---\n");
    text.push_str(&String::from_utf8_lossy(&output.stdout));
    text.push_str("--- stderr ---\n");
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    match scrub {
        Some(s) => text.replace(s, "<OUT>"),
        None => text,
    }
}

/// Compare `actual` against the stored golden snapshot, or write it when
/// `GRIFF_BLESS=1`.
pub(crate) fn assert_golden(name: &str, actual: &str) {
    let path = snapshot_path(name);
    if env::var("GRIFF_BLESS").as_deref() == Ok("1") {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, actual).unwrap();
        return;
    }
    let expected = fs::read_to_string(&path).unwrap_or_else(|_| {
        panic!(
            "missing golden snapshot {}; create it with \
             `GRIFF_BLESS=1 cargo test -p griff-cli`",
            path.display()
        )
    });
    assert_eq!(
        actual, expected,
        "CLI output drifted from golden snapshot `{name}`. If this change is \
         intended, re-bless with `GRIFF_BLESS=1 cargo test -p griff-cli`."
    );
}
