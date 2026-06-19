//! Browser playground (WASM) for the griff complement arranger — ADR-0024/0025.
//!
//! A deliberately thin, throwaway front (ADR-0024 §5): no framework, just two
//! `wasm-bindgen` functions returning JSON strings. Loading Guitar Pro tabs
//! needs the Rust GP reader, which pulls `zip`/`time`/`getrandom` and therefore
//! `wasm-bindgen` glue — so this build is no longer import-free (ADR-0025
//! supersedes the lean cdylib for the web front). Built with `wasm-bindgen
//! --target web` (see `build.sh`). The canonical `egui` frontend (ADR-0016)
//! replaces this at M2.
//!
//! - `arrange(mode, seed, offset, variation, track)` runs
//!   [`arrange_complement_varied`] over a part A — the built-in sample
//!   (`track < 0`) or a track of a user-loaded score (`track >= 0`) — and
//!   returns `{ppqn, tempo, realized_spread, error, tracks:[A, B]}`.
//! - `load_score(bytes)` parses an uploaded MIDI or Guitar Pro file, stashes the
//!   [`Score`], and returns a `{error, ppqn, tempo, bars, tracks}` summary.
//! - `detect_boundaries_json(track)` previews S4 phrase cuts, and
//!   `build_chunk_json(track, …)` captures a track as a schema-v7 `chunk.json`
//!   (ADR-0026) for download and later `griff manifest`.

use std::cell::RefCell;
use std::fmt::Write as _;
use std::ops::Range;

use wasm_bindgen::prelude::*;

use griff_core::complement::{
    arrange_complement_varied, ComplementSpec, RelationMode, VariationControl,
};
use griff_core::event::{NoteMarks, Pitch, Tempo, Ticks, TimeSignature, Tuning, Velocity};
use griff_core::generate::GenerationSeed;
use griff_core::import::import_score_auto;
use griff_core::score::{
    AtomEvent, EventGroup, EventGroupKind, LossReport, MasterBar, RepeatMarker, Score, Track, Voice,
};
use griff_core::slice::{extract_bars, TickRange};
use griff_core::split::bar_segments;

use griff_core::boundary::{self, BoundaryConfig};
use griff_core::corpus::{
    Acquisition, BoundaryEntry, ChunkId, ChunkMeta, QualityFlag, ReviewerDecision, RightsInfo,
    RightsStatus, SourceFormat, SourceRef, StyleCohort, SwancoreTag,
};
use griff_core::{gesture, structure, technique};

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

/// Escapes a string for embedding in JSON. Beyond `\` and `"`, this escapes the
/// control characters (`U+0000..=U+001F`) that RFC 8259 forbids raw: imported
/// MIDI/GP track names and `Debug`/`Display` error strings can carry `\n`, `\t`,
/// or other control bytes that would otherwise make the output unparseable and
/// crash `JSON.parse` in the browser.
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out
}

/// Upload guards for the browser, which loads *untrusted* files. Real swancore
/// tabs are far smaller; these caps stop a giant file from being copied into
/// wasm memory and a tiny Guitar Pro "archive bomb" whose `BCFZ` header declares
/// a multi-gigabyte payload — the GP6 reader `Vec::with_capacity`s that declared
/// length before decompressing a single byte.
const MAX_UPLOAD_BYTES: usize = 16 * 1024 * 1024;
const MAX_GP6_DECOMPRESSED: u32 = 64 * 1024 * 1024;

/// Returns a rejection reason if these bytes should not be handed to the parser
/// (too large, or a Guitar Pro container declaring an implausible payload).
fn reject_upload(data: &[u8]) -> Option<String> {
    if data.len() > MAX_UPLOAD_BYTES {
        return Some(format!(
            "file too large: {} bytes (limit {} MiB)",
            data.len(),
            MAX_UPLOAD_BYTES / (1024 * 1024)
        ));
    }
    // GP6 `.gpx` BCFZ container: bytes 4..8 are the little-endian declared
    // uncompressed length. A tiny file can claim gigabytes, so refuse it before
    // the GP reader allocates that buffer.
    if data.starts_with(b"BCFZ") {
        if let Some(hdr) = data.get(4..8) {
            let declared = u32::from_le_bytes([hdr[0], hdr[1], hdr[2], hdr[3]]);
            if declared > MAX_GP6_DECOMPRESSED {
                return Some(format!(
                    "refusing a Guitar Pro file that declares a {declared}-byte \
                     payload (limit {} MiB) — possible archive bomb",
                    MAX_GP6_DECOMPRESSED / (1024 * 1024)
                ));
            }
        }
    }
    None
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
    static LOADED: RefCell<Option<Score>> = const { RefCell::new(None) };
}

/// Arranges a complement and returns the result JSON. Part A is the loaded
/// score's `track` (when `track >= 0` and a score has been loaded), otherwise
/// the built-in sample.
#[wasm_bindgen]
pub fn arrange(mode: u32, seed: u32, offset: i32, variation: f32, track: i32) -> String {
    let params = ArrangeParams {
        mode,
        seed: u64::from(seed),
        offset,
        variation,
    };
    LOADED.with(|l| match (l.borrow().as_ref(), usize::try_from(track)) {
        (Some(score), Ok(ti)) => arrange_to_json(score, ti, params),
        _ => arrange_to_json(&sample_part_a(), 0, params),
    })
}

