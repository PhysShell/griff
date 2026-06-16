//! Browser playground (WASM) for the griff complement arranger — ADR-0024.
//!
//! A deliberately thin, throwaway front (ADR-0024 §5): no `wasm-bindgen`, no
//! framework. It exports three C-ABI functions and marshals a JSON result
//! through linear memory, so the build is just `cargo build --target
//! wasm32-unknown-unknown` and the page is static files. The canonical `egui`
//! frontend (ADR-0016) replaces this at M2.
//!
//! `arrange(mode, seed, offset, variation)` builds a fixed sample part A, runs
//! [`arrange_complement_varied`], and writes `{ppqn, tempo, realized_spread,
//! error, tracks:[A, B]}` into a thread-local buffer; JS reads it via
//! `arrange()` (pointer) + `arrange_len()` (length).

use std::cell::RefCell;
use std::fmt::Write as _;

use griff_core::complement::{
    arrange_complement_varied, ComplementSpec, RelationMode, VariationControl,
};
use griff_core::event::{NoteMarks, Pitch, Tempo, Ticks, TimeSignature, Tuning, Velocity};
use griff_core::generate::GenerationSeed;
use griff_core::score::{
    AtomEvent, EventGroup, EventGroupKind, LossReport, MasterBar, RepeatMarker, Score, Track, Voice,
};
use griff_core::slice::TickRange;

const PPQN: u16 = 480;
const BAR: u32 = 1920; // 4/4 at 480 PPQN
const EIGHTH: u32 = 240;
const TEMPO: f64 = 120.0;
const BARS: usize = 4;

/// A fixed, uniform-4/4 part A spanning ~two octaves of C natural minor, so the
/// ladder modes' `pitch_spread` knob is audible and `counter_melody` has room.
fn sample_part_a() -> Score {
    // C natural minor across two octaves.
    const SCALE: [u8; 15] = [48, 50, 51, 53, 55, 56, 58, 60, 62, 63, 65, 67, 68, 70, 72];
    let span = 2 * (SCALE.len() - 1); // triangle-wave period over the scale

    let master_bars = (0..BARS)
        .map(|i| {
            let start = u32::try_from(i).unwrap_or(0) * BAR;
            MasterBar {
                index: i,
                tick_range: TickRange::new(Ticks(start), Ticks(start + BAR))
                    .expect("ordered bar range"),
                time_signature: TimeSignature {
                    numerator: 4,
                    denominator: 4,
                },
                tempo: Tempo::new(TEMPO).expect("valid tempo"),
                repeat: RepeatMarker::default(),
            }
        })
        .collect();

    let mut groups = Vec::new();
    let per_bar = (BAR / EIGHTH) as usize; // 8 eighth notes per bar
    for bar in 0..BARS {
        let bar_start = u32::try_from(bar).unwrap_or(0) * BAR;
        for j in 0..per_bar {
            let i = bar * per_bar + j;
            // Triangle contour over the scale: up then down, repeating.
            let phase = i % span;
            let idx = if phase < SCALE.len() {
                phase
            } else {
                span - phase
            };
            let pitch = SCALE[idx];
            let onset = bar_start + u32::try_from(j).unwrap_or(0) * EIGHTH;
            groups.push(EventGroup {
                kind: EventGroupKind::Single,
                atoms: vec![AtomEvent::Note(griff_core::score::AtomNote {
                    absolute_start: Ticks(onset),
                    duration: Ticks(EIGHTH),
                    pitch: Pitch::new(pitch).unwrap_or(Pitch(48)),
                    velocity: Velocity::new(90).unwrap_or(Velocity(90)),
                    marks: NoteMarks::empty(),
                    position: None,
                })],
                technique_spans: Vec::new(),
            });
        }
    }

    Score {
        ticks_per_quarter: PPQN,
        master_bars,
        tracks: vec![Track {
            name: Some("A".to_string()),
            channel: 0,
            voices: vec![Voice {
                id: 0,
                event_groups: groups,
            }],
            tuning: Tuning::standard_e(),
        }],
        source_meta: None,
        loss: LossReport::new(),
    }
}

fn relation_mode(mode: u32) -> RelationMode {
    match mode {
        1 => RelationMode::RegisterContrast,
        2 => RelationMode::CallResponse,
        3 => RelationMode::SupportLayer,
        4 => RelationMode::OctaveDouble,
        5 => RelationMode::CounterMelody,
        _ => RelationMode::RhythmLock,
    }
}

