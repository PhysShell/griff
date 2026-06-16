//! Browser playground (WASM) for the griff complement arranger — ADR-0024.
//!
//! A deliberately thin, throwaway front (ADR-0024 §5): no `wasm-bindgen`, no
//! framework. It exports a handful of C-ABI functions and marshals a JSON
//! result through linear memory, so the build is just `cargo build --target
//! wasm32-unknown-unknown` and the page is static files. The canonical `egui`
//! frontend (ADR-0016) replaces this at M2.
//!
//! `arrange(mode, seed, offset, variation, track)` runs
//! [`arrange_complement_varied`] over a part A — either the built-in sample
//! (`track < 0`) or a track of a user-loaded score (`track >= 0`) — and writes
//! `{ppqn, tempo, realized_spread, error, tracks:[A, B]}` into a thread-local
//! buffer; JS reads it via the returned pointer + `arrange_len()`.
//!
//! File loading is import-free MIDI (the `gp` feature is off in the wasm build,
//! per ADR-0024): JS writes the file bytes into `input_alloc(len)`, calls
//! `load_score(len)` to parse + stash the [`Score`], and reads a track summary
//! (`load_len()`); `arrange(.., track)` then arranges over the chosen track.

use std::cell::RefCell;
use std::fmt::Write as _;

use griff_core::complement::{
    arrange_complement_varied, ComplementSpec, RelationMode, VariationControl,
};
use griff_core::event::{NoteMarks, Pitch, Tempo, Ticks, TimeSignature, Tuning, Velocity};
use griff_core::generate::GenerationSeed;
use griff_core::import::import_score_auto;
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

/// The four knobs behind one arrangement (mirror the UI controls).
#[derive(Clone, Copy)]
struct ArrangeParams {
    mode: u32,
    seed: u64,
    offset: i32,
    variation: f32,
}

/// Builds the result JSON for one arrangement request over
/// `source.tracks[track_index]` as part A. The header `ppqn`/`tempo` are read
/// off the source so a loaded score plays back at its own resolution.
fn arrange_to_json(source: &Score, track_index: usize, params: ArrangeParams) -> String {
    let ppqn = source.ticks_per_quarter;
    let tempo = source.master_bars.first().map_or(TEMPO, |b| b.tempo.0);

    let mut json = String::with_capacity(2048);
    let _ = write!(json, "{{\"ppqn\":{ppqn},\"tempo\":{tempo},");

    let part_a = match source.tracks.get(track_index) {
        Some(t) => t,
        None => {
            json.push_str("\"realized_spread\":0,\"error\":\"track out of range\",\"tracks\":[]}");
            return json;
        }
    };
    let a_name = part_a.name.clone().unwrap_or_else(|| "A".to_string());

    let spec = ComplementSpec {
        mode: relation_mode(params.mode),
        register_offset: params.offset.clamp(-48, 48) as i8,
    };
    let control = VariationControl {
        pitch_spread: f64::from(params.variation).clamp(0.0, 1.0),
    };

    match arrange_complement_varied(
        source,
        track_index,
        spec,
        GenerationSeed(params.seed),
        control,
    ) {
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
            let _ = write!(
                json,
                "{{\"name\":\"{}\",\"role\":\"a\",\"notes\":",
                json_escape(&a_name)
            );
            push_notes(&mut json, part_a);
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
            // Escape it: a Debug repr can carry quotes/backslashes that would
            // otherwise break the JSON the browser parses.
            let err = json_escape(&format!("{e:?}"));
            let _ = write!(
                json,
                "\"realized_spread\":0,\"error\":\"{err}\",\"tracks\":["
            );
            let _ = write!(
                json,
                "{{\"name\":\"{}\",\"role\":\"a\",\"notes\":",
                json_escape(&a_name)
            );
            push_notes(&mut json, part_a);
            json.push_str("}]");
        }
    }
    json.push('}');
    json
}

thread_local! {
    static OUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    static IN: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    static LOADED: RefCell<Option<Score>> = const { RefCell::new(None) };
}