/// Parses uploaded file bytes (MIDI or Guitar Pro), stashes the [`Score`] for
/// later [`arrange`] calls, and returns a JSON track summary. On failure leaves
/// any previously loaded score in place and returns `{"error":"…","tracks":[]}`.
#[wasm_bindgen]
pub fn load_score(bytes: &[u8]) -> String {
    load_to_json(bytes)
}

/// Imports `data`, stores the score on success, and returns a JSON summary
/// `{error, ppqn, tempo, bars, tracks:[{i,name,notes,tuning}]}`, where `tuning`
/// is the track's open-string MIDI numbers (low→high; the UI names them).
fn load_to_json(data: &[u8]) -> String {
    if let Some(reason) = reject_upload(data) {
        return format!("{{\"error\":\"{}\",\"tracks\":[]}}", json_escape(&reason));
    }
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
                let tuning = t
                    .tuning
                    .open_strings()
                    .iter()
                    .map(|p| p.0.to_string())
                    .collect::<Vec<_>>()
                    .join(",");
                let _ = write!(
                    json,
                    "{{\"i\":{i},\"name\":\"{}\",\"notes\":{},\"tuning\":[{}]}}",
                    json_escape(&name),
                    note_count(t),
                    tuning
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

// ── chunk.json capture (ADR-0026) ─────────────────────────────────────────────
//
// A thin, download-only path: measure a loaded track exactly as `griff curate`
// does and serialize a real `corpus::ChunkMeta`, so the bytes match what
// `griff manifest` reads. No persistence, no editing — that is the S8 web dock.

/// Maps an imported score's source-format tag to the corpus [`SourceFormat`]
/// (mirrors the CLI's `source_format`); an unknown/absent tag falls back to MIDI.
fn source_format(score: &Score) -> SourceFormat {
    match score.source_meta.as_ref().and_then(|m| m.format.as_deref()) {
        Some("GP3") => SourceFormat::Gp3,
        Some("GP4") => SourceFormat::Gp4,
        Some("GP5") => SourceFormat::Gp5,
        Some("GP6") => SourceFormat::Gpx,
        _ => SourceFormat::Midi,
    }
}

/// Detects S4 phrase boundaries for `track_index`, scaling the detector's tick
/// gaps to the score PPQN exactly as `griff curate`/`griff phrases` do.
fn detect_boundaries(score: &Score, track_index: usize) -> Vec<BoundaryEntry> {
    let ppqn = u32::from(score.ticks_per_quarter);
    let config = BoundaryConfig {
        min_gap: Ticks(ppqn.saturating_mul(2)),
        quantize_ticks: Ticks(ppqn.checked_div(4).unwrap_or(1).max(1)),
        ..BoundaryConfig::default()
    };
    boundary::detect_phrase_boundaries(score, track_index, &config)
        .into_iter()
        .map(|b| BoundaryEntry {
            start_tick: b.start_tick.0,
            end_tick: b.end_tick.0,
            score: b.score,
        })
        .collect()
}

/// Space/comma-separated indices → variants (mirrors the CLI's `parse_indices`).
fn parse_indices<T: Copy>(input: &str, variants: &[T]) -> Vec<T> {
    input
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter_map(|s| s.parse::<usize>().ok())
        .filter_map(|i| variants.get(i).copied())
        .collect()
}

/// Rights-status code (the CLI's prompt order) → enum; unknown → copyrighted.
fn rights_status_from(code: u32) -> RightsStatus {
    match code {
        0 => RightsStatus::PublicDomain,
        1 => RightsStatus::CcBy,
        2 => RightsStatus::CcBySa,
        4 => RightsStatus::Unknown,
        _ => RightsStatus::CopyrightedComposition,
    }
}

/// Acquisition code (the CLI's prompt order) → enum; unknown → community tab.
fn acquisition_from(code: u32) -> Acquisition {
    match code {
        1 => Acquisition::PurchasedOfficial,
        2 => Acquisition::SelfTranscribed,
        3 => Acquisition::OmrFromScan,
        4 => Acquisition::ArtistProvided,
        _ => Acquisition::CommunityTabSite,
    }
}

/// Reviewer code → optional decision (anything outside 0..=2 → none).
fn reviewer_from(code: i32) -> Option<ReviewerDecision> {
    match code {
        0 => Some(ReviewerDecision::Accepted),
        1 => Some(ReviewerDecision::Rejected),
        2 => Some(ReviewerDecision::NeedsReview),
        _ => None,
    }
}

/// Style-cohort code → enum (1 = adjacent, else core).
fn cohort_from(code: u32) -> StyleCohort {
    if code == 1 {
        StyleCohort::Adjacent
    } else {
        StyleCohort::Core
    }
}

/// Assembles a schema-v7 [`ChunkMeta`] for `track_index`, mirroring the CLI's
/// `build_chunk_meta` (single-track, no ensemble). `created_at`/`updated_at` come
/// from the caller so the output is a pure function of its inputs (SPEC §6).
#[allow(clippy::too_many_arguments)] // a capture seam mirroring the CLI's curate inputs
fn build_chunk_meta_record(
    score: &Score,
    track_index: usize,
    id: &str,
    title: &str,
    filename: &str,
    tuning: &str,
    cohort: u32,
    tags_idx: &str,
    quality_idx: &str,
    reviewer: i32,
    rights_status: u32,
    acquisition: u32,
    redistributable: bool,
    notes: &str,
    created_at: &str,
    updated_at: &str,
) -> Result<ChunkMeta, String> {
    if track_index >= score.tracks.len() {
        return Err(format!(
            "track {track_index} out of range (score has {} tracks)",
            score.tracks.len()
        ));
    }
    let (tempo_bpm, time_signature) = score.master_bars.first().map_or((120.0, (4u8, 4u8)), |b| {
        (
            b.tempo.0,
            (b.time_signature.numerator, b.time_signature.denominator),
        )
    });

    let tuning = if tuning.trim().is_empty() {
        "standard_e".to_owned()
    } else {
        tuning.trim().to_owned()
    };
    let chosen_tags = parse_indices(tags_idx, SwancoreTag::all_variants());
    // Fold in techniques the notation already states (ADR-0018): merge their
    // tags with the curator's choices, fill the free-form `techniques` list.
    let derived = technique::derive_techniques(score, track_index);
    let tags = technique::merge_tags(&chosen_tags, &derived.tags);
    let all_flags = [
        QualityFlag::Clean,
        QualityFlag::Lossy,
        QualityFlag::Quantized,
        QualityFlag::FlatDynamics,
    ];
    let quality_flags = if quality_idx.trim().is_empty() {
        vec![QualityFlag::Clean]
    } else {
        parse_indices(quality_idx, &all_flags)
    };
    let filename = if filename.trim().is_empty() {
        "unknown.mid".to_owned()
    } else {
        filename.trim().to_owned()
    };

    Ok(ChunkMeta {
        id: ChunkId(id.trim().to_owned()),
        title: title.trim().to_owned(),
        source: SourceRef {
            filename,
            format: source_format(score),
            bar_range: None,
        },
        tempo_bpm,
        ticks_per_quarter: score.ticks_per_quarter,
        time_signature,
        tuning,
        tags,
        boundaries: detect_boundaries(score, track_index),
        techniques: derived.names,
        quality_flags,
        reviewer: reviewer_from(reviewer),
        structure: structure::measure_structure(score, track_index).ok(),
        gesture: gesture::measure_gesture(score, track_index).ok(),
        complexity: structure::measure_complexity(score, track_index).ok(),
        style_cohort: Some(cohort_from(cohort)),
        ensemble: None,
        rights: Some(RightsInfo {
            rights_status: rights_status_from(rights_status),
            acquisition: acquisition_from(acquisition),
            redistributable,
            notes: notes.trim().to_owned(),
        }),
        created_at: created_at.to_owned(),
        updated_at: updated_at.to_owned(),
    })
}

/// Builds a chunk record and serializes it to pretty JSON, or returns a
/// `{"error":"…"}` envelope. Success output is a bare `ChunkMeta` (no `error`
/// key), byte-compatible with what `griff manifest` reads.
#[allow(clippy::too_many_arguments)]
fn chunk_to_json(
    score: &Score,
    track_index: usize,
    id: &str,
    title: &str,
    filename: &str,
    tuning: &str,
    cohort: u32,
    tags_idx: &str,
    quality_idx: &str,
    reviewer: i32,
    rights_status: u32,
    acquisition: u32,
    redistributable: bool,
    notes: &str,
    created_at: &str,
    updated_at: &str,
) -> String {
    match build_chunk_meta_record(
        score,
        track_index,
        id,
        title,
        filename,
        tuning,
        cohort,
        tags_idx,
        quality_idx,
        reviewer,
        rights_status,
        acquisition,
        redistributable,
        notes,
        created_at,
        updated_at,
    ) {
        Ok(meta) => serde_json::to_string_pretty(&meta).unwrap_or_else(|e| {
            format!(
                "{{\"error\":\"{}\"}}",
                json_escape(&format!("serialize: {e}"))
            )
        }),
        Err(e) => format!("{{\"error\":\"{}\"}}", json_escape(&e)),
    }
}

/// Emits detected phrase boundaries for `track_index` as
/// `{"error":null,"boundaries":[{start_tick,end_tick,score}…]}`.
fn boundaries_to_json(score: &Score, track_index: usize) -> String {
    if track_index >= score.tracks.len() {
        return "{\"error\":\"track out of range\",\"boundaries\":[]}".to_owned();
    }
    let mut json = String::from("{\"error\":null,\"boundaries\":[");
    for (k, b) in detect_boundaries(score, track_index)
        .into_iter()
        .enumerate()
    {
        if k > 0 {
            json.push(',');
        }
        let _ = write!(
            json,
            "{{\"start_tick\":{},\"end_tick\":{},\"score\":{:.3}}}",
            b.start_tick, b.end_tick, b.score
        );
    }
    json.push_str("]}");
    json
}

// ── auto-split capture (#2b): one chunk.json per detected phrase ──────────────
//
// Mirrors the CLI's `griff split` (cmd_split/phrase_chunks/chunks_for_segments):
// cut the selected track at its phrase boundaries, slice each segment into a
// standalone score, and measure + stamp it as its own ChunkMeta. The browser
// paginates the chunks for review and plays each in turn (#2b).

/// Builds the split envelope `{"error":null,"chunks":[…]}` for explicit bar
/// `segments` of `score`'s `track_index`. Each surviving segment is sliced with
/// [`extract_bars`] and measured on that *same* detected track: a segment silent
/// there is a phrase rest and is dropped, never re-measured on another track
/// (`griff split` is single-track). Survivors renumber from 0; each entry
/// carries `bar_lo`/`bar_hi` (inclusive `[start, end-1]`, like
/// [`griff_core::corpus::SourceRef`]), its primary-voice `notes` for playback,
/// and a download-ready pretty `chunk` (`chunk.json` text).
#[allow(clippy::too_many_arguments)]
fn split_segments_to_json(
    score: &Score,
    track_index: usize,
    segments: &[Range<usize>],
    id: &str,
    title: &str,
    filename: &str,
    tuning: &str,
    cohort: u32,
    tags_idx: &str,
    quality_idx: &str,
    reviewer: i32,
    rights_status: u32,
    acquisition: u32,
    redistributable: bool,
    notes: &str,
    created_at: &str,
    updated_at: &str,
) -> String {
    // Slice each segment; keep only those where the detected track sounds.
    let kept: Vec<(usize, usize, Score)> = segments
        .iter()
        .filter_map(|seg| {
            let sub = extract_bars(score, seg.clone());
            match sub.tracks.get(track_index) {
                Some(t) if note_count(t) > 0 => Some((seg.start, seg.end, sub)),
                _ => None,
            }
        })
        .collect();

    let mut json = String::from("{\"error\":null,\"chunks\":[");
    for (phrase, (start, end, sub)) in kept.iter().enumerate() {
        let pid = format!("{id}_p{phrase}");
        let ptitle = format!("{title} (phrase {phrase})");
        let mut meta = match build_chunk_meta_record(
            sub,
            track_index,
            &pid,
            &ptitle,
            filename,
            tuning,
            cohort,
            tags_idx,
            quality_idx,
            reviewer,
            rights_status,
            acquisition,
            redistributable,
            notes,
            created_at,
            updated_at,
        ) {
            Ok(m) => m,
            // Unreachable for a kept (in-range, sounding) track, but surface it
            // as an envelope rather than panicking in the browser.
            Err(e) => return format!("{{\"error\":\"{}\",\"chunks\":[]}}", json_escape(&e)),
        };
        let bar_lo = u32::try_from(*start).unwrap_or(0);
        let bar_hi = u32::try_from(end.saturating_sub(1)).unwrap_or(0);
        meta.source.bar_range = Some((bar_lo, bar_hi));

        let pretty = serde_json::to_string_pretty(&meta)
            .unwrap_or_else(|e| format!("{{\"error\":\"serialize: {e}\"}}"));

        if phrase > 0 {
            json.push(',');
        }
        let _ = write!(
            json,
            "{{\"id\":\"{}\",\"title\":\"{}\",\"bar_lo\":{bar_lo},\"bar_hi\":{bar_hi},\"notes\":",
            json_escape(&pid),
            json_escape(&ptitle),
        );
        // `kept` guarantees the track is present and note-bearing.
        if let Some(part) = sub.tracks.get(track_index) {
            push_notes(&mut json, part);
        } else {
            json.push_str("[]");
        }
        let _ = write!(json, ",\"chunk\":\"{}\"}}", json_escape(&pretty));
    }
    json.push_str("]}");
    json
}

/// Splits `track_index` of `score` at its detected phrase boundaries into one
/// chunk per sounding phrase (mirrors the CLI's `phrase_chunks`). An out-of-range
/// track or a bar-less score yields a `{"error":…,"chunks":[]}` envelope.
#[allow(clippy::too_many_arguments)]
fn split_to_json(
    score: &Score,
    track_index: usize,
    id: &str,
    title: &str,
    filename: &str,
    tuning: &str,
    cohort: u32,
    tags_idx: &str,
    quality_idx: &str,
    reviewer: i32,
    rights_status: u32,
    acquisition: u32,
    redistributable: bool,
    notes: &str,
    created_at: &str,
    updated_at: &str,
) -> String {
    if track_index >= score.tracks.len() {
        return format!(
            "{{\"error\":\"track {track_index} out of range (score has {} tracks)\",\"chunks\":[]}}",
            score.tracks.len()
        );
    }
    let cuts: Vec<u32> = detect_boundaries(score, track_index)
        .iter()
        .map(|b| b.start_tick)
        .collect();
    let segments = bar_segments(&score.master_bars, &cuts);
    if segments.is_empty() {
        return "{\"error\":\"score has no bars to split\",\"chunks\":[]}".to_owned();
    }
    split_segments_to_json(
        score,
        track_index,
        &segments,
        id,
        title,
        filename,
        tuning,
        cohort,
        tags_idx,
        quality_idx,
        reviewer,
        rights_status,
        acquisition,
        redistributable,
        notes,
        created_at,
        updated_at,
    )
}

/// Returns the swancore tag palette — the wire names of
/// `SwancoreTag::all_variants()` in order — as a JSON array, so the capture UI's
/// tag indices line up with what [`build_chunk_json`] parses.
#[wasm_bindgen]
pub fn tag_palette_json() -> String {
    serde_json::to_string(SwancoreTag::all_variants()).unwrap_or_else(|_| "[]".to_owned())
}

/// Detects phrase boundaries for `track` of the loaded score and returns them as
/// JSON, so the capture UI can preview phrase cuts before building a chunk.
#[wasm_bindgen]
pub fn detect_boundaries_json(track: u32) -> String {
    LOADED.with(|l| match l.borrow().as_ref() {
        Some(score) => boundaries_to_json(score, track as usize),
        None => "{\"error\":\"no score loaded\",\"boundaries\":[]}".to_owned(),
    })
}

/// Captures `track` of the loaded score as a schema-v7 `chunk.json` (ADR-0026):
/// measures the track and serializes a `corpus::ChunkMeta`. Returns the chunk
/// JSON on success or a `{"error":"…"}` envelope (no score / track out of range).
#[wasm_bindgen]
#[allow(clippy::too_many_arguments)]
pub fn build_chunk_json(
    track: u32,
    id: &str,
    title: &str,
    filename: &str,
    tuning: &str,
    cohort: u32,
    tags: &str,
    quality: &str,
    reviewer: i32,
    rights_status: u32,
    acquisition: u32,
    redistributable: bool,
    notes: &str,
    created_at: &str,
    updated_at: &str,
) -> String {
    LOADED.with(|l| match l.borrow().as_ref() {
        Some(score) => chunk_to_json(
            score,
            track as usize,
            id,
            title,
            filename,
            tuning,
            cohort,
            tags,
            quality,
            reviewer,
            rights_status,
            acquisition,
            redistributable,
            notes,
            created_at,
            updated_at,
        ),
        None => "{\"error\":\"no score loaded\"}".to_owned(),
    })
}

/// Splits `track` of the loaded score into one schema-v7 `chunk.json` per
/// detected phrase (#2b), so the capture UI can paginate the phrases for review
/// and play each in turn. Returns
/// `{"error":…|null,"chunks":[{id,title,bar_lo,bar_hi,notes,chunk}…]}`, where
/// each `chunk` is a download-ready `chunk.json`. The capture inputs mirror
/// [`build_chunk_json`] and are inherited by every phrase (ids/titles suffixed
/// `_p<N>` / `(phrase <N>)`).
#[wasm_bindgen]
#[allow(clippy::too_many_arguments)]
pub fn split_chunks_json(
    track: u32,
    id: &str,
    title: &str,
    filename: &str,
    tuning: &str,
    cohort: u32,
    tags: &str,
    quality: &str,
    reviewer: i32,
    rights_status: u32,
    acquisition: u32,
    redistributable: bool,
    notes: &str,
    created_at: &str,
    updated_at: &str,
) -> String {
    LOADED.with(|l| match l.borrow().as_ref() {
        Some(score) => split_to_json(
            score,
            track as usize,
            id,
            title,
            filename,
            tuning,
            cohort,
            tags,
            quality,
            reviewer,
            rights_status,
            acquisition,
            redistributable,
            notes,
            created_at,
            updated_at,
        ),
        None => "{\"error\":\"no score loaded\",\"chunks\":[]}".to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::{
        arrange_to_json, boundaries_to_json, build_chunk_meta_record, chunk_to_json, json_escape,
        load_to_json, parse_indices, sample_part_a, tag_palette_json, ArrangeParams,
    };

    #[test]
    fn json_escape_escapes_quotes_backslashes_and_control_chars() {
        // Backslash and quote.
        assert_eq!(json_escape(r#"a\b"c"#), r#"a\\b\"c"#);
        // The common control chars get their short forms.
        assert_eq!(json_escape("l1\nl2\tx\r"), "l1\\nl2\\tx\\r");
        // Other control bytes (e.g. a bell, 0x07) fall back to \uXXXX.
        assert_eq!(json_escape("\u{0007}"), "\\u0007");
        // Non-control Unicode passes through untouched (valid in UTF-8 JSON).
        assert_eq!(json_escape("café ✓"), "café ✓");
    }

    #[test]
    fn reject_upload_caps_size_and_refuses_archive_bombs() {
        use super::{reject_upload, MAX_GP6_DECOMPRESSED, MAX_UPLOAD_BYTES};
        // A small, ordinary input is accepted.
        assert!(reject_upload(b"MThd\0\0\0\x06").is_none());
        // Oversized input is rejected up front.
        assert!(reject_upload(&vec![0u8; MAX_UPLOAD_BYTES + 1]).is_some());
        // A BCFZ container declaring more than the cap is refused as a bomb...
        let mut bomb = b"BCFZ".to_vec();
        bomb.extend_from_slice(&(MAX_GP6_DECOMPRESSED + 1).to_le_bytes());
        assert!(reject_upload(&bomb).unwrap().contains("archive bomb"));
        // ...while a modest declared payload passes the guard.
        let mut modest = b"BCFZ".to_vec();
        modest.extend_from_slice(&1024u32.to_le_bytes());
        assert!(reject_upload(&modest).is_none());
    }

    #[test]
    fn load_rejects_oversized_uploads_with_error_json() {
        let json = load_to_json(&vec![0u8; super::MAX_UPLOAD_BYTES + 1]);
        assert!(json.contains("file too large"), "{json:.120}");
        assert!(json.contains("\"tracks\":[]"), "{json:.120}");
    }

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
        assert!(
            summary.contains("\"tuning\":["),
            "summary reports a per-track tuning field: {summary:.200}"
        );

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

    #[test]
    fn load_routes_guitar_pro_bytes_to_the_gp_reader() {
        // A header-only GP5 fuzz seed: enough to be recognised as Guitar Pro and
        // routed to the GP reader, which reports a clean GP parse error. This
        // proves the `gp` feature is on in the wasm build — a MIDI fallback would
        // report a "MIDI" error instead. (Couples to griff-core's GP error text:
        // if that wording changes, update the substring checked below.)
        let gp = include_bytes!("../../fuzz/corpus/guitar_pro_import/gp5_header_only.gp5");
        let json = load_to_json(gp);
        assert!(
            json.contains("\"error\":\""),
            "a truncated GP file must surface an error: {json}"
        );
        assert!(
            json.contains("Guitar Pro"),
            "the GP reader handled it, not the MIDI fallback: {json}"
        );
    }

    // ── chunk.json capture (ADR-0026) ─────────────────────────────────────────

    #[test]
    fn parse_indices_keeps_in_range_indices_only() {
        let v = [10u8, 20, 30];
        // Out-of-range indices are dropped; whitespace- or comma-separated.
        assert_eq!(parse_indices("0 2 5", &v), vec![10, 30]);
        assert_eq!(parse_indices("1,2", &v), vec![20, 30]);
        assert!(parse_indices("", &v).is_empty());
    }

    #[test]
    fn build_chunk_record_mirrors_curate_and_round_trips_through_corpus() {
        use griff_core::corpus::{
            Acquisition, ChunkMeta, QualityFlag, ReviewerDecision, RightsStatus, StyleCohort,
        };
        let score = sample_part_a();
        let meta = build_chunk_meta_record(
            &score,
            0,
            "dgd_001",
            "Test Riff",
            "riff.gp5",
            "   ", // blank tuning → default
            1,     // cohort: adjacent
            "0 5", // two tag indices
            "",    // quality blank → [Clean]
            0,     // reviewer: accepted
            3,
            0,
            false, // rights: copyrighted, community_tab_site, not redistributable
            "from example.com 2026-06-17",
            "2026-06-17T00:00:00Z",
            "2026-06-17T00:00:00Z",
        )
        .expect("record builds for an in-range track");

        assert_eq!(meta.id.0, "dgd_001");
        assert_eq!(
            meta.tuning, "standard_e",
            "blank tuning defaults like the CLI"
        );
        assert_eq!(meta.style_cohort, Some(StyleCohort::Adjacent));
        assert_eq!(meta.quality_flags, vec![QualityFlag::Clean]);
        assert_eq!(meta.reviewer, Some(ReviewerDecision::Accepted));
        assert_eq!(meta.tags.len(), 2, "two tag indices mapped");
        let rights = meta
            .rights
            .as_ref()
            .expect("rights captured (non-derivable, S5)");
        assert_eq!(rights.rights_status, RightsStatus::CopyrightedComposition);
        assert_eq!(rights.acquisition, Acquisition::CommunityTabSite);
        assert!(!rights.redistributable);

        // The emitted JSON must deserialize back into the same corpus ChunkMeta —
        // i.e. it is byte-compatible with what `griff manifest` consumes.
        let json = serde_json::to_string_pretty(&meta).expect("serialize");
        let back: ChunkMeta = serde_json::from_str(&json).expect("griff manifest can read it");
        assert_eq!(back, meta);
    }

    #[test]
    fn build_chunk_record_auto_fills_techniques_from_notation() {
        use griff_core::corpus::SwancoreTag;
        use griff_core::event::{NoteMark, SpanTechnique, TechniqueEvidence, Ticks};
        use griff_core::score::{AtomEvent, TechniqueSpan};
        use griff_core::slice::TickRange;

        // Inject a hammer-on span + a pinch-harmonic note into the sample's voice 0.
        let mut score = sample_part_a();
        let group = score
            .tracks
            .first_mut()
            .expect("track")
            .voices
            .first_mut()
            .expect("voice")
            .event_groups
            .first_mut()
            .expect("group");
        group.technique_spans.push(TechniqueSpan {
            technique: SpanTechnique::HammerOn,
            tick_range: TickRange::new(Ticks(0), Ticks(240)).expect("range"),
            evidence: TechniqueEvidence::explicit(),
        });
        if let AtomEvent::Note(n) = group.atoms.first_mut().expect("atom") {
            n.marks.insert(NoteMark::HarmonicPinch);
        }

        // The curator hand-picks only "intro" (index 21); derivation adds the rest.
        let meta = build_chunk_meta_record(
            &score, 0, "dgd_001", "Riff", "riff.gp5", "standard_e", 0, "21", "", -1, 3, 0,
            false, "", "t", "t",
        )
        .expect("record builds");

        // `techniques` filled from notation (superset includes the pinch harmonic).
        assert!(meta.techniques.contains(&"hammer_on".to_owned()), "{:?}", meta.techniques);
        assert!(
            meta.techniques.contains(&"pinch_harmonic".to_owned()),
            "{:?}",
            meta.techniques
        );
        // Curator's chosen tag survives; derived technique tags are merged in.
        assert!(meta.tags.contains(&SwancoreTag::Intro), "chosen tag kept: {:?}", meta.tags);
        assert!(meta.tags.contains(&SwancoreTag::HammerOn), "{:?}", meta.tags);
        assert!(meta.tags.contains(&SwancoreTag::ArtificialHarmonic), "{:?}", meta.tags);
    }

    #[test]
    fn build_chunk_record_rejects_out_of_range_track() {
        let score = sample_part_a();
        let err = build_chunk_meta_record(
            &score,
            99,
            "x",
            "x",
            "f.mid",
            "standard_e",
            0,
            "",
            "",
            -1,
            3,
            0,
            false,
            "",
            "t",
            "t",
        )
        .unwrap_err();
        assert!(err.contains("out of range"), "{err}");
    }

    #[test]
    fn chunk_to_json_emits_chunkmeta_on_success_and_error_envelope_on_failure() {
        let score = sample_part_a();
        // Success: a ChunkMeta has no top-level "error" key and parses as one.
        let ok = chunk_to_json(
            &score,
            0,
            "dgd_002",
            "Captured",
            "riff.mid",
            "standard_e",
            0,
            "",
            "",
            -1,
            3,
            0,
            false,
            "",
            "2026-06-17T00:00:00Z",
            "2026-06-17T00:00:00Z",
        );
        assert!(
            !ok.contains("\"error\""),
            "no error key on success: {ok:.160}"
        );
        let meta: griff_core::corpus::ChunkMeta =
            serde_json::from_str(&ok).expect("valid ChunkMeta JSON");
        assert_eq!(meta.id.0, "dgd_002");
        assert!(meta.rights.is_some());

        // Failure: an out-of-range track yields a parseable error envelope.
        let bad = chunk_to_json(
            &score,
            99,
            "x",
            "x",
            "f.mid",
            "standard_e",
            0,
            "",
            "",
            -1,
            3,
            0,
            false,
            "",
            "t",
            "t",
        );
        assert!(bad.contains("\"error\":\""), "error envelope: {bad}");
    }

    #[test]
    fn boundaries_to_json_reports_for_a_track_and_errors_out_of_range() {
        let score = sample_part_a();
        let j = boundaries_to_json(&score, 0);
        assert!(j.contains("\"error\":null"), "ok: {j:.120}");
        assert!(j.contains("\"boundaries\":["), "has a boundaries array");
        let oor = boundaries_to_json(&score, 99);
        assert!(oor.contains("out of range"), "out-of-range errors: {oor}");
    }

    #[test]
    fn tag_palette_json_lists_every_tag_in_wire_order() {
        let names: Vec<String> =
            serde_json::from_str(&tag_palette_json()).expect("array of tag names");
        // One entry per SwancoreTag, so capture-UI indices line up with the
        // indices `build_chunk_json` parses.
        assert_eq!(
            names.len(),
            griff_core::corpus::SwancoreTag::all_variants().len()
        );
        assert!(!names.is_empty());
        // Wire names are serde snake_case (lowercase + underscores).
        assert!(
            names.iter().all(|n| !n.is_empty()
                && n.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')),
            "snake_case palette: {names:?}"
        );
    }

    // ── auto-split capture (#2b): one chunk.json per detected phrase ───────────

    use super::{split_segments_to_json, split_to_json};
    use griff_core::corpus::ChunkMeta;
    use griff_core::event::{NoteMarks, Pitch, Tempo, Ticks, TimeSignature, Tuning, Velocity};
    use griff_core::score::{
        AtomEvent, AtomNote, EventGroup, EventGroupKind, LossReport, MasterBar, RepeatMarker,
        Score, Track, Voice,
    };
    use griff_core::slice::TickRange;
    use serde_json::Value;
    use std::ops::Range;

    fn quarter(start: u32, pitch: u8) -> AtomEvent {
        AtomEvent::Note(AtomNote {
            absolute_start: Ticks(start),
            duration: Ticks(480),
            pitch: Pitch::new(pitch).expect("pitch"),
            velocity: Velocity::new(90).expect("velocity"),
            marks: NoteMarks::empty(),
            position: None,
        })
    }

    /// A voice `id` holding `atoms` (one event group each).
    fn voice_of(id: u8, atoms: Vec<AtomEvent>) -> Voice {
        Voice {
            id,
            event_groups: atoms
                .into_iter()
                .map(|a| EventGroup {
                    kind: EventGroupKind::Single,
                    atoms: vec![a],
                    technique_spans: Vec::new(),
                })
                .collect(),
        }
    }

    /// A track over explicit `voices`.
    fn track_voices(voices: Vec<Voice>) -> Track {
        Track {
            name: None,
            channel: 0,
            voices,
            tuning: Tuning::standard_e(),
        }
    }

    /// A single primary-voice track holding `atoms`.
    fn track_notes(atoms: Vec<AtomEvent>) -> Track {
        track_voices(vec![voice_of(0, atoms)])
    }

    /// Four contiguous 4/4 bars (ticks 0..7680 at 480 PPQN) over `tracks`.
    fn four_bar_score(tracks: Vec<Track>) -> Score {
        let master_bars = (0..4)
            .map(|i| {
                let start = u32::try_from(i).unwrap_or(0) * 1920;
                MasterBar {
                    index: i,
                    tick_range: TickRange::new(Ticks(start), Ticks(start + 1920)).expect("ordered"),
                    time_signature: TimeSignature {
                        numerator: 4,
                        denominator: 4,
                    },
                    tempo: Tempo::new(120.0).expect("bpm"),
                    repeat: RepeatMarker::default(),
                }
            })
            .collect();
        Score {
            ticks_per_quarter: 480,
            master_bars,
            tracks,
            source_meta: None,
            loss: LossReport::new(),
        }
    }

    /// Splits explicit `segments` with fixed capture inputs; parses the envelope.
    fn split_segs(score: &Score, track: usize, segments: &[Range<usize>]) -> Value {
        let json = split_segments_to_json(
            score,
            track,
            segments,
            "dgd",
            "Riff",
            "riff.gp5",
            "standard_e",
            0,
            "",
            "",
            -1,
            3,
            0,
            false,
            "",
            "2026-06-18T00:00:00Z",
            "2026-06-18T00:00:00Z",
        );
        serde_json::from_str(&json).expect("split envelope is valid JSON")
    }

    /// The embedded pretty `chunk` of entry `i`, parsed back into a `ChunkMeta`.
    fn chunk_meta(env: &Value, i: usize) -> ChunkMeta {
        let text = env["chunks"][i]["chunk"]
            .as_str()
            .expect("chunk json string");
        serde_json::from_str(text).expect("chunk is a valid ChunkMeta")
    }

    #[test]
    fn split_segments_renumber_sounding_phrases_with_inclusive_bar_range() {
        // One track sounding in every bar: both segments survive, renumbered.
        let score = four_bar_score(vec![track_notes(vec![
            quarter(0, 60),
            quarter(1920, 62),
            quarter(3840, 64),
            quarter(5760, 65),
        ])]);
        let env = split_segs(&score, 0, &[0..2, 2..4]);
        assert!(env["error"].is_null(), "{env}");
        let chunks = env["chunks"].as_array().expect("array");
        assert_eq!(chunks.len(), 2, "one chunk per sounding segment");

        assert_eq!(chunks[0]["id"], "dgd_p0");
        assert_eq!(chunks[0]["bar_lo"], 0);
        assert_eq!(chunks[0]["bar_hi"], 1);
        assert!(
            !chunks[0]["notes"].as_array().expect("notes").is_empty(),
            "phrase carries its notes for playback"
        );
        assert_eq!(chunks[1]["id"], "dgd_p1");
        assert_eq!(chunks[1]["bar_lo"], 2);
        assert_eq!(chunks[1]["bar_hi"], 3);

        // The embedded chunk is a real ChunkMeta carrying the inclusive range.
        let m0 = chunk_meta(&env, 0);
        assert_eq!(m0.id.0, "dgd_p0");
        assert_eq!(m0.source.bar_range, Some((0, 1)));
    }

    #[test]
    fn split_segments_drop_phrases_silent_on_the_detected_track() {
        // Track 0 sounds only in bars 0–1; track 1 only in bars 2–3. Splitting
        // track 0 keeps just its sounding phrase — the [2,4) segment is a rest on
        // track 0 and is dropped, never re-measured on track 1 (single-track).
        let score = four_bar_score(vec![
            track_notes(vec![quarter(0, 60), quarter(1920, 62)]),
            track_notes(vec![quarter(3840, 48), quarter(5760, 50)]),
        ]);
        let env = split_segs(&score, 0, &[0..2, 2..4]);
        let chunks = env["chunks"].as_array().expect("array");
        assert_eq!(chunks.len(), 1, "the track-0-silent [2,4) segment is dropped");
        assert_eq!(chunks[0]["id"], "dgd_p0");
        assert_eq!(chunks[0]["bar_lo"], 0);
        assert_eq!(chunks[0]["bar_hi"], 1);
    }

    #[test]
    fn split_to_json_errors_on_out_of_range_track() {
        let json = split_to_json(
            &sample_part_a(),
            99,
            "dgd",
            "Riff",
            "riff.mid",
            "standard_e",
            0,
            "",
            "",
            -1,
            3,
            0,
            false,
            "",
            "t",
            "t",
        );
        let env: Value = serde_json::from_str(&json).expect("valid json");
        assert!(
            env["error"]
                .as_str()
                .unwrap_or_default()
                .contains("out of range"),
            "{env}"
        );
        assert!(env["chunks"].as_array().expect("array").is_empty());
    }

    #[test]
    fn split_to_json_splits_the_sample_into_sounding_phrases() {
        let json = split_to_json(
            &sample_part_a(),
            0,
            "dgd",
            "Riff",
            "riff.mid",
            "standard_e",
            0,
            "",
            "",
            -1,
            3,
            0,
            false,
            "",
            "2026-06-18T00:00:00Z",
            "2026-06-18T00:00:00Z",
        );
        let env: Value = serde_json::from_str(&json).expect("valid json");
        assert!(env["error"].is_null(), "{env}");
        let chunks = env["chunks"].as_array().expect("array");
        assert!(!chunks.is_empty(), "at least one phrase chunk");
        assert_eq!(chunks[0]["id"], "dgd_p0");
        // Every emitted chunk is a manifest-readable ChunkMeta.
        for i in 0..chunks.len() {
            let _ = chunk_meta(&env, i);
        }
    }

    #[test]
    fn split_drops_a_track_sounding_only_in_a_secondary_voice() {
        // The whole analysis stack — phrase-boundary detection, structure /
        // gesture / complexity, and playback — reads voice 0 (the primary
        // voice). A track whose notes live only in a secondary voice is
        // unmeasurable, so the split treats it as a rest and drops it, matching
        // the CLI's `primary_voice_note_count` contract. Scanning all voices
        // here would cut phrases on voice 0 yet keep/measure them on voice 1.
        let score = four_bar_score(vec![track_voices(vec![
            voice_of(0, Vec::new()),
            voice_of(1, vec![quarter(0, 60), quarter(1920, 62)]),
        ])]);
        let env = split_segs(&score, 0, &[0..2, 2..4]);
        assert!(env["error"].is_null(), "{env}");
        assert!(
            env["chunks"].as_array().expect("array").is_empty(),
            "a secondary-voice-only track is dropped (voice-0 analysis convention)"
        );
    }
}