/// Appends a track's primary-voice notes as a JSON array of `{p,s,d,v}`.
fn push_notes(json: &mut String, track: &Track) {
    json.push('[');
    let mut first = true;
    if let Some(voice) = track.voices.first() {
        for group in &voice.event_groups {
            for atom in &group.atoms {
                if let AtomEvent::Note(n) = atom {
                    if !first {
                        json.push(',');
                    }
                    first = false;
                    let _ = write!(
                        json,
                        "{{\"p\":{},\"s\":{},\"d\":{},\"v\":{}}}",
                        n.pitch.0, n.absolute_start.0, n.duration.0, n.velocity.0
                    );
                }
            }
        }
    }
    json.push(']');
}

fn json_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Builds the result JSON for one arrangement request.
fn build_json(mode: u32, seed: u64, offset: i32, variation: f32) -> String {
    let score = sample_part_a();
    let spec = ComplementSpec {
        mode: relation_mode(mode),
        register_offset: offset.clamp(-48, 48) as i8,
    };
    let control = VariationControl {
        pitch_spread: f64::from(variation).clamp(0.0, 1.0),
    };

    let mut json = String::with_capacity(2048);
    let _ = write!(json, "{{\"ppqn\":{PPQN},\"tempo\":{TEMPO},");

    match arrange_complement_varied(&score, 0, spec, GenerationSeed(seed), control) {
        Ok(varied) => {
            let combined = &varied.complement.score;
            let b_index = varied.complement.part_b_index;
            let b_name = combined
                .tracks
                .get(b_index)
                .and_then(|t| t.name.clone())
                .unwrap_or_else(|| "B".to_string());
            let _ = write!(
                json,
                "\"realized_spread\":{:.3},\"error\":null,\"tracks\":[",
                varied.realized_spread
            );
            json.push_str("{\"name\":\"A\",\"role\":\"a\",\"notes\":");
            push_notes(&mut json, &score.tracks[0]);
            json.push('}');
            if let Some(b_track) = combined.tracks.get(b_index) {
                let _ = write!(
                    json,
                    ",{{\"name\":\"{}\",\"role\":\"b\",\"notes\":",
                    json_escape(&b_name)
                );
                push_notes(&mut json, b_track);
                json.push('}');
            }
            json.push(']');
        }
        Err(e) => {
            // Surface the typed error; still return A so the page can draw it.
            let _ = write!(
                json,
                "\"realized_spread\":0,\"error\":\"{:?}\",\"tracks\":[",
                e
            );
            json.push_str("{\"name\":\"A\",\"role\":\"a\",\"notes\":");
            push_notes(&mut json, &score.tracks[0]);
            json.push_str("}]");
        }
    }
    json.push('}');
    json
}

thread_local! {
    static OUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

/// Arranges a complement and stores the JSON result; returns a pointer into
/// WASM linear memory. Read `arrange_len()` bytes from it (valid until the next
/// `arrange` call).
#[no_mangle]
pub extern "C" fn arrange(mode: u32, seed: u32, offset: i32, variation: f32) -> *const u8 {
    let json = build_json(mode, u64::from(seed), offset, variation);
    OUT.with(|o| {
        *o.borrow_mut() = json.into_bytes();
        o.borrow().as_ptr()
    })
}

/// Length in bytes of the JSON stored by the last [`arrange`] call.
#[no_mangle]
pub extern "C" fn arrange_len() -> usize {
    OUT.with(|o| o.borrow().len())
}

#[cfg(test)]
mod tests {
    use super::build_json;

    #[test]
    fn every_mode_emits_part_a_and_well_formed_header() {
        for mode in 0..6 {
            let j = build_json(mode, 5, 0, 1.0);
            assert!(
                j.starts_with("{\"ppqn\":480,\"tempo\":120"),
                "mode {mode}: {j:.60}"
            );
            assert!(j.contains("\"tracks\":["), "mode {mode}: has tracks");
            assert!(j.contains("\"role\":\"a\""), "mode {mode}: has part A");
            assert!(j.ends_with('}'), "mode {mode}: closed object");
        }
    }

    #[test]
    fn counter_melody_succeeds_on_the_uniform_sample() {
        // mode 5 = counter_melody; the sample is uniform 4/4, so no NonUniformTimeline.
        let j = build_json(5, 0, 0, 1.0);
        assert!(j.contains("\"error\":null"), "expected success: {j:.120}");
        assert!(j.contains("\"role\":\"b\""), "counter_melody emits part B");
    }

    #[test]
    fn pitch_spread_changes_rhythm_lock_output() {
        // mode 0 = rhythm_lock: the knob must move B's pitches.
        let locked = build_json(0, 5, 0, 0.0);
        let full = build_json(0, 5, 0, 1.0);
        assert_ne!(
            locked, full,
            "pitch_spread must change a grid-locked complement"
        );
    }

    #[test]
    fn deterministic_for_identical_args() {
        assert_eq!(build_json(5, 7, -12, 0.5), build_json(5, 7, -12, 0.5));
    }
}