/// Arranges a complement and stores the JSON result; returns a pointer into
/// WASM linear memory. Read `arrange_len()` bytes from it (valid until the next
/// `arrange` call). Part A is the loaded score's `track` (when `track >= 0` and
/// a score has been loaded), otherwise the built-in sample.
#[no_mangle]
pub extern "C" fn arrange(
    mode: u32,
    seed: u32,
    offset: i32,
    variation: f32,
    track: i32,
) -> *const u8 {
    let params = ArrangeParams {
        mode,
        seed: u64::from(seed),
        offset,
        variation,
    };
    let json = LOADED.with(|l| match (l.borrow().as_ref(), usize::try_from(track)) {
        (Some(score), Ok(ti)) => arrange_to_json(score, ti, params),
        _ => arrange_to_json(&sample_part_a(), 0, params),
    });
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

/// Reserves `len` bytes of input buffer and returns a writable pointer into
/// linear memory. JS copies the uploaded file's bytes here, then calls
/// [`load_score`]. (Create the JS view *after* this call: memory may grow.)
#[no_mangle]
pub extern "C" fn input_alloc(len: usize) -> *mut u8 {
    IN.with(|b| {
        let mut b = b.borrow_mut();
        b.clear();
        b.resize(len, 0);
        b.as_mut_ptr()
    })
}

/// Parses the first `len` bytes of the input buffer (MIDI; Guitar Pro when the
/// `gp` feature is on), stashes the [`Score`] for later [`arrange`] calls, and
/// writes a JSON track summary into the output buffer. Returns a pointer to it;
/// read [`load_len`] bytes. On failure, leaves any previously loaded score in
/// place and writes `{"error":"…","tracks":[]}`.
#[no_mangle]
pub extern "C" fn load_score(len: usize) -> *const u8 {
    let json = IN.with(|b| {
        let bytes = b.borrow();
        let end = len.min(bytes.len());
        load_to_json(&bytes[..end])
    });
    OUT.with(|o| {
        *o.borrow_mut() = json.into_bytes();
        o.borrow().as_ptr()
    })
}

/// Length in bytes of the JSON stored by the last [`load_score`] call.
#[no_mangle]
pub extern "C" fn load_len() -> usize {
    OUT.with(|o| o.borrow().len())
}

/// Imports `data`, stores the score on success, and returns a JSON summary
/// `{error, ppqn, tempo, bars, tracks:[{i,name,notes}]}`.
fn load_to_json(data: &[u8]) -> String {
    match import_score_auto(data) {
        Ok(score) => {
            let ppqn = score.ticks_per_quarter;
            let tempo = score.master_bars.first().map_or(TEMPO, |b| b.tempo.0);
            let mut json = String::with_capacity(512);
            let _ = write!(
                json,
                "{{\"error\":null,\"ppqn\":{ppqn},\"tempo\":{tempo},\"bars\":{},\"tracks\":[",
                score.master_bars.len()
            );
            for (i, t) in score.tracks.iter().enumerate() {
                if i > 0 {
                    json.push(',');
                }
                let name = t.name.clone().unwrap_or_else(|| format!("track {i}"));
                let _ = write!(
                    json,
                    "{{\"i\":{i},\"name\":\"{}\",\"notes\":{}}}",
                    json_escape(&name),
                    note_count(t)
                );
            }
            json.push_str("]}");
            LOADED.with(|l| *l.borrow_mut() = Some(score));
            json
        }
        Err(e) => {
            let err = json_escape(&format!("{e}"));
            format!("{{\"error\":\"{err}\",\"tracks\":[]}}")
        }
    }
}

/// Counts pitched notes in a track's primary voice (what the playground draws).
fn note_count(track: &Track) -> usize {
    track.voices.first().map_or(0, |v| {
        v.event_groups
            .iter()
            .flat_map(|g| &g.atoms)
            .filter(|a| matches!(a, AtomEvent::Note(_)))
            .count()
    })
}

#[cfg(test)]
mod tests {
    use super::{arrange_to_json, load_to_json, sample_part_a, ArrangeParams};

    /// Arrange over the built-in sample (the old `build_json` behaviour).
    fn sample_json(mode: u32, seed: u64, offset: i32, variation: f32) -> String {
        arrange_to_json(
            &sample_part_a(),
            0,
            ArrangeParams {
                mode,
                seed,
                offset,
                variation,
            },
        )
    }

    #[test]
    fn every_mode_emits_part_a_and_well_formed_header() {
        for mode in 0..6 {
            let j = sample_json(mode, 5, 0, 1.0);
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
        let j = sample_json(5, 0, 0, 1.0);
        assert!(j.contains("\"error\":null"), "expected success: {j:.120}");
        assert!(j.contains("\"role\":\"b\""), "counter_melody emits part B");
    }

    #[test]
    fn pitch_spread_changes_rhythm_lock_output() {
        // mode 0 = rhythm_lock: the knob must move B's pitches.
        let locked = sample_json(0, 5, 0, 0.0);
        let full = sample_json(0, 5, 0, 1.0);
        assert_ne!(
            locked, full,
            "pitch_spread must change a grid-locked complement"
        );
    }

    #[test]
    fn deterministic_for_identical_args() {
        assert_eq!(sample_json(5, 7, -12, 0.5), sample_json(5, 7, -12, 0.5));
    }

    #[test]
    fn load_rejects_non_midi_bytes() {
        let j = load_to_json(b"definitely not a midi file");
        assert!(j.contains("\"error\":\""), "expected an error: {j}");
        assert!(!j.contains("\"error\":null"), "must not claim success");
    }

    #[test]
    fn load_then_arrange_round_trips_a_midi_score() {
        // Export the built-in sample to MIDI, re-import it through the public
        // entry, and arrange over the imported track — the file-load path.
        let bytes = griff_core::midi::export_score(&sample_part_a()).expect("export");
        let summary = load_to_json(&bytes);
        assert!(summary.contains("\"error\":null"), "import: {summary:.160}");
        assert!(summary.contains("\"tracks\":["), "summary lists tracks");

        let imported = griff_core::import::import_score_auto(&bytes).expect("reimport");
        let j = arrange_to_json(
            &imported,
            0,
            ArrangeParams {
                mode: 5,
                seed: 0,
                offset: 0,
                variation: 1.0,
            },
        );
        assert!(
            j.contains("\"role\":\"a\""),
            "arranges the imported track: {j:.160}"
        );
    }
}
