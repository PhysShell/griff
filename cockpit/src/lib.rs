//! egui cockpit for griff — the renderer side of ADR-0027 (Slices 1–2).
//!
//! Paints the shared [`griff_ui_core`] `Scene` (the same placed grid the
//! `ratatui` preview draws) into an `eframe`/`egui` window, and maps raw egui
//! input back to the core's semantic [`Intent`]s. Per ADR-0016 this first egui
//! slice renders the existing `Scene` and nothing else: all layout and
//! interaction logic stays in `griff-ui-core`; this crate only maps placed
//! cells to pixels and key presses to intents.
//!
//! The same [`CockpitApp`] runs on two targets: a native `eframe` window (see
//! `main.rs` / `run_native`) and the browser, where the wasm `start` entry
//! boots it on an HTML canvas via eframe's `WebGL` runner (Slice 2). Both drive
//! the identical resolve → paint and input → intent path.

// Pixel layout is bounded arithmetic (cell sizes × small grid counts) plus
// float→cell casts; every value is clamped to the panel and the grid, so the
// overflow/precision/cast lints carry no signal here (the `render` rasteriser
// allows the same set).
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::arithmetic_side_effects,
    // Pixel coordinates: a fused multiply-add buys precision/perf that screen
    // layout has no use for, at the cost of legibility.
    clippy::suboptimal_flops
)]

#[cfg(not(target_arch = "wasm32"))]
use std::path::{Path, PathBuf};

use eframe::egui::{self, Align2, Color32, CornerRadius, FontId, Key, Rect};

use griff_core::corpus::{ChunkMeta, ReviewerDecision, RightsStatus, StyleCohort, SwancoreTag};
use griff_core::generation_input::CorpusMaterial;
use griff_core::import::import_score_auto;
use griff_core::score::Score;
use griff_swang::eval;
use griff_ui_core::history::{
    CandidateSource, ChainOutcomeRecord, ChainSupplier, CorpusContribution, GenerationRunId,
    GeneratorProvenance, HistoryId, Provenance, SessionHistory, Verdict,
};
use griff_ui_core::playback::{Player, TempoMap};

use audio::Synth;
use griff_core::candidate_chain::{ChainError, MasterBarField, TrackField, TransitionFactError};
use griff_core::layered_path::PathError;
use griff_core::scoring::RationaleEntry;
use griff_ui_core::curation::{decide_record, rename_record, set_tags, tag_palette};
use griff_ui_core::generate::{
    generate_run, global_chain_summary, ChainBarView, ChainBoundaryView, GeneratedRun,
    GlobalChainOutcome, GlobalChainSummary, PlannedGlobalChain,
};
use griff_ui_core::scene::{resolve, GridSize, SceneCell, GUTTER};
use griff_ui_core::theme::{cell_style, Rgb, Theme};
use griff_ui_core::viewport::CurationDecision;
use griff_ui_core::{
    analyze, build_chunk, build_view, filter_chunks, Analysis, CaptureInputs, CorpusFilter,
    CorpusStats, Intent, PianoRollView, Step, ViewContext, Viewport,
};

pub mod audio;
pub mod generation;
pub mod swang;

use generation::{kept_provenance, ActiveGenerateRun, GeneratePanel, GenerateRunContext};
use swang::SwangPanel;

/// Pixel width of one grid cell.
const CELL_W: f32 = 9.0;
/// Pixel height of one grid cell (also the section-band row height).
const CELL_H: f32 = 16.0;

// ── palette ─────────────────────────────────────────────────────────────────
// There is none here. The semantic palette lives in `griff_ui_core::theme`
// (ADR-0028), so this renderer and the ratatui preview cannot drift apart the
// way they did when the cockpit painted the section band as a bare colour
// block while the preview printed the class name. All this crate owns is the
// conversion into egui's colour type.

/// The core's colour, in egui's terms.
const fn color32(c: Rgb) -> Color32 {
    Color32::from_rgb(c.r, c.g, c.b)
}

/// Maps an egui key to the core's semantic [`Intent`], for the navigation and
/// playback subset this slice exposes (curation intents arrive with the dock).
const fn key_to_intent(key: Key) -> Option<Intent> {
    Some(match key {
        Key::Space => Intent::TogglePlay,
        Key::ArrowLeft => Intent::ScrollLeft,
        Key::ArrowRight => Intent::ScrollRight,
        Key::ArrowUp => Intent::PitchUp,
        Key::ArrowDown => Intent::PitchDown,
        Key::Plus | Key::Equals => Intent::ZoomIn,
        Key::Minus => Intent::ZoomOut,
        Key::OpenBracket => Intent::PrevSection,
        Key::CloseBracket => Intent::NextSection,
        Key::Home | Key::Num0 => Intent::Home,
        Key::I => Intent::ToggleInspector,
        Key::Q | Key::Escape => Intent::Quit,
        _ => return None,
    })
}

/// Builds the [`ViewContext`] for a view + analysis, mirroring the preview
/// front-end (the context is shell-side and identical across renderers).
fn build_context(view: &PianoRollView, analysis: &Analysis) -> ViewContext {
    ViewContext {
        tick_start: view.tick_start,
        tick_end: view.tick_end,
        ppq: view.ppq,
        tempo_bpm: view.tempo_bpm,
        section_starts: analysis.sections.iter().map(|s| s.tick_start).collect(),
        tag_count: 0,
        initial_tags: 0,
        has_record: false,
        can_merge: false,
        bar_ticks: match view.bar_lines.as_slice() {
            [first, second, ..] => second.saturating_sub(*first),
            _ => 0,
        },
        grid_end: view.bar_lines.last().copied().unwrap_or(0),
    }
}

// ── capture panel (ADR-0026) ────────────────────────────────────────────────

/// Rights-status options (code, label) in the CLI's prompt order.
const RIGHTS: &[(u32, &str)] = &[
    (0, "public domain"),
    (1, "CC-BY"),
    (2, "CC-BY-SA"),
    (3, "copyrighted"),
    (4, "unknown"),
];
/// Acquisition options (code, label) in the CLI's prompt order.
const ACQUISITION: &[(u32, &str)] = &[
    (0, "community tab"),
    (1, "purchased"),
    (2, "self-transcribed"),
    (3, "OMR scan"),
    (4, "artist-provided"),
];
/// Style-cohort options (code, label).
const COHORT: &[(u32, &str)] = &[(0, "core"), (1, "adjacent")];

// ── corpus dock (ADR-0027 Slice 5) ───────────────────────────────────────────

/// Rights-status filter options for the dock (labels mirror the capture combo).
const RIGHTS_STATUS: &[(RightsStatus, &str)] = &[
    (RightsStatus::PublicDomain, "public domain"),
    (RightsStatus::CcBy, "CC-BY"),
    (RightsStatus::CcBySa, "CC-BY-SA"),
    (RightsStatus::CopyrightedComposition, "copyrighted"),
    (RightsStatus::Unknown, "unknown"),
];
/// Style-cohort filter options for the dock.
const COHORTS: &[(StyleCohort, &str)] = &[
    (StyleCohort::Core, "core"),
    (StyleCohort::Adjacent, "adjacent"),
];

/// A combo selecting an optional filter facet; the "any" row clears it.
fn opt_combo<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut Option<T>,
    options: &[(T, &str)],
) {
    let current = value
        .and_then(|v| options.iter().find(|(t, _)| *t == v).map(|&(_, l)| l))
        .unwrap_or("any");
    egui::ComboBox::from_label(label)
        .selected_text(current)
        .show_ui(ui, |ui| {
            ui.selectable_value(value, None, "any");
            for &(variant, lbl) in options {
                ui.selectable_value(value, Some(variant), lbl);
            }
        });
}

/// The tag facet — every [`SwancoreTag`], or "any".
fn tag_combo(ui: &mut egui::Ui, value: &mut Option<SwancoreTag>) {
    let current = value.map_or_else(|| "any tag".to_owned(), |tag| format!("{tag:?}"));
    egui::ComboBox::from_label("tag")
        .selected_text(current)
        .show_ui(ui, |ui| {
            ui.selectable_value(value, None, "any tag".to_owned());
            for &tag in SwancoreTag::all_variants() {
                ui.selectable_value(value, Some(tag), format!("{tag:?}"));
            }
        });
}

/// One row of the dock's chunk list — a selectable label (a click selects the
/// chunk for curation), prefixed by its reviewer/dup marks, plus cohort and
/// rights. Returns whether the row was clicked this frame.
fn chunk_row(ui: &mut egui::Ui, chunk: &ChunkMeta, selected: bool) -> bool {
    ui.horizontal(|ui| {
        let mark = match chunk.reviewer {
            Some(ReviewerDecision::Accepted) => "✓ ",
            Some(ReviewerDecision::Rejected) => "✗ ",
            // No decision yet reads as "needs a look", like an explicit NeedsReview.
            Some(ReviewerDecision::NeedsReview) | None => "? ",
        };
        let dup = if chunk.duplicate.is_some() {
            "≈ "
        } else {
            ""
        };
        let clicked = ui
            .selectable_label(selected, format!("{mark}{dup}{}", chunk.id.0))
            .clicked();
        if let Some(cohort) = chunk.style_cohort {
            ui.weak(format!("{cohort:?}"));
        }
        match &chunk.rights {
            Some(rights) if rights.redistributable => {
                ui.colored_label(Color32::from_rgb(0x73, 0xd1, 0x3d), "↗")
                    .on_hover_text("redistributable");
            }
            Some(rights) => {
                ui.weak(format!("{:?}", rights.rights_status));
            }
            None => {
                ui.weak("rights?");
            }
        }
        clicked
    })
    .inner
}

/// The curation inspector for the selected chunk (ADR-0027 Slice 6): rename,
/// approve / reject, and retag. Each control sets the `action` the dock then
/// applies through the shared `griff_ui_core::curation` ops.
fn inspector(
    ui: &mut egui::Ui,
    chunk: &ChunkMeta,
    rename_buf: &mut String,
    action: &mut Option<CurationAction>,
) {
    ui.label(format!("▸ {}", chunk.id.0));
    ui.horizontal(|ui| {
        ui.text_edit_singleline(rename_buf);
        if ui.button("rename").clicked() {
            *action = Some(CurationAction::Rename(rename_buf.clone()));
        }
    });
    ui.horizontal(|ui| {
        if ui.button("✓ approve").clicked() {
            *action = Some(CurationAction::Decide(CurationDecision::Approve));
        }
        if ui.button("✗ reject").clicked() {
            *action = Some(CurationAction::Decide(CurationDecision::Reject));
        }
    });
    ui.label("tags");
    let palette = tag_palette();
    let mut names: Vec<String> = chunk.tags.iter().filter_map(|&t| tag_wire(t)).collect();
    ui.horizontal_wrapped(|ui| {
        for tag in &palette {
            let present = names.contains(tag);
            if ui.selectable_label(present, tag.as_str()).clicked() {
                if present {
                    names.retain(|n| n != tag);
                } else {
                    names.push(tag.clone());
                }
                *action = Some(CurationAction::Retag(names.clone()));
            }
        }
    });
}

/// A labelled single-line text field row.
fn text_field(ui: &mut egui::Ui, label: &str, value: &mut String) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.add(egui::TextEdit::singleline(value).desired_width(210.0));
    });
}

/// A labelled [`egui::ComboBox`] selecting a `u32` code from `(code, label)`s.
fn combo(ui: &mut egui::Ui, label: &str, value: &mut u32, options: &[(u32, &str)]) {
    let current = options
        .iter()
        .find(|opt| opt.0 == *value)
        .map_or("", |opt| opt.1);
    egui::ComboBox::from_label(label)
        .selected_text(current)
        .show_ui(ui, |ui| {
            for opt in options {
                ui.selectable_value(value, opt.0, opt.1);
            }
        });
}

/// Editable capture-form state (ADR-0026): the curator-supplied inputs the
/// panel edits before building a `chunk.json`.
///
/// Mirrors [`CaptureInputs`] with owned fields, plus a transient status line.
#[derive(Debug)]
struct CaptureForm {
    id: String,
    title: String,
    filename: String,
    tuning: String,
    tags_idx: String,
    notes: String,
    cohort: u32,
    rights_status: u32,
    acquisition: u32,
    redistributable: bool,
    status: Option<String>,
}

impl Default for CaptureForm {
    fn default() -> Self {
        Self {
            id: String::new(),
            title: String::new(),
            filename: String::new(),
            tuning: String::new(),
            tags_idx: String::new(),
            notes: String::new(),
            cohort: 0,        // core
            rights_status: 3, // copyrighted — the safe default until stated
            acquisition: 0,   // community tab
            redistributable: false,
            status: None,
        }
    }
}

impl CaptureForm {
    /// Borrows the form as [`CaptureInputs`], stamping `created`/`updated`.
    /// Quality and reviewer keep their `build_chunk` defaults (`[Clean]` / none).
    fn inputs<'a>(&'a self, created: &'a str, updated: &'a str) -> CaptureInputs<'a> {
        CaptureInputs {
            id: &self.id,
            title: &self.title,
            filename: &self.filename,
            tuning: &self.tuning,
            cohort: self.cohort,
            tags_idx: &self.tags_idx,
            quality_idx: "",
            reviewer: -1,
            rights_status: self.rights_status,
            acquisition: self.acquisition,
            redistributable: self.redistributable,
            notes: &self.notes,
            created_at: created,
            updated_at: updated,
        }
    }

    /// Seeds id/title/filename from a loaded source name (a slug of the stem).
    fn seed_from(&mut self, source: &str) {
        let stem = source.rsplit('/').next().unwrap_or(source);
        let base = stem.rsplit_once('.').map_or(stem, |(name, _)| name);
        let slug: String = base
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() {
                    c.to_ascii_lowercase()
                } else {
                    '_'
                }
            })
            .collect();
        slug.trim_matches('_').clone_into(&mut self.id);
        base.clone_into(&mut self.title);
        stem.clone_into(&mut self.filename);
        self.status = None;
    }
}

/// The current time as an RFC3339 timestamp (`created_at`/`updated_at`).
#[cfg(target_arch = "wasm32")]
fn now_rfc3339() -> String {
    js_sys::Date::new_0()
        .to_iso_string()
        .as_string()
        .unwrap_or_default()
}

/// The current time as an RFC3339 timestamp, from the system clock (no `chrono`:
/// Howard Hinnant's civil-from-days over the Unix epoch).
#[cfg(not(target_arch = "wasm32"))]
fn now_rfc3339() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let (hh, mm, ss) = (secs / 3600 % 24, secs / 60 % 60, secs % 60);
    let z = i64::try_from(secs / 86_400).unwrap_or(0) + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}T{hh:02}:{mm:02}:{ss:02}Z")
}

/// Saves a captured `chunk.json`: a browser download on web, a file in the
/// working directory on native.
#[cfg(target_arch = "wasm32")]
fn save_chunk(filename: &str, json: &str) -> Result<(), String> {
    web::persist(filename, json); // accumulate the OPFS corpus
    web::download(filename, json) // and export a copy
}

#[cfg(not(target_arch = "wasm32"))]
fn save_chunk(filename: &str, json: &str) -> Result<(), String> {
    use std::fs;
    fs::write(filename, json).map_err(|err| err.to_string())
}

/// The OPFS/disk filename for a chunk `id` — a slug so a stray `/` or `..` can't
/// escape the corpus dir (#98); the chunk's own id keeps its raw value. Capture
/// and curation derive the same name, so an edit overwrites its own file.
fn chunk_filename(id: &str) -> String {
    let slug: String = id
        .trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    let stem = slug.trim_matches('_');
    let stem = if stem.is_empty() { "chunk" } else { stem };
    format!("{stem}.chunk.json")
}

/// Re-persists an edited chunk to the corpus (OPFS on web, the working dir on
/// native) — no download, unlike capture's `save_chunk`.
// The OPFS write is fire-and-forget, so the web arm is infallible; the signature
// matches the fallible native arm so callers treat both the same.
#[cfg(target_arch = "wasm32")]
#[allow(clippy::unnecessary_wraps)]
fn persist_chunk(filename: &str, json: &str) -> Result<(), String> {
    web::persist(filename, json);
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn persist_chunk(filename: &str, json: &str) -> Result<(), String> {
    use std::fs;
    fs::write(filename, json).map_err(|err| err.to_string())
}

/// A curation edit on the selected chunk (ADR-0027 Slice 6), applied through the
/// shared `griff_ui_core::curation` ops so the cockpit never reimplements them.
enum CurationAction {
    /// Approve / reject — sets the reviewer decision.
    Decide(CurationDecision),
    /// Rename the chunk's title.
    Rename(String),
    /// Replace the chunk's tags with this set (wire names).
    Retag(Vec<String>),
}

/// A `SwancoreTag`'s wire name (`snake_case`), for the retag toggles.
fn tag_wire(tag: SwancoreTag) -> Option<String> {
    serde_json::to_value(tag)
        .ok()?
        .as_str()
        .map(ToOwned::to_owned)
}

/// A candidate the roll can show, tagged by which set it belongs to — so A/B
/// remembers the last one viewed across **both** generators and swaps back to
/// the right source, never a same-index candidate of the wrong set.
/// Where a Swang run's seed score **actually** came from.
///
/// Native reads the program's declared `source` path; the browser has no
/// filesystem and seeds from the displayed score instead. Recording which one
/// happened keeps provenance honest — the web target must not claim it read a
/// path it never opened.
#[derive(Debug, Clone, PartialEq, Eq)]
enum SwangSourceOrigin {
    /// The declared path was read from disk (native).
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))] // only native resolves a path
    ResolvedPath(String),
    /// The displayed score seeded the run (the browser's defined semantics).
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))] // only wasm seeds this way
    DisplayedScore,
}

impl SwangSourceOrigin {
    /// The path provenance should record: the resolved path, or `None` when the
    /// displayed score seeded the run (which a UI renders as "displayed score").
    fn provenance_path(&self) -> Option<String> {
        match self {
            Self::ResolvedPath(path) => Some(path.clone()),
            Self::DisplayedScore => None,
        }
    }
}

/// The **immutable** identity of one successful Swang run: the run id, the
/// exact evaluated program text, and the resolved source. A candidate's
/// provenance reads these — never the live editor text — so editing the program
/// after a run cannot rewrite an already-made candidate's origin.
#[derive(Debug, Clone)]
struct SwangRunContext {
    /// The run this evaluated set belongs to.
    run: GenerationRunId,
    /// The exact program text that was evaluated.
    program: String,
    /// The resolved source path, if the frontend read one.
    source_path: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AuditionCandidate {
    /// Index into the Generate panel's set.
    Generate(usize),
    /// The active Generate run's assembled S7 global chain. Carries no index: a
    /// run has exactly one chain, planned when the set was produced.
    GlobalChain,
    /// Index into the Swang panel's set.
    Swang(usize),
    /// A recorded candidate replayed from the session history, by its stable id
    /// (S8 Slice 3) — so A/B and the playhead survive a switch to an entry from
    /// an earlier generation whose panel set is long gone.
    History(HistoryId),
}

/// The history dedupe key for a run's global chain.
///
/// A run has exactly one chain, so a constant is the honest key: re-auditioning
/// it must land on the same entry rather than pile up duplicates, and the run id
/// beside it already distinguishes one run's chain from another's.
const CHAIN_CANDIDATE_ID: &str = "global-chain";

/// The egui cockpit application: a `Scene` renderer over the shared core.
#[derive(Debug)]
pub struct CockpitApp {
    view: PianoRollView,
    analysis: Analysis,
    title: String,
    vp: Viewport,
    ctx: ViewContext,
    fitted: bool,
    /// The imported score behind the view, kept for capture (ADR-0026); `None`
    /// for views built directly (tests) or before the first load.
    score: Option<Score>,
    /// The capture-panel form state (shown when the inspector is toggled).
    form: CaptureForm,
    /// The OPFS corpus loaded for the dock (ADR-0027 Slice 5); empty until the
    /// page reads the `chunk.json` tree. The dock browses and aggregates these.
    corpus: Vec<ChunkMeta>,
    /// The corpus dock's active browse filter.
    corpus_filter: CorpusFilter,
    /// Whether the corpus dock window is shown (the `c` key toggles it).
    show_dock: bool,
    /// The selected chunk's id, for the curation inspector (ADR-0027 Slice 6).
    selected: Option<String>,
    /// The inspector's rename-field buffer (seeded from the selection's title).
    rename_buf: String,
    /// The last curation action's outcome, shown in the dock.
    dock_status: Option<String>,
    /// Which track the roll shows in isolation — an index into the full score's
    /// tracks. The roll, sections, and capture all follow it (the toolbar picks).
    selected_track: usize,
    /// Every track's display name, for the toolbar's track selector.
    track_names: Vec<String>,
    /// The Generate panel: knobs, the last candidate set, the selection (S8).
    gen_panel: GeneratePanel,
    /// The Swang editor: program text, diagnostics, the last run's candidates
    /// (S8 Playground).
    swang: SwangPanel,
    /// The playback backend — native MIDI or web audio (S8 Slice 2).
    synth: Synth,
    /// The note schedule of the shown score, driven as the playhead sweeps.
    player: Player,
    /// Playback-speed multiplier over the score's written tempo — `1.0` plays
    /// as written, `2.0` at double speed. A tempo audition override: it never
    /// touches the Score, its MIDI export, or provenance.
    tempo_scale: f64,
    /// The master timeline's tempo — `(start tick, BPM)` segments — so playback
    /// bends at every tempo change instead of playing the whole score at the
    /// first bar's BPM. The single source of tempo (per the repo constraint).
    tempo_map: TempoMap,
    /// The **fractional** playhead in ticks — the source of truth while
    /// playing. `vp.play_tick` is its floor, for the visuals and the schedule;
    /// keeping the remainder here stops sub-tick frames stalling or drifting.
    play_pos: f64,
    /// The loop range in ticks, when looping is on: playback wraps to the
    /// start when it reaches the end.
    loop_range: Option<(u32, u32)>,
    /// The candidate currently painted, tagged by source — the anchor A/B
    /// swaps away from. `None` until the first candidate is shown.
    current: Option<AuditionCandidate>,
    /// The A/B "other" candidate — the last one viewed before `current`, of
    /// either source — that a single key swaps back to without regenerating.
    ab_other: Option<AuditionCandidate>,
    /// The session's append-only candidate history: every candidate shown is
    /// recorded here with its provenance and the curator's verdict (S8 Slice 3).
    /// A new generation adds to it; it is never destroyed within a session.
    history: SessionHistory,
    /// The current Swang run's immutable context — its run id, evaluated
    /// program, and resolved source — captured on a successful `swang_run`, so
    /// a candidate's provenance never reads the live editor text (S8 Slice 3).
    /// The Generate run's context lives with its set in `gen_panel.active`.
    swang_ctx: Option<SwangRunContext>,
    /// Whether the history window is shown (the `y` key toggles it).
    history_open: bool,
    /// The playhead tick at the end of the last frame — so an input that
    /// seeks the head (a section jump) is noticed and the audio repositioned.
    last_play_tick: u32,
    /// The corpus material a generation pass consumes — rhythm templates,
    /// novelty references, the gesture ask. `None` until a corpus is loaded;
    /// a pass then seeds from the displayed score alone.
    material: Option<CorpusMaterial>,
    /// Where a kept candidate is written (native only).
    #[cfg(not(target_arch = "wasm32"))]
    out_dir: PathBuf,
    /// The palette every cell — and the egui chrome around it — resolves through
    /// (ADR-0028). Owned, not global: the renderer never invents a colour.
    theme: Theme,
}

/// A single-track view of `score`: just `track`, so the roll shows one part
/// instead of every track overlaid. Out-of-range falls back to the whole score.
fn single_track_score(score: &Score, track: usize) -> Score {
    let mut sub = score.clone();
    if let Some(one) = score.tracks.get(track).cloned() {
        sub.tracks = vec![one];
    }
    sub
}

/// The score's master-timeline tempo as playback segments — one `(start tick,
/// BPM)` per bar, with equal-BPM runs collapsed by [`TempoMap`]. This is what
/// the playhead auditions; the master timeline is the single source of tempo.
fn tempo_map_of(score: &Score) -> TempoMap {
    TempoMap::new(
        score
            .master_bars
            .iter()
            .map(|mb| (mb.tick_range.start.0, mb.tempo.0))
            .collect(),
    )
}

/// Re-derives a loop that covered bars `from..=to` onto a **new** view's
/// `bar_lines`, clamped so `0 <= lo < hi <= tick_end`. Returns `None` when
/// those bars no longer span a real range in the new view — a shorter score
/// clears (or clamps) the loop rather than letting playback wander past its
/// end. Pure, so the remap is unit-tested without an egui app.
fn remap_loop_range(
    from: usize,
    to: usize,
    bar_lines: &[u32],
    tick_end: u32,
) -> Option<(u32, u32)> {
    let last = bar_lines.len().saturating_sub(1);
    let a = from.min(to).min(last);
    let b = from.max(to).min(last);
    let lo = *bar_lines.get(a)?;
    let hi = *bar_lines.get(b.saturating_add(1).min(last))?;
    (lo < hi && hi <= tick_end).then_some((lo, hi))
}

/// The sidecar written beside an exported global chain.
///
/// Mirrors the entry's typed `GlobalChain` provenance, exactly as
/// `KeptProvenance` mirrors a candidate's — the serialisable shape lives here,
/// in the frontend that owns the file format, and the model stays typed.
///
/// The supplier map is the load-bearing part: a `.mid` of a chain is otherwise
/// indistinguishable from a `.mid` of anything else, and "which candidate played
/// bar 2" is the one question this export exists to be able to answer later.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, serde::Serialize)]
struct KeptChain<'a> {
    /// What this file is — so a reader who finds it alone can tell.
    origin: &'static str,
    /// The Generate run that produced the chain.
    run: u64,
    /// The seed score's identity.
    source: Option<&'a str>,
    /// What the corpus **actually** contributed to that run.
    corpus: KeptCorpus,
    /// The ask seed.
    seed: u64,
    /// Bars the run asked for. From the captured ask, never from
    /// `suppliers.len()` — a result may not be its own evidence.
    bars: usize,
    /// Seed variants per strategy the run asked for.
    variants_per_strategy: usize,
    /// The chain policy the costs were weighed under.
    policy_id: &'a str,
    /// That policy's version.
    policy_version: u32,
    /// Ranked candidate 0 kept intact, under the same policy.
    baseline_cost: f64,
    /// The planned chain's total.
    total_cost: f64,
    /// Which candidate supplied each output bar.
    suppliers: Vec<KeptSupplier<'a>>,
}

/// The `origin` marker a chain sidecar carries.
#[cfg(not(target_arch = "wasm32"))]
const CHAIN_SIDECAR_ORIGIN: &str = "candidate_chain";

/// What the corpus contributed, as the sidecar writes it.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, serde::Serialize)]
struct KeptCorpus {
    /// Rhythm templates the corpus supplied.
    templates: usize,
    /// Novelty reference chunks it supplied.
    references: usize,
    /// Whether a corpus gesture was actually carved.
    gesture: bool,
}

/// One bar's supplier, as the sidecar writes it.
///
/// Mirrored rather than derived onto [`ChainSupplier`]: `history` is
/// deliberately free of serialisation, so the model cannot quietly acquire a
/// wire format and a compatibility obligation with it. The frontend that writes
/// the file owns the file's shape.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, serde::Serialize)]
struct KeptSupplier<'a> {
    /// The output bar this candidate filled.
    bar: usize,
    /// The supplying candidate's ordinal in the ranked set.
    candidate: usize,
    /// That candidate's own 1-based rank.
    rank: usize,
    /// That candidate's strategy.
    strategy: &'a str,
    /// That candidate's derived variant seed — its reproduction key.
    variant_seed: u64,
}

/// Paints a planned chain's costs, supplier map, and the core's own rationales.
///
/// Every number here was measured by the core and carried through
/// [`global_chain_summary`]. This function's whole job is layout: it does not
/// weigh, sum, or compare anything.
fn chain_summary_block(ui: &mut egui::Ui, summary: &GlobalChainSummary) {
    ui.monospace(format!("Baseline cost: {:.3}", summary.baseline_cost));
    ui.monospace(format!("Chain cost:    {:.3}", summary.total_cost));
    // Signed arithmetic, named for what it is. "improvement" would be a verdict
    // the policy did not deliver: `candidate_chain` v1 prefers a lower total,
    // and preferring is not the same as sounding better.
    ui.monospace(format!(
        "Delta:         {:+.3}  ({} under {} v{})",
        summary.delta,
        delta_relation(summary.delta),
        summary.policy_id,
        summary.policy_version,
    ));
    ui.separator();

    for (i, bar) in summary.bars.iter().enumerate() {
        ui.monospace(format!(
            "Bar {}  candidate {} · rank {} · {} · seed {:016x}",
            bar.bar_number, bar.candidate, bar.rank, bar.strategy, bar.variant_seed,
        ))
        .on_hover_text(bar_hover(bar));
        if let Some(boundary) = summary.boundaries.get(i) {
            ui.monospace(format!("   ┆ {}", boundary_line(boundary)))
                .on_hover_text(rationale_hover(&boundary.rationale));
        }
    }
}

/// How the chain's total compares to the intact winner's, in one word.
///
/// Stub: still calls an equal cost "higher".
fn delta_relation(delta: f64) -> &'static str {
    if delta < 0.0 {
        "lower"
    } else {
        "higher"
    }
}

/// A bar's local explanation, as hover text.
fn bar_hover(bar: &ChainBarView) -> String {
    // `candidate_quality` is only there if the policy charged it; absent stays
    // absent here too rather than becoming a printed zero.
    let quality = bar.candidate_quality.map_or_else(String::new, |quality| {
        format!("candidate_quality {quality:.3}\n")
    });
    let axes = bar
        .s6_axes
        .iter()
        .map(|(label, value)| format!("  {label} {value:.3}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "chain local {:.3}\nS6 aggregate {:.3}\n{quality}{}\n\nS6 axes:\n{axes}",
        bar.local_aggregate,
        bar.s6_aggregate,
        rationale_hover(&bar.local_rationale),
    )
}

/// A boundary's facts, in one line.
///
/// An unmeasured jump says so. It is not rendered as `0`, which would read as
/// the smoothest possible join rather than as an unanswered question.
fn boundary_line(boundary: &ChainBoundaryView) -> String {
    let jump = boundary.jump_semitones.map_or_else(
        || "jump not measured".to_owned(),
        |semitones| format!("jump {semitones:.0} st"),
    );
    let silent = if boundary.silent_boundary {
        " · silent boundary"
    } else {
        ""
    };
    let repeat = if boundary.rhythm_repeat {
        " · rhythm repeat"
    } else {
        ""
    };
    format!("{jump}{silent}{repeat} · cost {:.3}", boundary.aggregate)
}

/// The core's weighted rationale, as hover text.
fn rationale_hover(rationale: &[RationaleEntry]) -> String {
    rationale
        .iter()
        .map(|e| {
            format!(
                "{} {:.3} × {:.2} = {:.3}",
                e.axis, e.value, e.weight, e.contribution
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// A planned chain's typed provenance, from the run that captured it.
///
/// One construction shared by both paths that can record the chain — the
/// audition and the export — so they cannot describe the same result
/// differently. Every request field comes from the immutable context; every
/// result field from the captured plan.
fn chain_provenance(ctx: &GenerateRunContext, chain: &PlannedGlobalChain) -> GeneratorProvenance {
    GeneratorProvenance::GlobalChain {
        source: ctx.source.clone(),
        corpus: ctx.corpus,
        seed: ctx.seed,
        bars: ctx.bars,
        variants_per_strategy: ctx.variants_per_strategy,
        policy_id: chain.plan.provenance.policy_id,
        policy_version: chain.plan.provenance.policy_version,
        baseline_cost: chain.baseline_cost,
        total_cost: chain.plan.total_cost,
        suppliers: chain_suppliers(chain),
    }
}

/// The history title a planned chain wears.
fn chain_title(chain: &PlannedGlobalChain) -> String {
    format!("S7 global chain · {:.3}", chain.plan.total_cost)
}

/// The run-level record of what a captured chain outcome came to.
///
/// A projection of the captured outcome, taken at capture time: the costs and
/// the supplier map when planned, the core's own typed error when not. It holds
/// no score — the assembled one lives on the history entry the audition records,
/// and a refusal has none at all.
fn chain_outcome_record(chain: &GlobalChainOutcome) -> ChainOutcomeRecord {
    match chain {
        GlobalChainOutcome::Planned(chain) => ChainOutcomeRecord::Planned {
            policy_id: chain.plan.provenance.policy_id,
            policy_version: chain.plan.provenance.policy_version,
            baseline_cost: chain.baseline_cost,
            total_cost: chain.plan.total_cost,
            // The core's trace, so an old chain can still explain itself once
            // the run that made it has been replaced.
            steps: chain.plan.steps.clone(),
            transitions: chain.plan.transitions.clone(),
        },
        GlobalChainOutcome::Refused(error) => ChainOutcomeRecord::Refused { error: *error },
    }
}

/// Which candidate supplied each bar of a planned chain, from the core's steps.
fn chain_suppliers(chain: &PlannedGlobalChain) -> Vec<ChainSupplier> {
    chain
        .plan
        .steps
        .iter()
        .map(|step| ChainSupplier {
            bar: step.state.bar,
            candidate: step.state.candidate,
            rank: step.state.rank,
            strategy: format!("{:?}", step.state.strategy),
            variant_seed: step.state.variant_seed.0,
        })
        .collect()
}

/// Why the S7 global chain could not be planned, in a sentence — built here, in
/// the UI layer, from the core's typed error, exactly as [`provenance_summary`]
/// is built from typed provenance.
///
/// `format!("{err:?}")` is a Rust value printed at a person: it names variants
/// and braces, and explains nothing. A refusal is usually about the *set* rather
/// than about anything the user did, so it has to say which candidate and which
/// fact, in words.
fn chain_refusal_summary(error: ChainError) -> String {
    match error {
        ChainError::EmptySet => "the set has no candidates to chain".to_owned(),
        ChainError::NoBars => "the candidates have no bars to chain".to_owned(),
        ChainError::BarCountMismatch {
            candidate,
            expected,
            found,
        } => format!(
            "candidate {candidate} is {found} bars long, not {expected} — \
             the chain is never truncated to the shortest candidate"
        ),
        ChainError::PpqMismatch {
            candidate,
            expected,
            found,
        } => format!(
            "candidate {candidate} measures time at {found} ticks per quarter, not {expected}"
        ),
        ChainError::MasterBarMismatch {
            candidate,
            bar,
            field,
        } => format!(
            "candidate {candidate} disagrees about bar {bar}'s {} — \
             the candidates must share one timeline to be chained",
            master_bar_field_name(field),
        ),
        ChainError::TrackCountMismatch {
            candidate,
            expected,
            found,
        } => format!("candidate {candidate} has {found} tracks, not {expected}"),
        ChainError::TrackMetadataMismatch {
            candidate,
            track,
            field,
        } => format!(
            "candidate {candidate} disagrees about track {track}'s {}",
            track_field_name(field),
        ),
        ChainError::SourceMetaMismatch { candidate } => {
            format!("candidate {candidate} names a different source format")
        }
        ChainError::LossReportMismatch { candidate } => format!(
            "candidate {candidate} carries a different import loss report — \
             the chain will not merge two histories into one"
        ),
        ChainError::CrossBarMaterial { candidate, bar } => format!(
            "candidate {candidate} has material in bar {bar} that does not fit inside \
             the bar line — a bar cannot be lifted out without cutting it"
        ),
        ChainError::EmptyEventGroup { candidate } => {
            format!("candidate {candidate} has an event group with no notes or rests in it")
        }
        ChainError::MaterialOutsideTimeline { candidate, tick } => format!(
            "candidate {candidate} has material at tick {tick}, which is past the end \
             of its own timeline"
        ),
        ChainError::MissingMaterial {
            candidate,
            track,
            voice,
            bar,
        } => format!("candidate {candidate} has no track {track}, voice {voice}, bar {bar}"),
        ChainError::BoundaryFact(TransitionFactError::MissingFromBar { bar, bars }) => {
            format!("a boundary was measured from bar {bar} of a {bars}-bar candidate")
        }
        ChainError::BoundaryFact(TransitionFactError::MissingToBar { bar, bars }) => {
            format!("a boundary was measured into bar {bar} of a {bars}-bar candidate")
        }
        ChainError::Path(path) => path_error_summary(path),
    }
}

/// The name of a master-bar fact, in the words a musician uses for it.
const fn master_bar_field_name(field: MasterBarField) -> &'static str {
    match field {
        MasterBarField::Index => "number",
        MasterBarField::TickRange => "position",
        MasterBarField::TimeSignature => "time signature",
        MasterBarField::Tempo => "tempo",
        MasterBarField::Repeat => "repeat barlines",
    }
}

/// The name of a track fact, in the words a musician uses for it.
const fn track_field_name(field: TrackField) -> &'static str {
    match field {
        TrackField::Name => "name",
        TrackField::Channel => "channel",
        TrackField::Tuning => "tuning",
        TrackField::VoiceCount => "voice count",
        TrackField::VoiceId => "voice numbering",
    }
}

/// Why the layered solver rejected the problem the chain handed it.
///
/// These are invariant violations rather than facts about the music, so they
/// say so plainly instead of pretending the set did something musical wrong.
fn path_error_summary(error: PathError) -> String {
    match error {
        PathError::NoLayers => "the chain problem had no bars".to_owned(),
        PathError::EmptyLayer { layer } => format!("bar {layer} had no candidates to choose from"),
        PathError::TransitionCount { expected, found } => {
            format!("the chain problem had {found} boundary tables, not {expected}")
        }
        PathError::TransitionShape {
            layer,
            expected,
            found,
        } => format!("bar {layer}'s boundary table is {found:?}, not {expected:?}"),
        PathError::NonFiniteLocal { state, cost } => format!(
            "bar {}'s candidate {} scored {cost}, which is not a number the chain can compare",
            state.layer, state.ordinal,
        ),
        PathError::NonFiniteTransition { edge, cost } => format!(
            "the boundary from bar {} into bar {} scored {cost}, \
             which is not a number the chain can compare",
            edge.from.layer, edge.to.layer,
        ),
        PathError::NonFiniteAccumulation { state, cost } => format!(
            "the costs stopped adding up at bar {} (reached {cost})",
            state.layer,
        ),
    }
}

/// A one-line human description of a candidate's typed provenance — built here,
/// in the UI layer, so the model stays a typed value and never a baked string.
/// Names the generator, the ask (Generate) or the source (Swang), and the
/// candidate's rank — enough to tell where a history row came from at a glance.
fn provenance_summary(p: &Provenance) -> String {
    match &p.generator {
        GeneratorProvenance::Generate {
            source,
            seed,
            bars,
            corpus,
            rank,
            strategy,
            ..
        } => {
            let src = source.as_deref().unwrap_or("displayed score");
            let contribution = if corpus.is_seed_only() {
                "seed only".to_owned()
            } else {
                let gesture = if corpus.gesture { ", gesture" } else { "" };
                format!(
                    "corpus {} templates, {} refs{gesture}",
                    corpus.templates, corpus.references
                )
            };
            format!(
                "generate · {strategy} · #{rank} · source {src} · seed {seed} · {bars} bars · {contribution}"
            )
        }
        GeneratorProvenance::GlobalChain {
            source,
            seed,
            bars,
            baseline_cost,
            total_cost,
            suppliers,
            ..
        } => {
            // The suppliers, not a rank: a chain has no rank of its own, and the
            // bar map is the one fact that says what it actually is.
            let map = suppliers
                .iter()
                .map(|s| s.candidate.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "global chain · bars [{map}] · {total_cost:.3} vs intact {baseline_cost:.3} · \
                 source {} · seed {seed} · {bars} bars",
                source.as_deref().unwrap_or("displayed score"),
            )
        }
        GeneratorProvenance::Swang {
            source_path,
            rank,
            strategy,
            ..
        } => {
            let src = source_path.as_deref().unwrap_or("displayed score");
            format!("swang · {strategy} · #{rank} · source {src}")
        }
    }
}

/// A history row's display data, snapshotted so the window closure holds no
/// borrow of the app while it paints.
struct HistoryRow {
    id: HistoryId,
    sequence: u64,
    source: CandidateSource,
    verdict: Option<Verdict>,
    summary: String,
}

/// What the curator clicked on a history row this frame (at most one).
enum HistoryAction {
    /// Audition the row's snapshot.
    Audition(HistoryId),
    /// Toggle the row's favorite verdict.
    Favorite(HistoryId),
    /// Toggle the row's rejected verdict.
    Reject(HistoryId),
}

/// Paints one history row — selection/playing marker, source, verdict (glyph
/// AND word, never colour alone), the provenance line, and the actions — and
/// returns whatever the curator clicked.
fn history_row(
    ui: &mut egui::Ui,
    row: &HistoryRow,
    is_selected: bool,
    playing: bool,
) -> Option<HistoryAction> {
    let mut action = None;
    ui.horizontal(|ui| {
        let marker = if is_selected && playing {
            "▶ playing"
        } else if is_selected {
            "◉ selected"
        } else {
            "  ·"
        };
        ui.monospace(marker);
        let source = match row.source {
            CandidateSource::Generate => "gen",
            CandidateSource::GlobalChain => "chain",
            CandidateSource::Swang => "swang",
        };
        ui.monospace(format!("#{:<3} [{source:>5}]", row.sequence));
        if ui
            .button("audition")
            .on_hover_text("play this one (b to A/B)")
            .clicked()
        {
            action = Some(HistoryAction::Audition(row.id));
        }
        if ui
            .selectable_label(row.verdict == Some(Verdict::Favorite), "★ fav")
            .on_hover_text("favorite (clears reject)")
            .clicked()
        {
            action = Some(HistoryAction::Favorite(row.id));
        }
        if ui
            .selectable_label(row.verdict == Some(Verdict::Rejected), "⊘ rej")
            .on_hover_text("reject (clears favorite)")
            .clicked()
        {
            action = Some(HistoryAction::Reject(row.id));
        }
        // The verdict in words too, for the colour-blind and screen readers.
        ui.label(match row.verdict {
            Some(Verdict::Favorite) => "favorite",
            Some(Verdict::Rejected) => "rejected",
            None => "—",
        });
    });
    ui.weak(&row.summary);
    ui.separator();
    action
}

/// The most loop revolutions one frame may play before the transport takes a
/// bounded hitch and lands at the loop start. Far above any real per-frame lap
/// count (a frame is capped at 0.1 s, a loop is at least a bar), so only a
/// pathological `dt` is bounded — a normal revolution is never dropped.
const MAX_LOOP_WRAPS: usize = 1024;

/// One transport action inside a looped frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LoopStep {
    /// Play forward to this tick, from wherever the cursor sits.
    PlayTo(u32),
    /// Wrap: silence and reposition to this tick (the loop start).
    Wrap(u32),
}

/// Splits a looped frame of `dt` seconds into ordered steps: the tail up to
/// `hi`, then a full `lo..hi` for **every** whole revolution the frame laps,
/// then the remainder to the resume tick. The time to reach `hi` is measured
/// with [`TempoMap::time_to`] and the tempo is re-read at `lo` after each wrap,
/// so no revolution is silently dropped and the wrapped part never runs at the
/// tempo past `hi`. A head outside `[lo, hi)` wraps in first; a pathological
/// `dt` is bounded by [`MAX_LOOP_WRAPS`]. Returns the steps and the fractional
/// resume position. Pure, so the split is unit-tested against exact spans.
#[allow(clippy::too_many_arguments)] // the loop bounds + the tempo context are irreducible here
fn plan_loop(
    pos: f64,
    dt: f64,
    lo: u32,
    hi: u32,
    tempo: &TempoMap,
    ppq: u16,
    scale: f64,
) -> (Vec<LoopStep>, f64) {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // pos ≥ 0, ticks never near u32::MAX
    const fn floor(pos: f64) -> u32 {
        pos as u32
    }
    let lo_f = f64::from(lo);
    let hi_f = f64::from(hi);
    let mut steps = Vec::new();
    let mut remaining = dt.max(0.0);
    let mut pos = pos.max(0.0);
    // A head outside the loop wraps to the start before any time is spent.
    if pos < lo_f || pos >= hi_f {
        steps.push(LoopStep::Wrap(lo));
        pos = lo_f;
    }
    for _ in 0..MAX_LOOP_WRAPS {
        let dt_to_hi = tempo.time_to(pos, hi_f, ppq, scale);
        if remaining < dt_to_hi {
            // The frame ends inside the loop: play the partial span.
            let next = tempo.advance(pos, remaining, ppq, scale);
            steps.push(LoopStep::PlayTo(floor(next)));
            return (steps, next);
        }
        // Reach the loop end: play the tail up to `hi`, then wrap to `lo` and
        // re-read the tempo there on the next turn.
        steps.push(LoopStep::PlayTo(hi));
        steps.push(LoopStep::Wrap(lo));
        remaining -= dt_to_hi;
        pos = lo_f;
        if dt_to_hi <= 0.0 {
            break; // a zero-length span would otherwise spin
        }
    }
    // Bounded hitch: a pathological dt settles at the loop start.
    (steps, lo_f)
}

/// Each track's display name (`track N` when unnamed), in order — the labels for
/// the toolbar's track selector.
fn track_labels(score: &Score) -> Vec<String> {
    score
        .tracks
        .iter()
        .enumerate()
        .map(|(i, track)| {
            track
                .name
                .clone()
                .unwrap_or_else(|| format!("track {}", i + 1))
        })
        .collect()
}

impl CockpitApp {
    /// Builds the app from a view and its analysis; `title` labels the window.
    #[must_use]
    pub fn new(view: PianoRollView, analysis: Analysis, title: String) -> Self {
        let ctx = build_context(&view, &analysis);
        let view_tempo_bpm = view.tempo_bpm;
        let mut vp = Viewport::new(&ctx, view.high_pitch);
        vp.show_inspector = false; // the capture panel starts hidden (the `i` key shows it)
        let player = Player::from_view(&view);
        Self {
            view,
            analysis,
            title,
            vp,
            ctx,
            fitted: false,
            score: None,
            form: CaptureForm::default(),
            corpus: Vec::new(),
            corpus_filter: CorpusFilter::default(),
            show_dock: false,
            selected: None,
            rename_buf: String::new(),
            dock_status: None,
            selected_track: 0,
            track_names: Vec::new(),
            gen_panel: GeneratePanel::new(),
            swang: SwangPanel::default(),
            synth: Synth::new(),
            player,
            tempo_scale: 1.0,
            tempo_map: TempoMap::single(view_tempo_bpm),
            play_pos: 0.0,
            loop_range: None,
            current: None,
            ab_other: None,
            history: SessionHistory::new(),
            swang_ctx: None,
            history_open: false,
            last_play_tick: 0,
            material: None,
            #[cfg(not(target_arch = "wasm32"))]
            out_dir: PathBuf::from("keeps"),
            theme: Theme::dark(),
        }
    }

    /// Builds the app over an imported `score`, **keeping it** behind the view so
    /// Capture works on the displayed file (ADR-0026). Use this for every real
    /// score — the CLI entry and the web demo — so the initially-shown score is
    /// capturable without first re-loading it; `title` labels the window.
    #[must_use]
    pub fn from_score(score: Score, title: String) -> Self {
        let analysis = analyze(&score);
        let focus = analysis.focus_track;
        let mut app = Self::new(build_view(&score), analysis, title);
        app.track_names = track_labels(&score);
        app.score = Some(score);
        app.focus_on_track(focus); // show just the auto-picked track, not all overlaid
        app
    }

    /// Shows track `track` alone: rebuilds the view, sections, and viewport from
    /// its single-track sub-score and re-fits, so the roll stops overlaying every
    /// part. Capture then targets this track. A no-op without a loaded score or
    /// for an out-of-range index.
    fn focus_on_track(&mut self, track: usize) {
        let Some(score) = self.score.as_ref() else {
            return;
        };
        let n = score.tracks.len();
        // An out-of-range track on a non-empty score is a no-op (keep the view).
        // A track-less score (a valid MIDI with no note-bearing tracks) still
        // gets its empty plane built below, so the load isn't silently dropped.
        if track >= n && n != 0 {
            return;
        }
        // Capture the loop as bar indices on the OLD view before it is rebuilt,
        // so the switch can re-anchor it to the same bars of the NEW view.
        let prior_loop_bars = self.loop_range.map(|_| self.loop_bar_indices());
        let sub = single_track_score(score, track);
        let view = build_view(&sub);
        let analysis = analyze(&sub);
        let ctx = build_context(&view, &analysis);
        let mut vp = Viewport::new(&ctx, view.high_pitch);
        vp.show_inspector = self.vp.show_inspector; // keep the panel state across a switch
                                                    // Keep the playhead across a candidate switch, so A/B compares the same
                                                    // spot; a fresh file resets it explicitly (see `load`).
        vp.play_tick = self.vp.play_tick.min(ctx.tick_end);
        vp.playing = self.vp.playing;
        self.view = view;
        self.analysis = analysis;
        self.ctx = ctx;
        self.vp = vp;
        self.selected_track = track.min(n.saturating_sub(1));
        self.fitted = false;
        // Re-anchor the loop to the same bars of the new view — clamped inside
        // it, or cleared when those bars are gone — so a shorter score can never
        // leave playback grazing a range past its end (the master timeline is
        // the source of the geometry, not a stale absolute span).
        if let Some((from, to)) = prior_loop_bars {
            self.loop_range = remap_loop_range(from, to, &self.view.bar_lines, self.ctx.tick_end);
            // If the head fell outside the remapped loop, seek it to the start.
            if let Some((lo, hi)) = self.loop_range {
                if self.vp.play_tick < lo || self.vp.play_tick >= hi {
                    self.vp.play_tick = lo;
                }
            }
        }
        // Rebuild the note schedule for the new score and reposition it under
        // the playhead — silencing whatever the old score left ringing.
        self.player = Player::from_view(&self.view);
        self.player.seek(self.vp.play_tick, &mut self.synth);
        // Adopt this score's master-timeline tempo, and re-anchor the
        // fractional playhead on the (already-clamped) integer one.
        self.tempo_map = if sub.master_bars.is_empty() {
            TempoMap::single(self.view.tempo_bpm)
        } else {
            tempo_map_of(&sub)
        };
        self.play_pos = f64::from(self.vp.play_tick);
    }

    /// Advances the playhead by `dt` seconds at the auditioned tempo, firing
    /// the note events the head crossed. Loops through the selected range
    /// (playing every whole revolution a long frame laps), else stops and
    /// silences at the end.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // play_pos ≥ 0, ticks never near u32::MAX
    fn advance_audio(&mut self, dt: f64) {
        // If the integer playhead was moved out from under us (a seek, a
        // candidate switch, a Stop), adopt it before accumulating — `play_pos`
        // is authoritative only while it still floors to the visible head.
        if self.vp.play_tick != self.play_pos as u32 {
            self.play_pos = f64::from(self.vp.play_tick);
        }
        match self.loop_range {
            Some((lo, hi)) => self.advance_looped(dt, lo, hi),
            None => self.advance_to_end(dt),
        }
    }

    /// The non-looping path: advance along the tempo map, and at the score's
    /// end play the tail, silence, and stop.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // play_pos ≥ 0, ticks never near u32::MAX
    fn advance_to_end(&mut self, dt: f64) {
        let pos = self
            .tempo_map
            .advance(self.play_pos, dt, self.ctx.ppq, self.tempo_scale);
        let end = self.ctx.tick_end;
        if (pos as u32) < end {
            self.play_pos = pos;
            self.vp.play_tick = pos as u32;
            self.player.advance_to(pos as u32, &mut self.synth);
        } else {
            self.player.advance_to(end, &mut self.synth);
            self.player.silence(&mut self.synth);
            self.play_pos = f64::from(end);
            self.vp.play_tick = end;
            self.vp.playing = false;
        }
    }

    /// The looping path: [`plan_loop`] splits the frame into tail / full
    /// revolutions / remainder, and the player executes each step. Every whole
    /// revolution a long frame laps is played, and each wrap re-reads the tempo
    /// at the loop start.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // play_pos ≥ 0, ticks never near u32::MAX
    fn advance_looped(&mut self, dt: f64, lo: u32, hi: u32) {
        let (steps, resume) = plan_loop(
            self.play_pos,
            dt,
            lo,
            hi,
            &self.tempo_map,
            self.ctx.ppq,
            self.tempo_scale,
        );
        for step in steps {
            match step {
                LoopStep::PlayTo(t) => self.player.advance_to(t, &mut self.synth),
                LoopStep::Wrap(l) => self.player.seek(l, &mut self.synth),
            }
        }
        self.play_pos = resume;
        self.vp.play_tick = resume as u32;
    }

    /// Stops playback: playhead to the start, nothing sounding. The audition
    /// setup — tempo, loop, A/B — is **kept**, so stopping to re-listen or
    /// compare does not undo it (that is [`Self::reset_audition`]'s job, on a
    /// fresh file or a new generation).
    fn stop_playback(&mut self) {
        self.vp.playing = false;
        self.vp.play_tick = self.ctx.tick_start;
        self.play_pos = f64::from(self.ctx.tick_start);
        self.player.seek(self.vp.play_tick, &mut self.synth);
    }

    /// Clears the audition setup back to defaults — the written tempo, no
    /// loop, no A/B. A fresh file and a new generation session get this;
    /// Stop does not.
    const fn reset_audition(&mut self) {
        self.tempo_scale = 1.0;
        self.loop_range = None;
        self.current = None;
        self.ab_other = None;
    }

    /// Records that `shown` is now painted: the candidate we are leaving (of
    /// either source) becomes the A/B target, so `b` swaps back to the last one
    /// viewed regardless of which generator produced it. A no-op re-show keeps
    /// the existing A/B target.
    fn remember_shown(&mut self, shown: AuditionCandidate) {
        if let Some(current) = self.current {
            if current != shown {
                self.ab_other = Some(current);
            }
        }
        self.current = Some(shown);
    }

    /// The source label shown in the window title.
    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Replaces the displayed score by importing `bytes` through the shared
    /// importer (the same parser as the CLI — GP and MIDI alike). `source`
    /// labels the new score; the viewport re-fits to it on the next paint.
    ///
    /// # Errors
    /// Returns a message if `bytes` are not an importable MIDI/Guitar Pro file.
    pub fn load(&mut self, source: String, bytes: &[u8]) -> Result<(), String> {
        let score =
            import_score_auto(bytes).map_err(|err| format!("cannot import {source}: {err}"))?;
        let focus = analyze(&score).focus_track;
        self.track_names = track_labels(&score);
        self.form.seed_from(&source);
        self.title = source;
        self.vp.show_inspector = false; // a fresh load hides the capture panel
        self.score = Some(score);
        self.focus_on_track(focus); // rebuilds view/analysis/ctx/vp for the focus track
        self.stop_playback(); // a new file plays from its start
        self.reset_audition(); // and at its own tempo, no stale loop or A/B
        self.history.clear_selection(); // no history row is active on a fresh file
        Ok(())
    }

    /// Shows an already-built `score` under `title` — the path a freshly
    /// generated candidate takes, with no export/re-import round-trip. Unlike
    /// [`Self::load`] it leaves the capture form alone: the form still describes
    /// the curator's chunk, not the machine's candidate.
    pub fn show_score(&mut self, score: Score, title: String) {
        let focus = analyze(&score).focus_track;
        self.track_names = track_labels(&score);
        self.title = title;
        self.score = Some(score);
        self.focus_on_track(focus);
    }

    /// Attaches a corpus to the Generate panel: its material (rhythm templates,
    /// novelty references, gesture ask) and its source tabs as the seed
    /// pick-list. Without one, a pass seeds from the displayed score alone.
    pub fn attach_corpus(&mut self, material: CorpusMaterial, sources: Vec<generation::SourceTab>) {
        self.material = Some(material);
        self.gen_panel.source = if sources.is_empty() { None } else { Some(0) };
        self.gen_panel.sources = sources;
        self.gen_panel.open = true;
    }

    /// Where kept candidates are written.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn set_out_dir(&mut self, dir: PathBuf) {
        self.out_dir = dir;
    }

    /// The score a pass seeds from: the picked corpus tab, or — when none is
    /// picked (or the pick is stale) — the displayed score.
    fn generation_source(&self) -> Result<Score, String> {
        self.gen_panel.source_tab().map_or_else(
            || {
                self.score
                    .clone()
                    .ok_or_else(|| "no score loaded".to_owned())
            },
            |tab| {
                import_score_auto(&tab.bytes)
                    .map_err(|err| format!("cannot import {}: {err}", tab.name))
            },
        )
    }

    /// Runs the panel's ask through the shared compiler and shows the winner.
    ///
    /// The set is `griff generate`'s set: same entry point, same rerank, same
    /// order — rank 1 is the candidate the CLI would have written.
    fn do_generate(&mut self) {
        let ask = self.gen_panel.ask();
        // One pass: the ranked set and the chain planned from it, together. The
        // `RankedSet` lives and dies inside `generate_run`, so what comes back
        // is already a pair of snapshots.
        let outcome = self.generation_source().and_then(|score| {
            generate_run(&score, self.material.as_ref(), &ask).map_err(|err| format!("{err:?}"))
        });
        match outcome {
            Ok(GeneratedRun { set, chain }) => {
                self.reset_audition(); // a new candidate set starts fresh (no stale A/B)
                                       // Capture the run's request/input identity NOW, from the state
                                       // that produced the set — the source seed, the ask, and the
                                       // corpus's actual contribution — so no later knob change can
                                       // rewrite a candidate's origin.
                let context = GenerateRunContext {
                    run: self.history.begin_run(),
                    source: Some(
                        self.gen_panel
                            .source_tab()
                            .map_or_else(|| self.title.clone(), |tab| tab.name.clone()),
                    ),
                    seed: ask.seed,
                    bars: ask.bars,
                    variants_per_strategy: ask.variants_per_strategy,
                    corpus: CorpusContribution::from_pass(
                        self.material.as_ref().map_or(0, |m| m.rhythms.len()),
                        &set.summary,
                    ),
                };
                let n = set.rows.len();
                let tones = set.summary.scale_tones;
                // Against the run, once, whether or not the chain is ever
                // auditioned — so "did this run have a chain, and if not why"
                // outlives the run being replaced.
                self.history
                    .record_chain(context.run, chain_outcome_record(&chain));
                self.gen_panel.active = Some(ActiveGenerateRun {
                    context,
                    set,
                    chain,
                });
                self.gen_panel.status = Some(format!(
                    "{n} candidates ranked · {tones}-tone scale · seed {}",
                    ask.seed
                ));
                self.show_candidate(0);
            }
            Err(err) => {
                self.gen_panel.active = None;
                self.gen_panel.selected = None;
                self.gen_panel.status = Some(format!("generate failed: {err}"));
            }
        }
    }

    /// Paints the run's assembled S7 global chain into the roll, recording it in
    /// history as its own audition result.
    ///
    /// The chain is a **snapshot**: this reads the one planned when the set was
    /// produced and plans nothing. A refused chain shows nothing rather than
    /// substituting the intact winner — the S6 result is a different result, and
    /// handing it over under the chain's name would be the one lie this whole
    /// comparison exists to avoid.
    fn show_global_chain(&mut self) {
        let Some(active) = self.gen_panel.active.as_ref() else {
            return;
        };
        let GlobalChainOutcome::Planned(chain) = &active.chain else {
            return;
        };
        let ctx = &active.context;
        let score = chain.plan.score.clone();
        let title = chain_title(chain);
        let generator = chain_provenance(ctx, chain);
        let id = self.history.record(
            ctx.run,
            CHAIN_CANDIDATE_ID.to_owned(),
            title.clone(),
            score.clone(),
            generator,
        );
        self.history.select(id);
        self.remember_shown(AuditionCandidate::GlobalChain);
        // No row is the audition now. The table's highlight is a claim about
        // what is sounding, and the chain is bars from several candidates —
        // leaving a row marked would name one supplier as the whole result.
        self.gen_panel.selected = None;
        self.show_score(score, title);
    }

    /// Paints candidate `i` of the current set into the roll, recording it in
    /// the session history with its Generate provenance.
    ///
    /// The request/input fields come only from the run's immutable
    /// [`GenerateRunContext`] — never from live panel state — so re-showing a
    /// row (or showing one after a knob change) cannot rewrite its origin. Only
    /// the candidate-specific result (strategy, variant seed, rank, aggregate)
    /// is read from the row.
    fn show_candidate(&mut self, i: usize) {
        let Some(active) = self.gen_panel.active.as_ref() else {
            return;
        };
        let set = &active.set;
        let ctx = &active.context;
        let (Some(score), Some(row)) = (set.scores.get(i).cloned(), set.rows.get(i)) else {
            return;
        };
        let title = format!("#{} {} · {:.3}", row.rank, row.strategy, row.aggregate);
        let candidate_id = row.id.clone();
        let generator = GeneratorProvenance::Generate {
            source: ctx.source.clone(),
            corpus: ctx.corpus,
            seed: ctx.seed,
            bars: ctx.bars,
            variants_per_strategy: ctx.variants_per_strategy,
            strategy: row.strategy.clone(),
            variant_seed: row.variant_seed,
            rank: row.rank,
            aggregate: row.aggregate,
        };
        let run = ctx.run;
        let id = self
            .history
            .record(run, candidate_id, title.clone(), score.clone(), generator);
        self.history.select(id);
        // Remember the candidate we are leaving (of either source) so `b` can
        // A/B back to it without a fresh generation pass.
        self.remember_shown(AuditionCandidate::Generate(i));
        self.gen_panel.selected = Some(i);
        self.show_score(score, title);
    }

    /// A/B: swaps to the other of the last two candidates viewed — routing to
    /// the correct source, so a Swang candidate after a Generate one swaps back
    /// to the Swang set (and a history entry back to its snapshot), never a
    /// same-index row of the wrong set. Keeps the playhead so the comparison
    /// lands at the same spot. No regeneration.
    fn ab_swap(&mut self) {
        match self.ab_other {
            Some(AuditionCandidate::Generate(i)) => self.show_candidate(i),
            Some(AuditionCandidate::GlobalChain) => self.show_global_chain(),
            Some(AuditionCandidate::Swang(i)) => self.swang_show(i),
            Some(AuditionCandidate::History(id)) => self.select_history(id),
            None => {}
        }
    }

    /// Replays a recorded history candidate by its stable id: switches the roll
    /// to its immutable snapshot through the same safe path as any score change
    /// (`show_score` → `focus_on_track`: All Notes Off, loop remap, tempo, and
    /// playhead all handled), and marks it selected. A no-op for an unknown id.
    fn select_history(&mut self, id: HistoryId) {
        let Some(entry) = self.history.get(id) else {
            return;
        };
        let score = entry.score.clone();
        let title = entry.title.clone();
        self.history.select(id);
        self.remember_shown(AuditionCandidate::History(id));
        // A snapshot is sounding, so no *live* row is. The replayed entry may
        // even come from a run whose panel set is long gone — leaving a row of
        // the current set marked would point at music that is not playing.
        self.gen_panel.selected = None;
        self.swang.selected = None;
        self.show_score(score, title);
    }

    /// The session history window (S8 Slice 3): a newest-first feed of every
    /// candidate shown, each with its source, verdict, and a one-line
    /// provenance, plus audition / favorite / reject actions. State is
    /// distinguished by text and glyph, never colour alone.
    fn history_window(&mut self, ctx: &egui::Context) {
        // Snapshot what the list needs, so the closure holds no borrow of self
        // and the actions apply cleanly afterwards.
        let selected = self.history.selected();
        let playing = self.vp.playing;
        let rows: Vec<HistoryRow> = self
            .history
            .entries()
            .iter()
            .rev()
            .map(|e| HistoryRow {
                id: e.id,
                sequence: e.sequence,
                source: e.source,
                verdict: e.verdict,
                summary: provenance_summary(&e.provenance),
            })
            .collect();

        let mut action: Option<HistoryAction> = None;
        egui::Window::new("history · session")
            .default_width(440.0)
            .default_height(420.0)
            .show(ctx, |ui| {
                if rows.is_empty() {
                    ui.weak("no candidates yet — generate (g) or run a Swang program (e)");
                    return;
                }
                ui.weak(format!(
                    "{} candidate(s) this session — newest first",
                    rows.len()
                ));
                ui.separator();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for row in &rows {
                        let hit = history_row(ui, row, selected == Some(row.id), playing);
                        if action.is_none() {
                            action = hit;
                        }
                    }
                });
            });

        match action {
            Some(HistoryAction::Favorite(id)) => self.history.set_verdict(id, Verdict::Favorite),
            Some(HistoryAction::Reject(id)) => self.history.set_verdict(id, Verdict::Rejected),
            Some(HistoryAction::Audition(id)) => self.select_history(id),
            None => {}
        }
    }

    /// Writes candidate `i` as a `.mid` plus a provenance sidecar naming the
    /// exact ask that reproduces it. Native only — the browser has no
    /// filesystem (a web keep would download, out of this slice's scope).
    #[cfg(not(target_arch = "wasm32"))]
    fn keep_candidate(&mut self, i: usize) {
        let outcome = self.write_keep(i);
        self.gen_panel.status = Some(match outcome {
            Ok(path) => format!("kept {path}"),
            Err(err) => format!("keep failed: {err}"),
        });
    }

    /// Exports candidate `i` and its provenance into the out dir; returns the
    /// written `.mid` path.
    #[cfg(not(target_arch = "wasm32"))]
    fn write_keep(&self, i: usize) -> Result<String, String> {
        use std::fs;

        use griff_core::midi::export_score;

        // The exported score and its provenance come from the SAME captured run
        // — never from live panel state, which may have moved on since.
        let active = self
            .gen_panel
            .active
            .as_ref()
            .ok_or("nothing generated yet")?;
        let set = &active.set;
        let context = &active.context;
        let row = set.rows.get(i).ok_or("no such candidate")?;
        let score = set.scores.get(i).ok_or("no such candidate")?;
        let provenance = kept_provenance(context, row);

        fs::create_dir_all(&self.out_dir)
            .map_err(|e| format!("cannot create {}: {e}", self.out_dir.display()))?;
        let stem = format!(
            "seed{}_{}_{:016x}",
            context.seed, row.strategy, row.variant_seed
        );
        let mid = self.out_dir.join(format!("{stem}.mid"));
        let json = self.out_dir.join(format!("{stem}.json"));

        let bytes = export_score(score).map_err(|err| format!("{err:?}"))?;
        fs::write(&mid, &bytes).map_err(|e| format!("cannot write {}: {e}", mid.display()))?;
        let text = serde_json::to_string_pretty(&provenance).map_err(|err| err.to_string())?;
        fs::write(&json, text).map_err(|e| format!("cannot write {}: {e}", json.display()))?;
        Ok(mid.display().to_string())
    }

    /// Writes the run's assembled global chain, recording it first if it has not
    /// been auditioned yet — so what is written is a result that exists in the
    /// history, not a fourth thing built at the moment of export.
    #[cfg(not(target_arch = "wasm32"))]
    fn keep_chain(&mut self) {
        let Some(id) = self.ensure_chain_history_entry() else {
            return;
        };
        self.gen_panel.status = Some(match self.write_chain_keep(id) {
            Ok(path) => format!("kept -> {path}"),
            Err(err) => format!("keep failed: {err}"),
        });
    }

    /// The history entry for the active run's chain, recording it first if it
    /// has not been auditioned yet.
    ///
    /// Deliberately *not* `show_global_chain`: exporting is a file action, and
    /// it must not decide what the user is listening to. Keeping while the S6
    /// winner plays writes the chain and leaves the S6 winner playing.
    ///
    /// `record` is keyed by `(run, candidate_id)` and a run has exactly one
    /// chain, so this returns the audition's entry when there is one and mints
    /// the same entry when there is not — the export and the history cannot
    /// disagree about which result was written.
    #[cfg(not(target_arch = "wasm32"))]
    fn ensure_chain_history_entry(&mut self) -> Option<HistoryId> {
        let active = self.gen_panel.active.as_ref()?;
        let GlobalChainOutcome::Planned(chain) = &active.chain else {
            return None;
        };
        let ctx = &active.context;
        let generator = chain_provenance(ctx, chain);
        let title = chain_title(chain);
        Some(self.history.record(
            ctx.run,
            CHAIN_CANDIDATE_ID.to_owned(),
            title,
            chain.plan.score.clone(),
            generator,
        ))
    }

    /// Exports the global chain recorded as history entry `id` to `.mid`, plus
    /// a provenance sidecar; returns the written `.mid` path.
    ///
    /// Exports the **snapshot**, through the one canonical `Score` → MIDI path.
    /// The planner is not consulted: what was auditioned is what is written,
    /// however far the panel has moved on since.
    ///
    /// # Errors
    /// When the entry is missing, is not a chain, or the write fails.
    #[cfg(not(target_arch = "wasm32"))]
    fn write_chain_keep(&self, id: HistoryId) -> Result<String, String> {
        use std::fs;

        use griff_core::midi::export_score;

        // Everything below comes from the entry — the score and the provenance
        // that describes it, recorded together and immutable since. The active
        // run is not consulted: it may be a different run entirely by now.
        let entry = self.history.get(id).ok_or("no such history entry")?;
        let GeneratorProvenance::GlobalChain {
            seed,
            bars,
            variants_per_strategy,
            corpus,
            policy_id,
            policy_version,
            baseline_cost,
            total_cost,
            suppliers,
            source,
        } = &entry.provenance.generator
        else {
            return Err("that entry is not a global chain".to_owned());
        };
        let sidecar = KeptChain {
            origin: CHAIN_SIDECAR_ORIGIN,
            run: entry.run.0,
            source: source.as_deref(),
            corpus: KeptCorpus {
                templates: corpus.templates,
                references: corpus.references,
                gesture: corpus.gesture,
            },
            seed: *seed,
            // The ask's own number. `suppliers.len()` agrees, and a law says so,
            // but the ask is what the run recorded and the result is what it
            // produced — evidence does not get to derive itself.
            bars: *bars,
            variants_per_strategy: *variants_per_strategy,
            policy_id,
            policy_version: *policy_version,
            baseline_cost: *baseline_cost,
            total_cost: *total_cost,
            suppliers: suppliers
                .iter()
                .map(|s| KeptSupplier {
                    bar: s.bar,
                    candidate: s.candidate,
                    rank: s.rank,
                    strategy: &s.strategy,
                    variant_seed: s.variant_seed,
                })
                .collect(),
        };

        fs::create_dir_all(&self.out_dir)
            .map_err(|e| format!("cannot create {}: {e}", self.out_dir.display()))?;
        let stem = format!("seed{seed}_global-chain_run{}", entry.run.0);
        let mid = self.out_dir.join(format!("{stem}.mid"));
        let json = self.out_dir.join(format!("{stem}.json"));

        let bytes = export_score(&entry.score).map_err(|err| format!("{err:?}"))?;
        fs::write(&mid, &bytes).map_err(|e| format!("cannot write {}: {e}", mid.display()))?;
        let text = serde_json::to_string_pretty(&sidecar).map_err(|err| err.to_string())?;
        fs::write(&json, text).map_err(|e| format!("cannot write {}: {e}", json.display()))?;
        Ok(mid.display().to_string())
    }

    /// Hands the last kept `.mid` to the OS default handler — the cheap way to
    /// *hear* a candidate (a notation editor or DAW is already registered for
    /// `.mid`). The cockpit does not synthesise audio.
    #[cfg(not(target_arch = "wasm32"))]
    fn open_keep(&mut self, i: usize) {
        let outcome = self
            .write_keep(i)
            .and_then(|path| open_in_default_app(Path::new(&path)).map(|()| path));
        self.gen_panel.status = Some(match outcome {
            Ok(path) => format!("opened {path}"),
            Err(err) => format!("open failed: {err}"),
        });
    }

    /// The Generate panel (S8): the ask, the ranked candidate table, and the
    /// keep actions. Rank 1 is what `griff generate` would have written.
    fn generate_window(&mut self, ctx: &egui::Context) {
        let mut acts = GenerateActions::default();
        egui::Window::new("generate · candidates")
            .default_width(460.0)
            .default_height(420.0)
            .show(ctx, |ui| {
                if !self.gen_panel.sources.is_empty() {
                    let current = self
                        .gen_panel
                        .source_tab()
                        .map_or("(displayed score)", |t| t.name.as_str())
                        .to_owned();
                    egui::ComboBox::from_label("seed tab")
                        .selected_text(current)
                        .width(300.0)
                        .show_ui(ui, |ui| {
                            for (i, tab) in self.gen_panel.sources.iter().enumerate() {
                                if ui
                                    .selectable_label(self.gen_panel.source == Some(i), &tab.name)
                                    .clicked()
                                {
                                    self.gen_panel.source = Some(i);
                                }
                            }
                        });
                }
                ui.horizontal(|ui| {
                    ui.label("seed");
                    ui.add(egui::DragValue::new(&mut self.gen_panel.seed).speed(1.0));
                    ui.label("bars");
                    ui.add(egui::DragValue::new(&mut self.gen_panel.bars).range(1..=64));
                    ui.label("variants");
                    ui.add(egui::DragValue::new(&mut self.gen_panel.variants).range(1..=40))
                        .on_hover_text("per strategy — the set holds this x 5");
                    ui.checkbox(&mut self.gen_panel.gesture, "gesture")
                        .on_hover_text("carve the corpus's burst/rest phrasing");
                });
                ui.horizontal(|ui| {
                    if ui.button("⚙ generate").clicked() {
                        acts.run = true;
                    }
                    if ui.button("🎲 next seed").clicked() {
                        self.gen_panel.seed = self.gen_panel.seed.wrapping_add(1);
                        acts.run = true;
                    }
                    if let Some(status) = &self.gen_panel.status {
                        ui.weak(status);
                    }
                });
                ui.weak(if self.material.is_some() {
                    "mode: corpus rhythms + novelty + gesture"
                } else {
                    "mode: seed only — no corpus (run with --corpus for rhythms)"
                });

                self.generate_candidates(ui, &mut acts);
            });
        self.apply_generate_actions(&acts);
    }

    /// Acts on what the Generate window asked for, once it is no longer holding
    /// a borrow of the panel it asked about.
    fn apply_generate_actions(&mut self, acts: &GenerateActions) {
        if acts.run {
            self.do_generate();
        }
        match acts.show {
            Some(AuditionPick::Candidate(i)) => self.show_candidate(i),
            // The S6 winner is ranked candidate 0, always — the candidate the
            // chain's baseline cost is the cost of.
            Some(AuditionPick::Intact) => self.show_candidate(0),
            Some(AuditionPick::Chain) => self.show_global_chain(),
            None => {}
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            if let Some(i) = acts.keep {
                self.keep_candidate(i);
            }
            if acts.keep_chain {
                self.keep_chain();
            }
            if let Some(i) = acts.open {
                self.open_keep(i);
            }
        }
        #[cfg(target_arch = "wasm32")]
        if acts.keep.is_some() || acts.open.is_some() {
            self.gen_panel.status = Some("keep is native-only in this slice".to_owned());
        }
    }

    /// The Swang editor (S8 Playground): program text, span diagnostics, an
    /// explicit Run, the ranked candidates, and Build. It drives the one
    /// shared evaluator — no `griff.exe`, no second generator — and never
    /// regenerates on a keystroke; Run is a button (or Ctrl+Enter).
    fn swang_window(&mut self, ctx: &egui::Context) {
        let mut actions = SwangActions::default();
        egui::Window::new("swang · editor")
            .default_width(540.0)
            .default_height(560.0)
            .show(ctx, |ui| {
                actions = self.swang_body(ui);
            });

        // Apply after the window releases its borrow of `self`.
        match actions.action {
            SwangAction::Check => self.swang.check(),
            SwangAction::Format => self.swang.format(),
            SwangAction::Run => self.swang_run(),
            SwangAction::Build => self.swang_build(),
            SwangAction::None => {}
        }
        if let Some(i) = actions.click {
            self.swang_show(i);
        }
    }

    /// The Swang window's body: the button row, the editor, the diagnostics,
    /// and the candidate table. Returns the actions the user asked for, so
    /// `swang_window` applies them after the borrow closes.
    fn swang_body(&mut self, ui: &mut egui::Ui) -> SwangActions {
        let mut actions = SwangActions::default();
        ui.horizontal(|ui| {
            if ui
                .button("✓ check")
                .on_hover_text("parse + validate")
                .clicked()
            {
                actions.action = SwangAction::Check;
            }
            if ui
                .button("⤷ format")
                .on_hover_text("canonical text")
                .clicked()
            {
                actions.action = SwangAction::Format;
            }
            if ui
                .button("▶ run")
                .on_hover_text("evaluate (Ctrl+Enter)")
                .clicked()
            {
                actions.action = SwangAction::Run;
            }
            if self.swang.run_is_current()
                && ui
                    .button("⏏ build")
                    .on_hover_text("write the program's own export")
                    .clicked()
            {
                actions.action = SwangAction::Build;
            }
            if !self.swang.status.is_empty() {
                ui.weak(&self.swang.status);
            }
        });

        let editor = egui::ScrollArea::vertical()
            .id_salt("swang-editor")
            .max_height(240.0)
            .show(ui, |ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut self.swang.text)
                        .code_editor()
                        .desired_rows(14)
                        .desired_width(f32::INFINITY),
                )
            })
            .inner;
        // An edit invalidates the last run: the shown candidates and the
        // Build button must not outlive the text they came from.
        if editor.changed() {
            self.swang.invalidate_run();
        }
        // Run on Ctrl+Enter, but only while the editor actually holds focus —
        // a stray shortcut must not fire a generation pass.
        if editor.has_focus() && ui.input(|i| i.key_pressed(Key::Enter) && i.modifiers.command) {
            actions.action = SwangAction::Run;
        }

        if !self.swang.diagnostics.is_empty() {
            ui.separator();
            for d in &self.swang.diagnostics {
                ui.label(format!("[{}] {}: {}", d.code, d.location, d.message));
            }
        }

        ui.weak("mode: Swang explicit rhythm (the ascii kernel) — a corpus would add novelty/gesture, never the grid");
        self.swang_candidates(ui, &mut actions.click);
        actions
    }

    /// The Swang panel's candidate table: the set's provenance line and the
    /// ranked rows, each with its rerank axes on hover. A click is reported
    /// through `click`, applied after the window releases its borrow.
    fn swang_candidates(&self, ui: &mut egui::Ui, click: &mut Option<usize>) {
        let Some(set) = self.swang.set.as_ref() else {
            return;
        };
        ui.separator();
        ui.weak(format!(
            "{} candidates · {} templates · {}-tone scale",
            set.rows.len(),
            set.summary.templates,
            set.summary.scale_tones,
        ));
        egui::ScrollArea::vertical()
            .id_salt("swang-candidates")
            .max_height(180.0)
            .show(ui, |ui| {
                for (i, row) in set.rows.iter().enumerate() {
                    let label = format!(
                        "#{:<2} {:<24} {:.3}  ·  {} notes",
                        row.rank, row.strategy, row.aggregate, row.note_count
                    );
                    let hover = row
                        .axes
                        .iter()
                        .map(|&(name, v)| format!("{name}: {v:.3}"))
                        .collect::<Vec<_>>()
                        .join("\n");
                    if ui
                        .selectable_label(self.swang.selected == Some(i), label)
                        .on_hover_text(hover)
                        .clicked()
                    {
                        *click = Some(i);
                    }
                }
            });
    }

    /// Compiles the program (real span diagnostics on error), resolves its
    /// declared seed score, evaluates, and shows the selection. Compile comes
    /// first so a broken program keeps its diagnostics instead of a generic
    /// message; the evaluator only ever sees a loaded score, never a path.
    fn swang_run(&mut self) {
        let Some(compiled) = self.swang.compile() else {
            return; // `compile` filled the span diagnostics and cleared the run
        };
        let (source, origin) = match self.resolve_swang_source(&compiled) {
            Ok(resolved) => resolved,
            Err(err) => {
                self.swang.invalidate_run();
                self.swang.diagnostics.clear();
                self.swang.status = err;
                return;
            }
        };
        // Slice 1 loads no corpus; `apply` refuses a program that declares one
        // rather than silently running without it.
        self.swang.apply(&compiled, source, None);
        if let Some(i) = self.swang.selected {
            self.reset_audition(); // a new Swang generation session starts A/B fresh
                                   // Capture the run's identity from the program just evaluated — its
                                   // exact text and declared source — so later edits cannot rewrite a
                                   // recorded candidate's provenance.
            self.swang_ctx = Some(SwangRunContext {
                run: self.history.begin_run(),
                program: self.swang.text.clone(),
                // What the frontend ACTUALLY resolved — a path on native, the
                // displayed score (and so no path) in the browser.
                source_path: origin.provenance_path(),
            });
            self.swang_show(i);
        }
    }

    /// The seed score a compiled program names, resolved to a `Score`. Native
    /// reads the declared path and **errors** if it cannot — a missing or
    /// mistyped `source` is never silently swapped for the displayed file, so
    /// a program's provenance always matches the music it made. The web target
    /// has no filesystem and uses the displayed score by defined semantics.
    #[cfg(not(target_arch = "wasm32"))]
    #[allow(
        clippy::unused_self,
        reason = "mirrors the wasm signature, which reads self.score"
    )]
    fn resolve_swang_source(
        &self,
        compiled: &eval::CompiledProgram,
    ) -> Result<(Score, SwangSourceOrigin), String> {
        use std::fs;

        let path = compiled.source_path();
        let bytes = fs::read(path).map_err(|e| format!("cannot read source \"{path}\": {e}"))?;
        let score =
            import_score_auto(&bytes).map_err(|e| format!("cannot import \"{path}\": {e}"))?;
        // The path really was read — provenance may name it.
        Ok((score, SwangSourceOrigin::ResolvedPath(path.to_owned())))
    }

    /// The web target has no filesystem: the displayed score seeds the run by
    /// defined frontend semantics (the declared `source` path is not read in
    /// the browser this slice) — so the origin is the displayed score, and
    /// provenance must not claim the declared path.
    #[cfg(target_arch = "wasm32")]
    fn resolve_swang_source(
        &self,
        _compiled: &eval::CompiledProgram,
    ) -> Result<(Score, SwangSourceOrigin), String> {
        let score = self.score.clone().ok_or_else(|| {
            "load a score first — browser Swang uses the displayed file".to_owned()
        })?;
        Ok((score, SwangSourceOrigin::DisplayedScore))
    }

    /// Paints Swang candidate `i` into the roll.
    fn swang_show(&mut self, i: usize) {
        self.swang.selected = Some(i);
        let Some(set) = self.swang.set.as_ref() else {
            return;
        };
        let (Some(score), Some(row)) = (set.scores.get(i).cloned(), set.rows.get(i)) else {
            return;
        };
        let title = format!(
            "swang #{} {} · {:.3}",
            row.rank, row.strategy, row.aggregate
        );
        let candidate_id = row.id.clone();
        // Program + source come from the run's captured context, never the live
        // editor text. Without a context (no successful run yet) there is
        // nothing honest to record.
        let Some(ctx) = self.swang_ctx.as_ref() else {
            return;
        };
        let generator = GeneratorProvenance::Swang {
            program: ctx.program.clone(),
            source_path: ctx.source_path.clone(),
            strategy: row.strategy.clone(),
            variant_seed: row.variant_seed,
            rank: row.rank,
            aggregate: row.aggregate,
        };
        let run = ctx.run;
        let id = self
            .history
            .record(run, candidate_id, title.clone(), score.clone(), generator);
        self.history.select(id);
        // Swang candidates record into A/B too, so `b` can compare a Swang take
        // against the last candidate of either source.
        self.remember_shown(AuditionCandidate::Swang(i));
        self.show_score(score, title);
    }

    /// Writes the selected candidate to the program's own `export` path, plus a
    /// provenance sidecar. Native only — a browser download is a later slice.
    #[cfg(not(target_arch = "wasm32"))]
    fn swang_build(&mut self) {
        let outcome = self.write_swang_export();
        self.swang.status = match outcome {
            Ok(path) => format!("built -> {path}"),
            Err(err) => format!("build failed: {err}"),
        };
    }

    /// Exports the selected candidate to the program's `export` path and stamps
    /// a `.json` provenance sidecar; returns the written `.mid` path.
    #[cfg(not(target_arch = "wasm32"))]
    fn write_swang_export(&self) -> Result<String, String> {
        use std::fs;

        use griff_core::midi::export_score;

        // Belt to the UI's suspenders: never export a result the current text
        // did not produce, even if a change somehow slipped past the editor's
        // change signal. Provenance must describe the music it made.
        if !self.swang.run_is_current() {
            return Err("the program changed since the last run — Run it again".to_owned());
        }
        let path = self
            .swang
            .export_path
            .clone()
            .ok_or("run the program first")?;
        let i = self.swang.selected.ok_or("no candidate selected")?;
        let set = self.swang.set.as_ref().ok_or("run the program first")?;
        let row = set.rows.get(i).ok_or("no such candidate")?;
        let score = set.scores.get(i).ok_or("no such candidate")?;

        let bytes = export_score(score).map_err(|e| format!("{e:?}"))?;
        fs::write(&path, &bytes).map_err(|e| format!("{e}"))?;

        let provenance = serde_json::json!({
            "schema": "griff.swang-build",
            "version": 1,
            "program": self.swang.text,
            "source": self.swang.source_path(),
            "selected": { "rank": row.rank, "strategy": row.strategy, "variant_seed": row.variant_seed },
        });
        let sidecar = format!("{path}.json");
        let text = serde_json::to_string_pretty(&provenance).map_err(|e| format!("{e}"))?;
        fs::write(&sidecar, text).map_err(|e| format!("{e}"))?;
        Ok(path)
    }

    /// The web target cannot write files; Build is native-only this slice.
    #[cfg(target_arch = "wasm32")]
    fn swang_build(&mut self) {
        self.swang.status = "build is native-only in this slice".to_owned();
    }

    /// The Generate panel's lower half: the set's provenance line, the ranked
    /// rows, and the keep actions for the selected one. Reports what the user
    /// asked for through `show` / `keep` / `open`, so the window applies every
    /// action after the panel closes its borrow of the panel state.
    /// The run's two audition variants: the intact S6 winner, and the S7 global
    /// chain assembled from the same set.
    ///
    /// Named for what they are, not "original" and "alternative" — one is a
    /// whole candidate S6 ranked first, the other is a part built from bars of
    /// several. The intact winner stays the default; the chain is an explicit
    /// second thing to ask for.
    fn generate_audition_variants(&self, ui: &mut egui::Ui, acts: &mut GenerateActions) {
        let Some(active) = self.gen_panel.active.as_ref() else {
            return;
        };
        ui.separator();
        ui.horizontal(|ui| {
            ui.monospace("audition");
            // Ranked candidate 0 specifically — the candidate `baseline_cost`
            // measures. Browsing another row is the table's job, not this
            // chip's, and highlighting it for any Generate row would say the
            // baseline is measuring whatever is playing.
            let intact = self.current == Some(AuditionCandidate::Generate(0));
            if ui
                .selectable_label(intact, "S6 Intact")
                .on_hover_text("the whole candidate S6 ranked first")
                .clicked()
            {
                acts.show = Some(AuditionPick::Intact);
            }
            match &active.chain {
                GlobalChainOutcome::Planned(chain) => {
                    let showing = self.current == Some(AuditionCandidate::GlobalChain);
                    if ui
                        .selectable_label(showing, "S7 Global Chain")
                        .on_hover_text(format!(
                            "one candidate per bar, chosen for the whole sequence\n\
                             chain {:.3} vs intact {:.3}",
                            chain.plan.total_cost, chain.baseline_cost,
                        ))
                        .clicked()
                    {
                        acts.show = Some(AuditionPick::Chain);
                    }
                    // Keeping is native-only, like the candidate keep beside it.
                    #[cfg(not(target_arch = "wasm32"))]
                    if ui
                        .button("keep")
                        .on_hover_text("write the assembled chain and its supplier map")
                        .clicked()
                    {
                        acts.keep_chain = true;
                    }
                }
                GlobalChainOutcome::Refused(error) => {
                    // Shown, disabled, with the reason — not hidden, which would
                    // leave the user wondering, and not substituted with the
                    // intact winner, which would be a lie.
                    ui.add_enabled(false, egui::Button::new("S7 Global Chain"))
                        .on_disabled_hover_text(chain_refusal_summary(*error));
                    ui.weak("unavailable");
                }
            }
        });
        if let GlobalChainOutcome::Refused(error) = &active.chain {
            ui.weak(format!(
                "no global chain: {}",
                chain_refusal_summary(*error)
            ));
            return;
        }
        if let Some(summary) = self.displayed_chain_summary() {
            chain_summary_block(ui, &summary);
        }
    }

    /// The explanation of the chain that is **sounding**, if one is.
    ///
    /// Stub: still reads the active run.
    fn displayed_chain_summary(&self) -> Option<GlobalChainSummary> {
        self.history
            .chain_of(self.gen_panel.active.as_ref()?.context.run)
            .and_then(global_chain_summary)
    }

    fn generate_candidates(&self, ui: &mut egui::Ui, acts: &mut GenerateActions) {
        let Some(set) = self.gen_panel.set() else {
            ui.separator();
            ui.weak(match self.material {
                Some(_) => "a corpus is loaded — generate to rank a candidate set",
                None => "no corpus: the pass will seed from the displayed score alone",
            });
            return;
        };

        ui.separator();
        let gesture = set.summary.gesture.as_ref().map_or_else(
            || "off".to_owned(),
            |(n, rest)| format!("{n} notes / {rest}"),
        );
        ui.weak(format!(
            "{} templates · {} references · gesture {} · {}-tone scale{}",
            set.summary.templates,
            set.summary.references,
            gesture,
            set.summary.scale_tones,
            if set.summary.skipped.is_empty() {
                String::new()
            } else {
                format!(" · {} records skipped", set.summary.skipped.len())
            },
        ));
        self.generate_audition_variants(ui, acts);
        ui.separator();

        egui::ScrollArea::vertical().show(ui, |ui| {
            for (i, row) in set.rows.iter().enumerate() {
                let selected = self.gen_panel.selected == Some(i);
                let label = format!(
                    "{:>3}. {:<26} {:.3}  {} notes",
                    row.rank, row.strategy, row.aggregate, row.note_count,
                );
                let hover = row
                    .axes
                    .iter()
                    .map(|(name, value)| format!("{name} {value:.2}"))
                    .collect::<Vec<_>>()
                    .join("\n");
                if ui
                    .selectable_label(selected, egui::RichText::new(label).monospace())
                    .on_hover_text(hover)
                    .clicked()
                {
                    acts.show = Some(AuditionPick::Candidate(i));
                }
            }
        });

        if let Some(i) = self.gen_panel.selected {
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("⤓ keep .mid").clicked() {
                    acts.keep = Some(i);
                }
                if ui
                    .button("🔊 open")
                    .on_hover_text("write it and hand it to your .mid app")
                    .clicked()
                {
                    acts.open = Some(i);
                }
            });
        }
    }

    /// Captures the focused track of the loaded score as a `chunk.json` string
    /// (ADR-0026), through the shared [`griff_ui_core::capture::build_chunk`] —
    /// byte-compatible with what `griff manifest` reads.
    ///
    /// # Errors
    /// Returns a message if no score is loaded yet, or if measuring fails.
    pub fn capture_json(&self, inputs: &CaptureInputs<'_>) -> Result<String, String> {
        let score = self
            .score
            .as_ref()
            .ok_or_else(|| "no score loaded".to_owned())?;
        let chunk = build_chunk(score, self.selected_track, inputs)?;
        serde_json::to_string_pretty(&chunk).map_err(|err| err.to_string())
    }

    /// Builds and saves a `chunk.json` for the focused track from the form,
    /// recording the outcome in the form's status line.
    fn do_capture(&mut self) {
        let now = now_rfc3339();
        let result = self
            .capture_json(&self.form.inputs(&now, &now))
            .and_then(|json| {
                let filename = chunk_filename(&self.form.id);
                save_chunk(&filename, &json).map(|()| filename)
            });
        self.form.status = Some(match result {
            Ok(filename) => format!("saved {filename}"),
            Err(err) => format!("capture failed: {err}"),
        });
    }

    /// The capture panel (ADR-0026): a floating form editing the curator inputs,
    /// with a button that builds a `chunk.json` for the focused track. Shown
    /// when the inspector is toggled (the `i` key).
    fn capture_panel(&mut self, ctx: &egui::Context) {
        egui::Window::new("capture · chunk.json")
            .default_width(320.0)
            .show(ctx, |ui| {
                text_field(ui, "id", &mut self.form.id);
                text_field(ui, "title", &mut self.form.title);
                text_field(ui, "file", &mut self.form.filename);
                text_field(ui, "tuning", &mut self.form.tuning);
                text_field(ui, "tags", &mut self.form.tags_idx);
                text_field(ui, "notes", &mut self.form.notes);
                combo(ui, "rights", &mut self.form.rights_status, RIGHTS);
                combo(ui, "acquired", &mut self.form.acquisition, ACQUISITION);
                combo(ui, "cohort", &mut self.form.cohort, COHORT);
                ui.checkbox(&mut self.form.redistributable, "redistributable");
                if ui.button("⬇ capture chunk.json").clicked() {
                    self.do_capture();
                }
                if let Some(status) = &self.form.status {
                    ui.label(status);
                }
            });
    }

    /// Loads a corpus from serialized `chunk.json` strings — the OPFS tree on
    /// web (ADR-0027 Slice 5) — into the dock and shows it. Unparseable entries
    /// are skipped, so a partially-readable corpus still browses.
    pub fn load_corpus(&mut self, jsons: &[String]) {
        self.corpus = jsons
            .iter()
            .filter_map(|json| serde_json::from_str::<ChunkMeta>(json).ok())
            .collect();
        self.show_dock = true;
    }

    /// Applies a curation edit to the chunk `id` through the shared
    /// `griff_ui_core::curation` ops, updating the in-memory corpus; returns the
    /// re-serialized `chunk.json` to persist.
    ///
    /// # Errors
    /// A message if the chunk is absent or the op rejects the edit.
    fn curate(&mut self, id: &str, action: &CurationAction) -> Result<String, String> {
        let idx = self
            .corpus
            .iter()
            .position(|c| c.id.0 == id)
            .ok_or("no such chunk")?;
        let json = serde_json::to_string(self.corpus.get(idx).ok_or("no such chunk")?)
            .map_err(|err| err.to_string())?;
        let edited = match action {
            CurationAction::Decide(decision) => decide_record(&json, *decision),
            CurationAction::Rename(title) => rename_record(&json, title),
            CurationAction::Retag(names) => set_tags(&json, names),
        }
        .map_err(|err| format!("{err:?}"))?;
        let meta: ChunkMeta = serde_json::from_str(&edited).map_err(|err| err.to_string())?;
        *self.corpus.get_mut(idx).ok_or("no such chunk")? = meta;
        Ok(edited)
    }

    /// Curates the chunk `id` and persists it back to the corpus, recording the
    /// outcome in the dock's status line. On a (synchronous, native) persist
    /// failure the in-memory edit is rolled back so the dock matches disk.
    fn apply_curation(&mut self, id: &str, action: &CurationAction) {
        let snapshot = self
            .corpus
            .iter()
            .position(|c| c.id.0 == id)
            .and_then(|idx| self.corpus.get(idx).cloned().map(|chunk| (idx, chunk)));
        self.dock_status = Some(match self.curate(id, action) {
            Ok(json) => {
                let filename = chunk_filename(id);
                match persist_chunk(&filename, &json) {
                    Ok(()) => format!("saved {filename}"),
                    Err(err) => {
                        // The persist failed — undo the in-memory edit so the dock
                        // never shows a change that didn't reach disk.
                        if let Some((idx, before)) = snapshot {
                            if let Some(slot) = self.corpus.get_mut(idx) {
                                *slot = before;
                            }
                        }
                        format!("save failed: {err}")
                    }
                }
            }
            Err(err) => format!("curate failed: {err}"),
        });
    }

    /// The corpus dock (ADR-0027 Slice 5): an aggregate dashboard, browse filters
    /// (class/tag · rights · cohort · dedup), and the filtered chunk list over the
    /// loaded `corpus`. On web the 📚 toolbar button fills it from OPFS; `c` toggles.
    fn corpus_dock(&mut self, ctx: &egui::Context) {
        let mut clicked: Option<String> = None;
        let mut action: Option<(String, CurationAction)> = None;
        egui::Window::new("corpus")
            .default_width(360.0)
            .show(ctx, |ui| {
                let stats = CorpusStats::aggregate(&self.corpus);
                if stats.total == 0 {
                    ui.label("no corpus loaded — capture chunks, then press 📚 Corpus");
                    return;
                }
                // ── dashboard ──
                ui.label(format!(
                    "{} chunks · {} redistributable · {} near-dup · {} rights unset",
                    stats.total, stats.redistributable, stats.duplicates, stats.rights_unset,
                ));
                let tags = stats.present_tags();
                if !tags.is_empty() {
                    let top: Vec<String> = tags
                        .iter()
                        .take(4)
                        .map(|(tag, n)| format!("{tag:?}×{n}"))
                        .collect();
                    ui.weak(top.join("   "));
                }
                ui.separator();
                // ── filters ──
                ui.horizontal(|ui| {
                    ui.label("find");
                    ui.text_edit_singleline(&mut self.corpus_filter.query);
                });
                ui.horizontal(|ui| {
                    opt_combo(ui, "cohort", &mut self.corpus_filter.cohort, COHORTS);
                    opt_combo(ui, "rights", &mut self.corpus_filter.rights, RIGHTS_STATUS);
                });
                ui.horizontal(|ui| {
                    tag_combo(ui, &mut self.corpus_filter.tag);
                    ui.checkbox(&mut self.corpus_filter.redistributable_only, "redist only");
                    ui.checkbox(&mut self.corpus_filter.duplicates_only, "dups only");
                });
                ui.separator();
                // ── filtered list (selectable) ──
                let kept = filter_chunks(&self.corpus, &self.corpus_filter);
                ui.label(format!("{} / {} shown", kept.len(), stats.total));
                egui::ScrollArea::vertical()
                    .max_height(200.0)
                    .show(ui, |ui| {
                        for chunk in kept {
                            let is_sel = self.selected.as_deref() == Some(chunk.id.0.as_str());
                            if chunk_row(ui, chunk, is_sel) {
                                clicked = Some(chunk.id.0.clone());
                            }
                        }
                    });
                // ── curation inspector (ADR-0027 Slice 6) ──
                if let Some(id) = self.selected.clone() {
                    if let Some(chunk) = self.corpus.iter().find(|c| c.id.0 == id) {
                        ui.separator();
                        // Bind any edit to *this* chunk's id, so a same-frame row click
                        // (which changes the selection below) can't redirect it.
                        let mut pending = None;
                        inspector(ui, chunk, &mut self.rename_buf, &mut pending);
                        if let Some(pending) = pending {
                            action = Some((chunk.id.0.clone(), pending));
                        }
                    }
                }
                if let Some(status) = &self.dock_status {
                    ui.weak(status);
                }
            });
        // Apply the click/curation after the closure releases its borrows.
        if let Some(id) = clicked {
            if self.selected.as_deref() != Some(id.as_str()) {
                if let Some(chunk) = self.corpus.iter().find(|c| c.id.0 == id) {
                    self.rename_buf = chunk.title.clone();
                }
                self.dock_status = None;
            }
            self.selected = Some(id);
        }
        if let Some((id, action)) = action {
            self.apply_curation(&id, &action);
        }
    }

    /// Drains the frame's key presses into the reducer; returns whether the
    /// user asked to quit.
    fn handle_input(&mut self, ctx: &egui::Context) -> bool {
        // While a capture-form text field has focus, let egui keep the keys:
        // typing in a field must not drive viewport shortcuts (space toggles
        // playback, arrows scroll the roll, `i` closes the panel).
        if ctx.egui_wants_keyboard_input() {
            return false;
        }
        // `c`, `g` and `t` toggle the dock, the Generate panel and the palette —
        // shell concerns, not viewport `Intent`s. The palette especially: which
        // colours a renderer wears is not something the shared reducer should
        // have an opinion about.
        if ctx.input(|i| i.key_pressed(Key::C)) {
            self.show_dock = !self.show_dock;
        }
        if ctx.input(|i| i.key_pressed(Key::G)) {
            self.gen_panel.open = !self.gen_panel.open;
        }
        if ctx.input(|i| i.key_pressed(Key::E)) {
            self.swang.open = !self.swang.open;
        }
        if ctx.input(|i| i.key_pressed(Key::Y)) {
            self.history_open = !self.history_open; // the session candidate history
        }
        if ctx.input(|i| i.key_pressed(Key::B)) {
            self.ab_swap(); // A/B between the last two candidates
        }
        if ctx.input(|i| i.key_pressed(Key::T)) {
            self.toggle_theme();
        }
        let intents: Vec<Intent> = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Key {
                        key, pressed: true, ..
                    } => key_to_intent(*key),
                    _ => None,
                })
                .collect()
        });
        let mut quit = false;
        for intent in intents {
            if matches!(self.vp.apply(intent, &self.ctx), Step::Quit) {
                quit = true;
            }
        }
        quit
    }

    /// Resolves and paints the scene into `ui`.
    fn paint(&mut self, ui: &egui::Ui) {
        let rect = ui.max_rect();
        let origin = rect.min;
        let cols = (rect.width() / CELL_W) as u16;
        let total_rows = (rect.height() / CELL_H) as u16;
        let rows = total_rows.saturating_sub(1); // the band takes the top row

        if !self.fitted && cols > GUTTER {
            self.vp
                .fit(u32::from(cols.saturating_sub(GUTTER)), &self.ctx);
            self.fitted = true;
        }
        if self.vp.playing {
            self.vp.autoscroll(u32::from(cols.saturating_sub(GUTTER)));
        }

        let scene = resolve(
            &self.view,
            &self.analysis,
            &self.vp,
            GridSize { cols, rows },
        );
        let painter = ui.painter();
        for col in 0..scene.cols {
            if let Some(cell) = scene.band_cell(col) {
                paint_cell(painter, cell_rect(origin, col, 0), *cell, &self.theme);
            }
        }
        for row in 0..scene.rows {
            for col in 0..scene.cols {
                if let Some(cell) = scene.plane_cell(row, col) {
                    paint_cell(
                        painter,
                        cell_rect(origin, col, row.saturating_add(1)),
                        *cell,
                        &self.theme,
                    );
                }
            }
        }
    }

    /// Switches the cockpit between the theme's two modes.
    ///
    /// Everything downstream — the plane, the band, and egui's own chrome via
    /// [`Self::install_visuals`] — reads the theme every frame, so there is
    /// nothing else to repaint.
    pub fn toggle_theme(&mut self) {
        self.theme = if self.is_dark() {
            Theme::light()
        } else {
            Theme::dark()
        };
    }

    /// Whether the cockpit is currently in the theme's dark mode.
    fn is_dark(&self) -> bool {
        self.theme.surface.luminance() < 0.5
    }

    /// Paints egui's own chrome from the theme, so the widgets around the plane
    /// come from the same palette the plane does — before this, the plane was the
    /// design mock's and the chrome was stock egui.
    ///
    /// Text colours stay egui's: it derives weak / strong / disabled from its
    /// base, and overriding the base flattens those distinctions into one.
    fn install_visuals(&self, ctx: &egui::Context) {
        let mut visuals = if self.is_dark() {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };
        visuals.panel_fill = color32(self.theme.surface);
        visuals.window_fill = color32(self.theme.panel);
        visuals.window_stroke = egui::Stroke::new(1.0_f32, color32(self.theme.stroke));
        visuals.selection.bg_fill = color32(self.theme.accent);
        visuals.hyperlink_color = color32(self.theme.accent);
        ctx.set_visuals(visuals);
    }

    /// The top toolbar — the discoverable surface, so the controls aren't hidden
    /// behind hotkeys: a track selector (the roll shows one part at a time),
    /// play/pause, and toggles for the capture form and the corpus dock.
    /// The corpus dock and the three panel toggles (generate, swang, history)
    /// — the windowed views the toolbar and their hotkeys open.
    fn window_toggle_buttons(&mut self, ui: &mut egui::Ui) {
        if ui
            .button("📚 corpus")
            .on_hover_text("browse the captured corpus (c)")
            .clicked()
        {
            self.show_dock = !self.show_dock;
        }
        if ui
            .button("⚙ generate")
            .on_hover_text("rank a candidate set from the corpus (g)")
            .clicked()
        {
            self.gen_panel.open = !self.gen_panel.open;
        }
        if ui
            .button("✎ swang")
            .on_hover_text("write a Swang program and run it (e)")
            .clicked()
        {
            self.swang.open = !self.swang.open;
        }
        if ui
            .button("🕮 history")
            .on_hover_text("browse this session's candidates (y)")
            .clicked()
        {
            self.history_open = !self.history_open;
        }
    }

    fn toolbar_bar(&mut self, ui: &mut egui::Ui) -> Option<usize> {
        let mut focus: Option<usize> = None;
        ui.horizontal_wrapped(|ui| {
            if self.track_names.len() > 1 {
                let current = self
                    .track_names
                    .get(self.selected_track)
                    .map_or("—", String::as_str);
                egui::ComboBox::from_label("track")
                    .selected_text(current)
                    .show_ui(ui, |ui| {
                        for (i, name) in self.track_names.iter().enumerate() {
                            if ui
                                .selectable_label(i == self.selected_track, name)
                                .clicked()
                            {
                                focus = Some(i);
                            }
                        }
                    });
                ui.separator();
            }
            let play = if self.vp.playing {
                "⏸ pause"
            } else {
                "▶ play"
            };
            if ui.button(play).on_hover_text("space").clicked() {
                self.vp.playing = !self.vp.playing;
            }
            if ui
                .button("⤓ capture")
                .on_hover_text("edit + cut a chunk from the selected track (i)")
                .clicked()
            {
                self.vp.show_inspector = !self.vp.show_inspector;
            }
            self.window_toggle_buttons(ui);
            let mode = if self.is_dark() {
                "◑ light"
            } else {
                "◐ dark"
            };
            if ui
                .button(mode)
                .on_hover_text("switch the palette (t)")
                .clicked()
            {
                self.toggle_theme();
            }
            if !self.track_names.is_empty() {
                ui.separator();
                ui.weak(format!(
                    "{}  ·  track {}/{}",
                    self.title,
                    self.selected_track.saturating_add(1),
                    self.track_names.len()
                ));
            }
        });
        focus
    }

    /// The score's own tempo, floored positive so the audition scale never
    /// divides by zero.
    const fn base_bpm(&self) -> f64 {
        self.ctx.tempo_bpm.max(1.0)
    }

    /// The BPM the playhead currently advances at — written tempo × audition
    /// scale. A display value; it never touches the Score or its export.
    fn playback_bpm(&self) -> f64 {
        self.base_bpm() * self.tempo_scale
    }

    /// Sets the audition scale so playback runs at `bpm` (clamped 20..=300),
    /// leaving the score untouched.
    fn set_playback_bpm(&mut self, bpm: f64) {
        self.tempo_scale = (bpm.clamp(20.0, 300.0)) / self.base_bpm();
    }

    /// Turns a bar-index range into the loop's tick span, or clears the loop.
    fn set_loop_bars(&mut self, on: bool, from_bar: usize, to_bar: usize) {
        self.loop_range = on
            .then(|| remap_loop_range(from_bar, to_bar, &self.view.bar_lines, self.ctx.tick_end))
            .flatten();
    }

    /// The bottom transport bar (S8 Slice 2): play/pause, stop, tempo
    /// audition, loop, and the MIDI device picker.
    fn transport_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let play = if self.vp.playing { "⏸" } else { "▶" };
            if ui
                .button(play)
                .on_hover_text("play / pause (space)")
                .clicked()
            {
                self.vp.playing = !self.vp.playing;
            }
            if ui
                .button("⏹")
                .on_hover_text("stop — rewind and silence")
                .clicked()
            {
                self.stop_playback();
            }

            ui.separator();
            // Tempo audition — the score's own tempo is untouched.
            ui.label("bpm");
            let mut bpm = self.playback_bpm();
            if ui
                .add(
                    egui::DragValue::new(&mut bpm)
                        .range(20.0..=300.0)
                        .speed(1.0),
                )
                .on_hover_text("audition tempo — never changes the score or its export")
                .changed()
            {
                self.set_playback_bpm(bpm);
            }
            if ui.button("½×").clicked() {
                self.tempo_scale = 0.5;
            }
            if ui
                .button("1×")
                .on_hover_text("back to the written tempo")
                .clicked()
            {
                self.tempo_scale = 1.0;
            }
            if ui.button("2×").clicked() {
                self.tempo_scale = 2.0;
            }

            ui.separator();
            self.transport_loop(ui);

            if self.ab_other.is_some() {
                ui.separator();
                if ui
                    .button("A/B")
                    .on_hover_text("swap to the other candidate (b)")
                    .clicked()
                {
                    self.ab_swap();
                }
            }
        });
        ui.horizontal(|ui| self.transport_device(ui));
    }

    /// The loop controls: a toggle plus the bar range it spans.
    fn transport_loop(&mut self, ui: &mut egui::Ui) {
        let bars = self.view.bar_count.max(1);
        let (mut on, (mut from, mut to)) = self.loop_range.map_or_else(
            || (false, (0, bars.saturating_sub(1))),
            |_| (true, self.loop_bar_indices()),
        );
        let mut changed = ui.checkbox(&mut on, "loop").changed();
        ui.add_enabled_ui(on, |ui| {
            ui.label("bars");
            changed |= ui
                .add(egui::DragValue::new(&mut from).range(0..=bars.saturating_sub(1)))
                .changed();
            ui.label("–");
            changed |= ui
                .add(egui::DragValue::new(&mut to).range(0..=bars.saturating_sub(1)))
                .changed();
        });
        if changed {
            self.set_loop_bars(on, from, to);
        }
    }

    /// The loop range as inclusive bar indices, for the loop `DragValue`s.
    fn loop_bar_indices(&self) -> (usize, usize) {
        let Some((lo, hi)) = self.loop_range else {
            return (0, self.view.bar_count.saturating_sub(1));
        };
        let lines = &self.view.bar_lines;
        let from = lines.partition_point(|&t| t < lo);
        let to = lines.partition_point(|&t| t < hi).saturating_sub(1);
        (from, to.max(from))
    }

    /// The MIDI output device picker (native) or the backend status (web).
    fn transport_device(&mut self, ui: &mut egui::Ui) {
        let mut connect: Option<usize> = None;
        let mut refresh = false;
        if self.synth.ports().is_empty() {
            ui.weak(self.synth.status());
            if ui
                .button("🔄")
                .on_hover_text("rescan MIDI outputs")
                .clicked()
            {
                refresh = true;
            }
        } else {
            let current = self
                .synth
                .selected()
                .and_then(|i| self.synth.ports().get(i))
                .map_or("(pick a MIDI output)", String::as_str)
                .to_owned();
            egui::ComboBox::from_label("out")
                .selected_text(current)
                .show_ui(ui, |ui| {
                    for (i, name) in self.synth.ports().iter().enumerate() {
                        if ui
                            .selectable_label(self.synth.selected() == Some(i), name)
                            .clicked()
                        {
                            connect = Some(i);
                        }
                    }
                });
            if ui.button("🔄").on_hover_text("rescan").clicked() {
                refresh = true;
            }
            ui.weak(self.synth.status());
        }
        if let Some(i) = connect {
            self.connect_device(i);
        }
        if refresh {
            // Rescanning can drop the open port; hush it first, then reposition.
            self.player.silence(&mut self.synth);
            self.synth.refresh_ports();
            self.player.seek(self.vp.play_tick, &mut self.synth);
        }
    }

    /// Switches the MIDI output: releases whatever is sounding on the **old**
    /// port before the connection is dropped (a new port cannot ask the old
    /// one to hush), opens the new one, and repositions the schedule on it.
    fn connect_device(&mut self, index: usize) {
        self.player.silence(&mut self.synth);
        self.synth.connect(index);
        self.player.seek(self.vp.play_tick, &mut self.synth);
    }
}

/// The one button the user pressed in the Swang window this frame (buttons
/// are mutually exclusive; Ctrl+Enter also maps to `Run`).
#[derive(Default, PartialEq, Eq)]
enum SwangAction {
    #[default]
    None,
    Check,
    Format,
    Run,
    Build,
}

/// What the Swang window's body reports back for `swang_window` to apply
/// after the `self` borrow closes.
#[derive(Default)]
struct SwangActions {
    action: SwangAction,
    click: Option<usize>,
}

/// What the Generate window's frame asked for, applied after it closes.
///
/// The window paints while holding a borrow of the panel, so every action is
/// recorded here and acted on afterwards — the same deferral the Swang panel
/// uses, and the reason none of these can be a method call mid-paint.
#[derive(Debug, Default)]
struct GenerateActions {
    /// Run the pass again.
    run: bool,
    /// What to audition, if anything.
    show: Option<AuditionPick>,
    /// Write the run's assembled global chain to disk.
    keep_chain: bool,
    /// Write candidate `i` to disk.
    keep: Option<usize>,
    /// Open candidate `i`'s kept file.
    open: Option<usize>,
}

/// What one frame asked to audition.
///
/// An enum rather than a flag apiece: a frame chooses **one** thing to hear, and
/// three independent booleans could describe a frame asking for three at once —
/// which would resolve by whichever branch ran last, silently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AuditionPick {
    /// Candidate `i` of the current set.
    Candidate(usize),
    /// The S6 intact winner — ranked candidate 0, the one the chain's baseline
    /// cost measures.
    Intact,
    /// The run's assembled global chain.
    Chain,
}

/// The pixel rect of grid position (`col`, `vis_row`).
fn cell_rect(origin: egui::Pos2, col: u16, vis_row: u16) -> Rect {
    let x = origin.x + f32::from(col) * CELL_W;
    let y = origin.y + f32::from(vis_row) * CELL_H;
    Rect::from_min_size(egui::pos2(x, y), egui::vec2(CELL_W, CELL_H))
}

/// Paints one placed cell — the theme says what it looks like, this draws it.
fn paint_cell(painter: &egui::Painter, rect: Rect, cell: SceneCell, theme: &Theme) {
    let style = cell_style(cell, theme);
    if let Some(fill) = style.fill {
        painter.rect_filled(rect, CornerRadius::ZERO, color32(fill));
    }
    if cell.glyph != ' ' {
        if let Some(ink) = style.ink {
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                cell.glyph,
                FontId::monospace(CELL_H * 0.8),
                color32(ink),
            );
        }
    }
}

impl eframe::App for CockpitApp {
    // egui 0.34 deprecates the `TopBottomPanel` alias and the panel `.show`;
    // `show_inside` carves the toolbar off the eframe-provided central ui.
    #[allow(deprecated)]
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // Apply a file/capture/corpus request the page handed us, if any.
        #[cfg(target_arch = "wasm32")]
        web::drain(self);
        let ctx = ui.ctx().clone();
        self.install_visuals(&ctx);
        if self.handle_input(&ctx) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        // An input that moved the playhead (a section jump) is a seek: silence
        // and reposition the audio before advancing.
        if self.vp.play_tick != self.last_play_tick {
            self.play_pos = f64::from(self.vp.play_tick);
            self.player.seek(self.vp.play_tick, &mut self.synth);
        }
        // A pause — Space, or the viewport stopping at the end — must never
        // leave a note ringing.
        if !self.vp.playing && self.player.active_count() > 0 {
            self.player.silence(&mut self.synth);
        }
        if self.vp.playing {
            let dt = f64::from(ctx.input(|i| i.stable_dt)).min(0.1);
            self.advance_audio(dt);
            ctx.request_repaint();
        }
        self.last_play_tick = self.vp.play_tick;
        let focus = egui::TopBottomPanel::top("toolbar")
            .show_inside(ui, |ui| self.toolbar_bar(ui))
            .inner;
        if let Some(track) = focus {
            self.focus_on_track(track);
        }
        egui::TopBottomPanel::bottom("transport").show_inside(ui, |ui| self.transport_bar(ui));
        egui::CentralPanel::default().show_inside(ui, |ui| self.paint(ui));
        if self.vp.show_inspector {
            self.capture_panel(&ctx);
        }
        if self.show_dock {
            self.corpus_dock(&ctx);
        }
        if self.gen_panel.open {
            self.generate_window(&ctx);
        }
        if self.swang.open {
            self.swang_window(&ctx);
        }
        if self.history_open {
            self.history_window(&ctx);
        }
    }
}

/// Hands `path` to the OS's default handler for its type. The cockpit does not
/// synthesise audio: hearing a kept `.mid` means opening it in whatever the user
/// already has registered (a notation editor, a DAW).
///
/// # Errors
/// A message when the handler cannot be spawned.
#[cfg(not(target_arch = "wasm32"))]
fn open_in_default_app(path: &Path) -> Result<(), String> {
    use std::process::Command;

    let mut cmd = if cfg!(target_os = "windows") {
        // `start` is a cmd builtin, not an executable; the empty "" is its
        // window-title argument, which a quoted path would otherwise be taken as.
        let mut c = Command::new("cmd");
        c.args(["/C", "start", ""]);
        c
    } else if cfg!(target_os = "macos") {
        Command::new("open")
    } else {
        Command::new("xdg-open")
    };
    cmd.arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|err| format!("cannot open {}: {err}", path.display()))
}

/// The browser (wasm) entry point — ADR-0027 Slice 2.
///
/// Mirrors `main.rs` for the web: imports a score through the shared
/// [`griff_core::import::import_score_auto`] (the same parser as the CLI — GP
/// and MIDI alike, ADR-0025), builds the renderer-agnostic view + analysis, and
/// starts the [`CockpitApp`] on an HTML canvas via eframe's `WebGL` runner. Slice
/// 2 paints a built-in demo score; interactive file loading is Slice 3.
#[cfg(target_arch = "wasm32")]
// The `expect`s inside are deliberate: the baked demo is validated on native by
// the `the_baked_demo_score_imports_with_notes_and_sections` test, and a failed
// `WebGL` init is unrecoverable — surfacing the panic is the intended UX.
#[allow(clippy::expect_used)]
pub mod web {
    use std::cell::{Cell, RefCell};

    use eframe::egui;
    use wasm_bindgen::prelude::*;
    use wasm_bindgen_futures::{spawn_local, JsFuture};
    use web_sys::console;

    use griff_core::import::import_score_auto;
    use griff_ui_core::manifest_from_jsons;

    use crate::CockpitApp;

    /// A tiny MIDI baked into the app so the web front paints a real,
    /// importer-parsed score on first load, before the user opens their own.
    const DEMO_SCORE: &[u8] = include_bytes!("../assets/demo.mid");

    /// A picked file waiting to load: its display name and raw bytes.
    type PendingFile = Option<(String, Vec<u8>)>;

    // A one-slot inbox: the page drops a picked file here and the app drains it
    // on the next frame. wasm is single-threaded, so a thread-local needs no lock.
    thread_local! {
        static INBOX: RefCell<PendingFile> = const { RefCell::new(None) };
        /// Set by `request_capture`; drained into a `do_capture` next frame.
        static CAPTURE: Cell<bool> = const { Cell::new(false) };
        /// Set by `load_corpus` (the page read the OPFS tree); drained into the dock.
        static CORPUS: RefCell<Option<Vec<String>>> = const { RefCell::new(None) };
        // The running app's egui context, stashed at start so the inbox/capture
        // requests can wake the reactive web runner.
        static CTX: RefCell<Option<egui::Context>> = const { RefCell::new(None) };
    }

    /// Wakes the reactive web runner so the app drains pending requests.
    fn wake() {
        CTX.with(|cell| {
            if let Some(ctx) = cell.borrow().as_ref() {
                ctx.request_repaint();
            }
        });
    }

    /// Hands a picked file (name + bytes) to the running cockpit; the page calls
    /// this from its file-input change handler. The score loads on the next frame.
    #[wasm_bindgen]
    pub fn load_score(name: String, bytes: Vec<u8>) {
        INBOX.with(|inbox| *inbox.borrow_mut() = Some((name, bytes)));
        wake();
    }

    /// Requests a capture of the focused track; the app builds and downloads its
    /// `chunk.json` on the next frame. The page can wire this to a button.
    #[wasm_bindgen]
    pub fn request_capture() {
        CAPTURE.with(|flag| flag.set(true));
        wake();
    }

    /// Folds `chunk.json` strings — the page reads them from the OPFS corpus —
    /// into a `manifest.json` through the shared core (the in-wasm `griff
    /// manifest`, ADR-0027 §3).
    ///
    /// # Errors
    /// Returns a message (thrown to JS) if any string is not a valid chunk.
    // wasm-bindgen marshals `Vec<String>` across the JS boundary by value; we
    // only borrow it to fold.
    #[allow(clippy::needless_pass_by_value)]
    #[wasm_bindgen]
    pub fn build_manifest_json(jsons: Vec<String>) -> Result<String, String> {
        let manifest = manifest_from_jsons(&jsons)?;
        serde_json::to_string_pretty(&manifest).map_err(|err| err.to_string())
    }

    /// Hands the OPFS corpus — every `chunk.json`'s text — to the dock (ADR-0027
    /// Slice 5); the page reads the tree and calls this. The dock opens and lists
    /// the chunks on the next frame.
    #[wasm_bindgen]
    pub fn load_corpus(jsons: Vec<String>) {
        CORPUS.with(|cell| *cell.borrow_mut() = Some(jsons));
        wake();
    }

    /// Applies a pending file and/or capture request. Called by the app at the
    /// top of each frame.
    pub(crate) fn drain(app: &mut CockpitApp) {
        let pending = INBOX.with(|inbox| inbox.borrow_mut().take());
        if let Some((name, bytes)) = pending {
            if let Err(err) = app.load(name, &bytes) {
                console::error_1(&err.into());
            }
        }
        if CAPTURE.with(Cell::take) {
            app.do_capture();
        }
        if let Some(jsons) = CORPUS.with(|cell| cell.borrow_mut().take()) {
            app.load_corpus(&jsons);
        }
    }

    /// Triggers a browser download of `contents` as `filename` (a transient
    /// object-URL anchor click).
    ///
    /// # Errors
    /// Returns a message if the DOM/Blob/URL plumbing is unavailable.
    pub(crate) fn download(filename: &str, contents: &str) -> Result<(), String> {
        use wasm_bindgen::JsCast as _;
        let document = web_sys::window()
            .and_then(|w| w.document())
            .ok_or("no document")?;
        let parts = js_sys::Array::of1(&JsValue::from_str(contents));
        let blob = web_sys::Blob::new_with_str_sequence(&parts).map_err(|_| "blob".to_owned())?;
        let url = web_sys::Url::create_object_url_with_blob(&blob)
            .map_err(|_| "object url".to_owned())?;
        let anchor = document
            .create_element("a")
            .and_then(|el| {
                el.dyn_into::<web_sys::HtmlAnchorElement>()
                    .map_err(Into::into)
            })
            .map_err(|_| "anchor".to_owned())?;
        anchor.set_href(&url);
        anchor.set_download(filename);
        anchor.click();
        let _revoked = web_sys::Url::revoke_object_url(&url);
        Ok(())
    }

    /// Writes `contents` to `corpus/<filename>` in the Origin Private File
    /// System — the browser corpus is the same `chunk.json` bytes the CLI reads.
    // The OPFS handles aren't `Send`, but wasm is single-threaded and the future
    // is driven by `spawn_local`, which never requires `Send`.
    #[allow(clippy::future_not_send)]
    async fn opfs_save(filename: String, contents: String) -> Result<(), JsValue> {
        use wasm_bindgen::JsCast as _;
        let storage = web_sys::window()
            .ok_or_else(|| JsValue::from_str("no window"))?
            .navigator()
            .storage();
        let root = JsFuture::from(storage.get_directory())
            .await?
            .dyn_into::<web_sys::FileSystemDirectoryHandle>()?;
        let dir_opts = web_sys::FileSystemGetDirectoryOptions::new();
        dir_opts.set_create(true);
        let corpus = JsFuture::from(root.get_directory_handle_with_options("corpus", &dir_opts))
            .await?
            .dyn_into::<web_sys::FileSystemDirectoryHandle>()?;
        let file_opts = web_sys::FileSystemGetFileOptions::new();
        file_opts.set_create(true);
        let file = JsFuture::from(corpus.get_file_handle_with_options(&filename, &file_opts))
            .await?
            .dyn_into::<web_sys::FileSystemFileHandle>()?;
        let writable = JsFuture::from(file.create_writable())
            .await?
            .dyn_into::<web_sys::FileSystemWritableFileStream>()?;
        JsFuture::from(writable.write_with_str(&contents)?).await?;
        JsFuture::from(writable.close()).await?;
        Ok(())
    }

    /// Persists a chunk to the OPFS `corpus/` tree (async, fire-and-forget),
    /// mirroring the CLI corpus layout (ADR-0027 §3).
    pub(crate) fn persist(filename: &str, contents: &str) {
        let (filename, contents) = (filename.to_owned(), contents.to_owned());
        spawn_local(async move {
            if let Err(err) = opfs_save(filename, contents).await {
                console::error_1(&err);
            }
        });
    }

    /// Builds the cockpit over the baked demo score.
    fn demo_app() -> CockpitApp {
        let score = import_score_auto(DEMO_SCORE).expect("the baked demo score must import");
        CockpitApp::from_score(score, "demo".to_owned())
    }

    /// Boots the cockpit on `canvas`. The page's ES module calls this after the
    /// generated wasm initialises (`wasm-bindgen --target web`); eframe then
    /// drives the frame loop through `requestAnimationFrame`.
    #[wasm_bindgen]
    pub fn start(canvas: web_sys::HtmlCanvasElement) {
        let app = demo_app();
        let options = eframe::WebOptions::default();
        spawn_local(async move {
            eframe::WebRunner::new()
                .start(
                    canvas,
                    options,
                    Box::new(|cc| {
                        CTX.with(|cell| *cell.borrow_mut() = Some(cc.egui_ctx.clone()));
                        Ok(Box::new(app))
                    }),
                )
                .await
                .expect("failed to start the cockpit web runner");
        });
    }
}

#[cfg(test)]
mod tests {
    // Tests build views from known-good fixtures, `expect`/`unwrap` on them,
    // panic on impossible cases, and index fixed in-range arrays.
    #![allow(
        clippy::expect_used,
        clippy::unwrap_used,
        clippy::panic,
        clippy::indexing_slicing
    )]

    use super::*;
    use std::collections::HashSet;
    use std::{env, fs};

    use eframe::egui;
    use eframe::egui::epaint::ClippedShape;
    use eframe::egui::Shape;
    use griff_core::classify::BarClass;
    use griff_ui_core::playback::ticks_per_second;
    use griff_ui_core::scene::CellRole;

    /// Every fill the painter emitted in one frame.
    fn painted_fills(shapes: &[ClippedShape]) -> HashSet<(u8, u8, u8)> {
        fn walk(shape: &Shape, out: &mut HashSet<(u8, u8, u8)>) {
            match shape {
                Shape::Rect(rect) => {
                    let c = rect.fill;
                    if c.a() > 0 {
                        out.insert((c.r(), c.g(), c.b()));
                    }
                }
                Shape::Vec(shapes) => {
                    for s in shapes {
                        walk(s, out);
                    }
                }
                _ => {}
            }
        }
        let mut out = HashSet::new();
        for clipped in shapes {
            walk(&clipped.shape, &mut out);
        }
        out
    }

    /// Every colour the theme allows a cell to be painted in.
    fn theme_palette(theme: &Theme) -> HashSet<(u8, u8, u8)> {
        let rgb = |c: Rgb| (c.r, c.g, c.b);
        let mut allowed: HashSet<(u8, u8, u8)> = [
            theme.surface,
            theme.panel,
            theme.stroke,
            theme.grid_bar,
            theme.row_shade,
            theme.playhead,
            theme.boundary,
        ]
        .into_iter()
        .map(rgb)
        .collect();
        for class in [
            BarClass::Riff,
            BarClass::Breakdown,
            BarClass::Solo,
            BarClass::Clean,
            BarClass::Unknown,
        ] {
            for selected in [true, false] {
                allowed.insert(rgb(theme.class_fill(class, selected)));
            }
        }
        for lane in 0..6 {
            allowed.insert(rgb(theme.lane(lane)));
        }
        allowed
    }

    #[test]
    // egui 0.34 flags `Context::run` / `CentralPanel::show`; they still drive a
    // CPU frame, which is what this needs.
    #[allow(deprecated)]
    fn every_colour_the_cockpit_paints_comes_from_the_theme() {
        // This crate's whole job is the conversion from the core's `Rgb`. A
        // colour it mixed itself is a colour the preview does not have, and a
        // place the two renderers can drift apart again (ADR-0028).
        let mut app = demo_app();
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(
                egui::pos2(0.0, 0.0),
                egui::vec2(1200.0, 600.0),
            )),
            ..Default::default()
        };
        let output = ctx.run(input, |ctx| {
            // What `App::ui` does before it paints: without it the surface is
            // stock egui's #1b1b1b, not the theme's — which is the drift this
            // whole exercise is about.
            app.install_visuals(ctx);
            egui::CentralPanel::default().show(ctx, |ui| app.paint(ui));
        });

        let allowed = theme_palette(&app.theme);
        for painted in painted_fills(&output.shapes) {
            assert!(
                allowed.contains(&painted),
                "the cockpit painted #{:02x}{:02x}{:02x}, which is in no token of the theme",
                painted.0,
                painted.1,
                painted.2
            );
        }
    }

    /// Every glyph the painter emitted in one frame, in paint order.
    fn painted_glyphs(shapes: &[ClippedShape]) -> String {
        fn walk(shape: &Shape, out: &mut String) {
            match shape {
                Shape::Text(text) => out.push_str(text.galley.text()),
                Shape::Vec(shapes) => {
                    for s in shapes {
                        walk(s, out);
                    }
                }
                _ => {}
            }
        }
        let mut out = String::new();
        for clipped in shapes {
            walk(&clipped.shape, &mut out);
        }
        out
    }

    #[test]
    // egui 0.34 flags `Context::run` / `CentralPanel::show`; they still drive a
    // CPU frame, which is what this needs.
    #[allow(deprecated)]
    fn the_light_palette_is_reachable_and_paints_the_whole_frame() {
        // The core has carried a light mode since ADR-0028; until the cockpit
        // could switch to it, it was a palette nobody could see. Toggling must
        // repaint *everything* from it — a half-switched frame (light plane,
        // dark chrome) is the drift in miniature.
        let mut app = demo_app();
        assert_eq!(app.theme, Theme::dark(), "the cockpit opens dark");

        app.toggle_theme();
        assert_eq!(app.theme, Theme::light(), "and toggles to light");

        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(
                egui::pos2(0.0, 0.0),
                egui::vec2(1200.0, 600.0),
            )),
            ..Default::default()
        };
        let output = ctx.run(input, |ctx| {
            app.install_visuals(ctx);
            egui::CentralPanel::default().show(ctx, |ui| app.paint(ui));
        });

        let light = theme_palette(&Theme::light());
        let dark = theme_palette(&Theme::dark());
        let painted = painted_fills(&output.shapes);
        for fill in &painted {
            assert!(
                light.contains(fill),
                "in light mode the cockpit painted #{:02x}{:02x}{:02x}, \
                 which is in no light token",
                fill.0,
                fill.1,
                fill.2
            );
        }
        assert!(
            !painted
                .iter()
                .any(|fill| dark.contains(fill) && !light.contains(fill)),
            "the frame still carries a colour only the dark palette has"
        );
    }

    #[test]
    // egui 0.34 flags `Context::run` / `CentralPanel::show`; they still drive a
    // CPU frame, which is exactly what this test needs (as the paint tests
    // above do).
    #[allow(deprecated)]
    fn the_painted_band_spells_out_the_section_class() {
        // The end of the path the unit tests only cover in pieces: resolve a
        // real scene, run one CPU frame, and read back what the painter actually
        // drew. A colour mapping that returns the right ink is worth nothing if
        // the glyph never reaches a shape.
        let mut app = demo_app();
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(
                egui::pos2(0.0, 0.0),
                egui::vec2(1200.0, 600.0),
            )),
            ..Default::default()
        };
        let output = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| app.paint(ui));
        });

        let painted = painted_glyphs(&output.shapes);
        assert!(
            painted.contains("SEC"),
            "the band's gutter header never reached the painter: {painted:?}"
        );
        let classes = ["Riff", "Breakdown", "Solo", "Clean", "Unknown"];
        assert!(
            classes.iter().any(|class| painted.contains(class)),
            "the band painted no class label at all: {painted:?}"
        );
    }

    #[test]
    fn all_mapped_keys_resolve_to_their_intent() {
        use Intent::{
            Home, NextSection, PitchDown, PitchUp, PrevSection, Quit, ScrollLeft, ScrollRight,
            ToggleInspector, TogglePlay, ZoomIn, ZoomOut,
        };
        let mapped = [
            (Key::Space, TogglePlay),
            (Key::ArrowLeft, ScrollLeft),
            (Key::ArrowRight, ScrollRight),
            (Key::ArrowUp, PitchUp),
            (Key::ArrowDown, PitchDown),
            (Key::Plus, ZoomIn),
            (Key::Equals, ZoomIn),
            (Key::Minus, ZoomOut),
            (Key::OpenBracket, PrevSection),
            (Key::CloseBracket, NextSection),
            (Key::Home, Home),
            (Key::Num0, Home),
            (Key::I, ToggleInspector),
            (Key::Q, Quit),
            (Key::Escape, Quit),
        ];
        for (key, intent) in mapped {
            assert_eq!(
                key_to_intent(key),
                Some(intent),
                "{key:?} should map to {intent:?}"
            );
        }
        for key in [Key::F1, Key::A, Key::Tab, Key::Enter] {
            assert_eq!(key_to_intent(key), None, "{key:?} should be inert");
        }
    }

    #[test]
    #[allow(deprecated)] // egui 0.34 flags CentralPanel::show; it still drives a CPU frame.
    fn paints_a_resolved_scene_headlessly() {
        use eframe::egui;
        use griff_ui_core::{Lane, NoteRect, Section};

        let view = PianoRollView {
            ppq: 480,
            tick_start: 0,
            tick_end: 3840,
            low_pitch: 52,
            high_pitch: 64,
            bar_lines: vec![0, 1920, 3840],
            lanes: vec![Lane {
                name: "lead".to_owned(),
                notes: vec![
                    NoteRect {
                        onset: 0,
                        end: 480,
                        pitch: 60,
                    },
                    NoteRect {
                        onset: 960,
                        end: 1440,
                        pitch: 64,
                    },
                ],
            }],
            tempo_bpm: 120.0,
            bar_count: 2,
        };
        let analysis = Analysis {
            focus_track: 0,
            sections: vec![Section {
                class: BarClass::Riff,
                bar_start: 0,
                bar_end: 2,
                tick_start: 0,
                tick_end: 3840,
            }],
            metrics: None,
            complexity: None,
            boundaries: vec![],
        };
        let mut app = CockpitApp::new(view, analysis, "headless".to_owned());

        // One egui frame on the CPU — no window, no GPU: the paint pass
        // tessellates the resolved scene into draw shapes.
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(
                egui::pos2(0.0, 0.0),
                egui::vec2(800.0, 400.0),
            )),
            ..Default::default()
        };
        let output = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| app.paint(ui));
        });

        assert!(
            output.shapes.len() > 50,
            "expected the cockpit to paint the scene's cells, got {} shapes",
            output.shapes.len()
        );
    }

    #[test]
    fn the_baked_demo_score_imports_with_notes_and_sections() {
        use griff_core::import::import_score_auto;
        use griff_ui_core::{analyze, build_view};

        // Exactly the bytes the wasm `start` bakes in (assets/demo.mid). Proving
        // it imports and resolves here — headlessly, on native — means the
        // browser demo (Slice 2) never trips its `expect`, and paints a real
        // score (notes + a classified section band), not an empty grid.
        let score = import_score_auto(include_bytes!("../assets/demo.mid"))
            .expect("the baked demo score must import");
        let view = build_view(&score);
        let analysis = analyze(&score);

        assert!(
            view.lanes.iter().any(|lane| !lane.notes.is_empty()),
            "the demo must carry notes for the web front to paint"
        );
        assert!(
            !analysis.sections.is_empty(),
            "the demo must analyse into sections for the classification band"
        );
    }

    // ── input path: keys → intents → viewport (the cockpit's own wiring) ──────

    fn key_event(key: Key) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::default(),
        }
    }

    /// Feeds one key press through a real egui frame into the app's input
    /// handler (exactly the path `eframe::App::ui` drives); returns whether the
    /// app asked to quit.
    #[allow(deprecated)] // egui 0.34 flags `Context::run`; it still drives a CPU frame.
    fn press(app: &mut CockpitApp, key: Key) -> bool {
        let ctx = egui::Context::default();
        let raw = egui::RawInput {
            events: vec![key_event(key)],
            ..Default::default()
        };
        let mut quit = false;
        let _frame = ctx.run(raw, |ctx| quit = app.handle_input(ctx));
        quit
    }

    fn demo_app() -> CockpitApp {
        use griff_core::import::import_score_auto;
        let score = import_score_auto(include_bytes!("../assets/demo.mid")).expect("demo imports");
        CockpitApp::from_score(score, "demo".to_owned())
    }

    /// A `bars`-bar 4/4 score at 960 ppq (3840 ticks/bar), one note-less track —
    /// `build_view` takes bar lines and the end from the master bars alone, so
    /// this exercises the loop/transport geometry without note fixtures.
    fn n_bar_score(bars: u32) -> Score {
        use griff_core::event::{Tempo, Ticks, TimeSignature, Tuning};
        use griff_core::score::{LossReport, MasterBar, RepeatMarker, Track, Voice};
        use griff_core::slice::TickRange;
        const BAR: u32 = 3840;
        let master_bars = (0..bars)
            .map(|i| MasterBar {
                index: i as usize,
                tick_range: TickRange::new(Ticks(i * BAR), Ticks((i + 1) * BAR)).expect("range"),
                time_signature: TimeSignature::new(4, 4).expect("4/4"),
                tempo: Tempo::new(120.0).expect("120"),
                repeat: RepeatMarker::default(),
            })
            .collect();
        let track = Track {
            name: None,
            channel: 0,
            voices: vec![Voice {
                id: 0,
                event_groups: Vec::new(),
            }],
            tuning: Tuning::standard_e(),
        };
        Score {
            ticks_per_quarter: 960,
            master_bars,
            tracks: vec![track],
            source_meta: None,
            loss: LossReport::new(),
        }
    }

    // ── S8 Slice 2: transport ────────────────────────────────────────────────

    #[test]
    fn a_looped_frame_that_laps_plays_tail_then_full_then_remainder() {
        // #125 correctness: a frame that spans more than one loop length must
        // play the tail, then a FULL revolution, then the remainder — the old
        // advance-then-modulo dropped the middle lap (right coordinate, wrong
        // music). head 150, a 300-tick frame, loop [0,200).
        let tempo = TempoMap::single(120.0);
        let ppq = 480;
        let tps = ticks_per_second(ppq, 120.0, 1.0);
        let (steps, resume) = plan_loop(150.0, 300.0 / tps, 0, 200, &tempo, ppq, 1.0);
        assert_eq!(
            steps,
            vec![
                LoopStep::PlayTo(200), // 150 → 200 tail
                LoopStep::Wrap(0),
                LoopStep::PlayTo(200), // 0 → 200 the full revolution, not dropped
                LoopStep::Wrap(0),
                LoopStep::PlayTo(50), // 0 → 50 remainder
            ],
        );
        assert_eq!(resume as u32, 50, "and the head resumes at 50");
    }

    #[test]
    fn a_looped_frame_ignores_the_tempo_past_the_loop_end() {
        // #125 correctness: a 1000-BPM segment begins exactly at the loop end.
        // The wrapped spans use the in-loop 120 BPM, so the plan is identical to
        // the flat-tempo case — the tempo past `hi` is never consulted.
        let bent = TempoMap::new(vec![(0, 120.0), (200, 1000.0)]);
        let ppq = 480;
        let tps = ticks_per_second(ppq, 120.0, 1.0);
        let (steps, resume) = plan_loop(150.0, 300.0 / tps, 0, 200, &bent, ppq, 1.0);
        assert_eq!(
            steps,
            vec![
                LoopStep::PlayTo(200),
                LoopStep::Wrap(0),
                LoopStep::PlayTo(200),
                LoopStep::Wrap(0),
                LoopStep::PlayTo(50),
            ],
            "the wrap uses the loop-start tempo, not the fast tail",
        );
        assert_eq!(resume as u32, 50);
    }

    #[test]
    fn a_looped_frame_wraps_a_head_that_sits_past_the_loop_end() {
        // A loop set while the head is past its end wraps in first, then plays.
        let tempo = TempoMap::single(120.0);
        let (steps, _) = plan_loop(500.0, 0.0, 0, 200, &tempo, 480, 1.0);
        assert_eq!(
            steps.first(),
            Some(&LoopStep::Wrap(0)),
            "the stray head wraps in"
        );
    }

    #[test]
    fn a_looped_frame_is_bounded_for_an_absurd_dt() {
        // A pathological dt does not spin forever — it takes a bounded hitch and
        // lands at the loop start.
        let tempo = TempoMap::single(120.0);
        let (steps, resume) = plan_loop(0.0, 1_000_000.0, 0, 200, &tempo, 480, 1.0);
        assert!(
            steps.len() <= 2 * MAX_LOOP_WRAPS + 1,
            "bounded, not unbounded"
        );
        assert_eq!(resume as u32, 0, "and settles at the loop start");
    }

    #[test]
    fn remap_loop_range_keeps_present_bars_and_clamps_to_the_end() {
        // Bar lines of a 3-bar score, end 11520.
        let lines = [0_u32, 3840, 7680, 11520];
        assert_eq!(
            remap_loop_range(1, 1, &lines, 11520),
            Some((3840, 7680)),
            "bar 1 maps to its tick span",
        );
        assert_eq!(
            remap_loop_range(0, 2, &lines, 11520),
            Some((0, 11520)),
            "the whole range stays whole",
        );
    }

    #[test]
    fn remap_loop_range_clears_a_range_the_new_view_lacks() {
        // A 1-bar score: lines [0, 3840], end 3840.
        let lines = [0_u32, 3840];
        assert_eq!(
            remap_loop_range(1, 1, &lines, 3840),
            None,
            "bar 1 no longer exists — the loop is cleared",
        );
        assert_eq!(
            remap_loop_range(0, 0, &lines, 3840),
            Some((0, 3840)),
            "bar 0 clamps to the one bar that remains",
        );
        // A never-past-the-end guard, even if a stray line exceeds the score.
        assert_eq!(
            remap_loop_range(0, 5, &[0, 9999], 3840),
            None,
            "hi must fit"
        );
    }

    #[test]
    fn switching_to_a_shorter_score_clamps_or_clears_the_loop() {
        // #125 initial-review correctness thread: on a candidate/score switch,
        // focus_on_track must revalidate the loop against the NEW view — never
        // leave an absolute range that runs past the shorter score's end and
        // graze silently there.
        let mut app = CockpitApp::from_score(n_bar_score(2), "A".to_owned());
        app.set_loop_bars(true, 1, 1); // loop the SECOND bar of the 2-bar score
        let (_, hi_a) = app.loop_range.expect("loop set on A");
        assert!(hi_a > 3840, "the loop lives in A's second bar");

        app.vp.play_tick = 5000; // head inside A's second bar
        app.vp.playing = true;
        app.show_score(n_bar_score(1), "B".to_owned()); // switch to a 1-bar score

        let end = app.ctx.tick_end;
        assert!(end <= 3840, "B is one bar — a shorter score");
        if let Some((lo, hi)) = app.loop_range {
            assert!(
                lo < hi && hi <= end,
                "a kept loop is clamped inside B: ({lo},{hi}) end={end}",
            );
            assert!(
                app.vp.play_tick >= lo && app.vp.play_tick < hi,
                "the head sits inside the remapped loop",
            );
        }
        // Playback never wanders past the new score's end.
        for _ in 0..50 {
            app.advance_audio(0.05);
            assert!(
                app.vp.play_tick <= end,
                "the playhead never exceeds B's end",
            );
        }
    }

    #[test]
    fn playback_bends_at_a_master_timeline_tempo_change() {
        // #125 re-review 1: the playhead follows the master timeline's tempo,
        // so the half after a tempo change advances at the new rate — not the
        // whole score at the first bar's BPM.
        let mut app = demo_app();
        app.ctx.ppq = 480;
        app.ctx.tick_start = 0;
        app.ctx.tick_end = 1_000_000; // room to run without reaching the end
        app.tempo_map = TempoMap::new(vec![(0, 120.0), (4800, 240.0)]);
        app.vp.playing = true;

        // 0.1 s inside the 120-BPM segment (480 ppq → 960 tick/s → 96 ticks).
        app.vp.play_tick = 0;
        app.play_pos = 0.0;
        app.advance_audio(0.1);
        let slow = app.vp.play_tick;

        // 0.1 s inside the 240-BPM segment (→ 1920 tick/s → 192 ticks).
        app.vp.play_tick = 4800;
        app.play_pos = f64::from(4800);
        app.advance_audio(0.1);
        let fast = app.vp.play_tick - 4800;

        assert!(slow > 0, "the slow segment moves");
        assert!(
            fast >= slow * 2 - 1 && fast <= slow * 2 + 1,
            "the doubled tempo advances ~2x as far: slow={slow} fast={fast}",
        );
    }

    #[test]
    fn a_thousand_sub_tick_frames_do_not_drift() {
        // #125 re-review 1: the fractional accumulator neither stalls nor gains
        // a phantom tick over many frames each shorter than a single tick — the
        // old `step.max(1)` nudge would have raced far ahead.
        let mut app = demo_app();
        app.ctx.ppq = 480;
        app.ctx.tick_start = 0;
        app.ctx.tick_end = 1_000_000;
        app.tempo_map = TempoMap::single(1.0); // 1 BPM at 480 ppq → 8 tick/s
        app.vp.play_tick = 0;
        app.play_pos = 0.0;
        app.vp.playing = true;
        for _ in 0..1000 {
            app.advance_audio(0.001); // 1000 × 1 ms = 1 s of travel = 8 ticks
        }
        assert_eq!(
            app.vp.play_tick, 8,
            "a second of sub-tick frames sums to exactly 8 ticks, no drift",
        );
    }

    #[test]
    fn playback_advances_then_stops_and_silences_at_the_end() {
        let mut app = demo_app();
        app.vp.playing = true;
        app.advance_audio(0.05);
        assert!(app.vp.play_tick > 0, "the playhead moves while playing");
        app.advance_audio(10_000.0); // far past the end
        assert_eq!(app.vp.play_tick, app.ctx.tick_end, "it lands on the end");
        assert!(!app.vp.playing, "and stops there");
        assert_eq!(app.player.active_count(), 0, "nothing left ringing");
    }

    #[test]
    fn looping_plays_the_tail_then_the_wrapped_remainder() {
        // #125 review 2: crossing the loop end must not jump straight to the
        // start — the overshoot past the boundary plays from the loop start,
        // not thrown away.
        let mut app = demo_app();
        app.loop_range = Some((0, 200)); // a tight loop, in ticks
        app.vp.play_tick = 150;
        app.vp.playing = true;
        // A frame worth exactly 300 ticks: 150→450 crosses the 200 boundary by
        // 250, which wraps to 50 (250 mod 200) past the loop start.
        let tps = ticks_per_second(app.ctx.ppq, app.ctx.tempo_bpm, 1.0);
        app.advance_audio(300.0 / tps);
        assert_eq!(
            app.vp.play_tick, 50,
            "the remainder plays from the loop start, not lost",
        );
        assert!(app.vp.playing, "a loop never stops on its own");
    }

    #[test]
    fn stop_rewinds_but_keeps_the_audition_setup() {
        // #125 review UX: Stop is not Reset — tempo, loop, and A/B survive it;
        // only a fresh file or a new generation clears them.
        let mut app = demo_app();
        app.vp.play_tick = 500;
        app.tempo_scale = 2.0;
        app.set_loop_bars(true, 0, 0);
        app.ab_other = Some(AuditionCandidate::Generate(3));
        app.stop_playback();
        assert_eq!(app.vp.play_tick, app.ctx.tick_start, "rewound");
        assert!((app.tempo_scale - 2.0).abs() < 1e-9, "tempo kept");
        assert!(app.loop_range.is_some(), "loop kept");
        assert_eq!(
            app.ab_other,
            Some(AuditionCandidate::Generate(3)),
            "A/B kept"
        );

        app.reset_audition();
        assert!(
            (app.tempo_scale - 1.0).abs() < 1e-9,
            "reset clears the tempo"
        );
        assert!(
            app.loop_range.is_none() && app.ab_other.is_none(),
            "and the loop + A/B",
        );
    }

    #[test]
    fn switching_the_output_hushes_the_old_port() {
        // #125 review 3: a port switch silences the old connection before it
        // is dropped, so no note is stranded ringing on it.
        let mut app = demo_app();
        app.gen_panel.variants = 2;
        app.do_generate();
        app.vp.playing = true;
        app.advance_audio(0.2); // sound some notes
        app.connect_device(0); // switch output (an absent port is fine)
        assert_eq!(
            app.player.active_count(),
            0,
            "the old port is hushed on the switch",
        );
    }

    #[test]
    fn the_audition_tempo_never_changes_the_score() {
        let mut app = demo_app();
        let written = app.base_bpm();
        app.set_playback_bpm(written * 2.0);
        assert!((app.tempo_scale - 2.0).abs() < 1e-6, "2x scale");
        assert!(
            (app.ctx.tempo_bpm - written).abs() < 1e-9,
            "the score's own tempo is untouched — audition only",
        );
    }

    #[test]
    fn a_candidate_switch_keeps_the_playhead_and_rebuilds_the_voice() {
        let mut app = demo_app();
        app.gen_panel.variants = 2;
        app.do_generate();
        app.vp.play_tick = 300;
        app.vp.playing = true;
        // Switch to another candidate mid-playback.
        app.show_candidate(1);
        assert_eq!(
            app.vp.play_tick, 300,
            "the playhead holds across the switch"
        );
        assert_eq!(
            app.player.active_count(),
            0,
            "the old score's notes are silenced"
        );
    }

    #[test]
    fn ab_swaps_between_the_last_two_candidates_without_regenerating() {
        let mut app = demo_app();
        app.gen_panel.variants = 2;
        app.do_generate(); // shows candidate 0
        assert_eq!(app.gen_panel.selected, Some(0));
        app.show_candidate(1); // now B; A (0) is remembered
        assert_eq!(app.ab_other, Some(AuditionCandidate::Generate(0)));
        app.ab_swap();
        assert_eq!(
            app.gen_panel.selected,
            Some(0),
            "A/B returns to the other one"
        );
        app.ab_swap();
        assert_eq!(app.gen_panel.selected, Some(1), "and back again — a toggle");
    }

    /// A generated run whose chain was refused. Generation succeeded — the
    /// candidates are all there and all playable; only the chaining of them did
    /// not happen.
    ///
    /// Staged rather than provoked: every candidate of a real run descends from
    /// one generator and one import, so they always agree about the timeline and
    /// nothing the panel can ask for produces a genuine refusal. The staging is
    /// therefore a whole run — a fresh run id, its set, its refused outcome
    /// recorded against it, and its intact winner recorded under it — because a
    /// run whose captured chain says one thing and whose history says another is
    /// not a state the code can reach, and a fixture that builds one tests
    /// nothing that exists.
    fn app_with_refused_chain() -> (CockpitApp, ChainError) {
        let refusal = ChainError::CrossBarMaterial {
            candidate: 2,
            bar: 1,
        };
        let mut app = demo_app();
        app.gen_panel.variants = 2;
        app.do_generate();
        let run = app.history.begin_run();
        {
            let active = app.gen_panel.active.as_mut().expect("a run");
            active.context.run = run;
            active.chain = GlobalChainOutcome::Refused(refusal);
        }
        app.history
            .record_chain(run, ChainOutcomeRecord::Refused { error: refusal });
        app.show_candidate(0);
        (app, refusal)
    }

    #[test]
    fn a_refused_chain_leaves_the_intact_audition_available() {
        // The S6 winner is not downstream of the chain. A set that cannot be
        // chained is still a set of candidates someone wants to hear.
        let (mut app, _) = app_with_refused_chain();
        assert!(
            !app.gen_panel
                .active
                .as_ref()
                .expect("a run")
                .set
                .rows
                .is_empty(),
            "generate succeeded — the refusal is about chaining what it made",
        );
        app.show_candidate(0);
        assert_eq!(app.gen_panel.selected, Some(0), "the intact winner shows");
        assert_eq!(
            app.current,
            Some(AuditionCandidate::Generate(0)),
            "and is the audition",
        );
        assert!(
            !app.player.is_silent(),
            "and it plays: the S6 path is untouched by the chain's absence",
        );
    }

    #[test]
    fn a_refused_chain_is_never_auditioned_as_an_empty_chain() {
        // Neither a silent score, nor the intact winner wearing the chain's
        // name. Asking for a chain that does not exist gets nothing at all.
        let (mut app, _) = app_with_refused_chain();
        app.show_candidate(0);
        let before = app.history.entries().len();
        app.show_global_chain();
        assert_eq!(
            app.history.entries().len(),
            before,
            "nothing was recorded — there was no result to record",
        );
        assert!(
            !app.history
                .entries()
                .iter()
                .any(|e| e.source == CandidateSource::GlobalChain),
            "and no entry claims to be a chain",
        );
        assert_eq!(
            app.current,
            Some(AuditionCandidate::Generate(0)),
            "the intact winner is still what is playing",
        );
    }

    #[test]
    fn a_refused_chain_keeps_its_typed_error_on_the_run() {
        // Kept typed, on the run that produced it — not flattened to a string
        // at the moment of failure, and not recomputed later by asking the
        // planner to try again.
        let (app, refusal) = app_with_refused_chain();
        let GlobalChainOutcome::Refused(error) =
            &app.gen_panel.active.as_ref().expect("a run").chain
        else {
            panic!("staged as refused");
        };
        assert_eq!(*error, refusal, "the exact typed error the core returned");
    }

    #[test]
    fn a_refusal_explains_itself_without_a_debug_dump() {
        // A structured sentence built from the typed value — the same rule
        // provenance_summary follows. `{err:?}` is a Rust value printed at a
        // human: it names private-looking variants and braces, and it is not an
        // explanation of anything.
        let summary = chain_refusal_summary(ChainError::CrossBarMaterial {
            candidate: 2,
            bar: 1,
        });
        assert!(
            !summary.contains('{') && !summary.contains("CrossBarMaterial"),
            "not a debug dump: {summary}",
        );
        assert!(
            summary.contains("candidate 2") && summary.contains("bar 1"),
            "but it still names the offending fact: {summary}",
        );
        assert!(
            summary.to_lowercase().contains("bar line"),
            "in words that say what went wrong: {summary}",
        );
    }

    #[test]
    fn every_refusal_reason_has_a_sentence() {
        // No reason may fall through to a debug dump. If the core gains an
        // error, this fails until someone says what it means.
        let reasons = [
            ChainError::EmptySet,
            ChainError::NoBars,
            ChainError::BarCountMismatch {
                candidate: 1,
                expected: 4,
                found: 3,
            },
            ChainError::PpqMismatch {
                candidate: 1,
                expected: 960,
                found: 480,
            },
            ChainError::MasterBarMismatch {
                candidate: 1,
                bar: 2,
                field: MasterBarField::Tempo,
            },
            ChainError::TrackCountMismatch {
                candidate: 1,
                expected: 1,
                found: 2,
            },
            ChainError::TrackMetadataMismatch {
                candidate: 1,
                track: 0,
                field: TrackField::Tuning,
            },
            ChainError::SourceMetaMismatch { candidate: 1 },
            ChainError::LossReportMismatch { candidate: 1 },
            ChainError::CrossBarMaterial {
                candidate: 1,
                bar: 0,
            },
            ChainError::EmptyEventGroup { candidate: 1 },
            ChainError::MaterialOutsideTimeline {
                candidate: 1,
                tick: 99,
            },
            ChainError::MissingMaterial {
                candidate: 1,
                track: 0,
                voice: 0,
                bar: 3,
            },
            ChainError::BoundaryFact(TransitionFactError::MissingToBar { bar: 9, bars: 4 }),
            ChainError::Path(PathError::NoLayers),
        ];
        for reason in reasons {
            let summary = chain_refusal_summary(reason);
            assert!(
                !summary.is_empty() && !summary.contains('{'),
                "{reason:?} has no sentence: {summary}",
            );
        }
    }

    #[test]
    fn switching_to_the_global_chain_silences_the_old_source_and_keeps_the_playhead() {
        // The Slice 2 contract, inherited: one sounding source at a time, and
        // the comparison lands at the same spot in the bar.
        let mut app = demo_app();
        app.gen_panel.variants = 2;
        app.do_generate();
        app.vp.play_tick = 300;
        app.vp.playing = true;
        app.advance_audio(0.2); // this moves the playhead — the switch must not
        let tick = app.vp.play_tick;
        app.show_global_chain();
        assert_eq!(
            app.vp.play_tick, tick,
            "the playhead holds across the variant switch",
        );
        assert_eq!(
            app.player.active_count(),
            0,
            "the intact winner's notes are silenced — no leak into the chain",
        );
        assert_eq!(
            app.current,
            Some(AuditionCandidate::GlobalChain),
            "and exactly one source is active",
        );
    }

    #[test]
    fn ab_alternates_between_the_intact_winner_and_the_chain() {
        let mut app = demo_app();
        app.gen_panel.variants = 2;
        app.do_generate(); // shows Generate(0)
        app.show_global_chain(); // leaving Generate(0)
        assert_eq!(
            app.ab_other,
            Some(AuditionCandidate::Generate(0)),
            "the intact winner we left is the A/B target",
        );
        app.ab_swap();
        assert_eq!(app.current, Some(AuditionCandidate::Generate(0)));
        assert_eq!(
            app.ab_other,
            Some(AuditionCandidate::GlobalChain),
            "and the chain is now the other",
        );
        app.ab_swap();
        assert_eq!(
            app.current,
            Some(AuditionCandidate::GlobalChain),
            "b routes back to the chain, not to a same-index row",
        );
    }

    #[test]
    fn switching_variants_does_not_grow_the_history() {
        // A/B is a comparison, not an event. Each variant is one result, and
        // re-hearing it is the same result — the (run, candidate_id) key says
        // so. A history that grows on every keypress is a log, not a record.
        let mut app = demo_app();
        app.gen_panel.variants = 2;
        app.do_generate();
        app.show_global_chain();
        let after_both = app.history.entries().len();
        assert_eq!(after_both, 2, "the intact winner and the chain");
        for _ in 0..5 {
            app.ab_swap();
        }
        assert_eq!(
            app.history.entries().len(),
            after_both,
            "five swaps recorded nothing new",
        );
    }

    #[test]
    fn the_loop_and_a_seek_survive_a_switch_to_the_chain() {
        // Both variants stand on the master timeline every candidate agreed on,
        // so a loop over bar 2 is the same bar 2 in either.
        let mut app = demo_app();
        app.gen_panel.variants = 2;
        app.do_generate();
        let bars = app.view.bar_lines.clone();
        assert!(bars.len() >= 3, "the demo has bars to loop over");
        app.loop_range = Some((bars[1], bars[2]));
        app.vp.play_tick = bars[1] + 10;
        app.show_global_chain();
        assert_eq!(
            app.loop_range,
            Some((bars[1], bars[2])),
            "the loop is the same span of the same timeline",
        );
        assert_eq!(app.vp.play_tick, bars[1] + 10, "and the seek holds");
    }

    #[test]
    fn switching_variants_while_stopped_does_not_start_playing() {
        let mut app = demo_app();
        app.gen_panel.variants = 2;
        app.do_generate();
        app.vp.playing = false;
        app.show_global_chain();
        assert!(!app.vp.playing, "auditioning is not playing");
        assert_eq!(app.player.active_count(), 0, "and nothing sounds");
    }

    // ── the reducer refuses, not just the button ─────────────────────────────

    #[test]
    fn a_refused_chain_cannot_be_activated_by_an_action() {
        // A disabled button is not a type system. The action must be refused
        // where it is applied, or a stale frame, a keybinding, or a future
        // caller reaches a result that does not exist.
        let (mut app, _) = app_with_refused_chain();
        app.show_candidate(0);
        app.vp.playing = true;
        app.advance_audio(0.2);
        let sounding = app.player.active_count();
        let tick = app.vp.play_tick;
        let entries = app.history.entries().len();

        app.apply_generate_actions(&GenerateActions {
            show: Some(AuditionPick::Chain),
            ..GenerateActions::default()
        });

        assert_eq!(
            app.current,
            Some(AuditionCandidate::Generate(0)),
            "the active source is still the intact winner",
        );
        assert_eq!(
            app.player.active_count(),
            sounding,
            "and playback was not reset — the source never changed",
        );
        assert_eq!(app.vp.play_tick, tick, "nor the playhead");
        assert_eq!(
            app.history.entries().len(),
            entries,
            "and nothing was recorded",
        );
    }

    #[test]
    fn ab_cannot_route_to_a_refused_chain() {
        // The stale-target case: an A/B target pointing at a chain the run does
        // not have. Nothing happens, rather than something empty happening.
        let (mut app, _) = app_with_refused_chain();
        app.show_candidate(0);
        app.ab_other = Some(AuditionCandidate::GlobalChain);
        app.ab_swap();
        assert_eq!(
            app.current,
            Some(AuditionCandidate::Generate(0)),
            "the intact winner is still what is playing",
        );
        assert!(
            !app.history
                .entries()
                .iter()
                .any(|e| e.source == CandidateSource::GlobalChain),
            "and no chain entry was invented",
        );
    }

    // ── the panel says what is actually sounding ─────────────────────────────

    #[test]
    fn the_intact_variant_action_returns_to_ranked_candidate_0() {
        // "S6 Intact" means ranked candidate 0 — the whole candidate S6 put
        // first, and the one `baseline_cost` measures. Not "whichever row was
        // last clicked".
        let mut app = demo_app();
        app.gen_panel.variants = 2;
        app.do_generate();
        app.show_candidate(1); // browse another row
        app.show_global_chain();
        app.apply_generate_actions(&GenerateActions {
            show: Some(AuditionPick::Intact),
            ..GenerateActions::default()
        });
        assert_eq!(
            app.current,
            Some(AuditionCandidate::Generate(0)),
            "the intact winner is the baseline's candidate, not the last browsed row",
        );
    }

    #[test]
    fn the_candidate_table_stops_marking_a_row_while_the_chain_plays() {
        // The table's highlight is a claim about what is sounding. While the
        // chain plays, no row is: the chain is made of bars from several of
        // them, and pointing at one would name a supplier as the whole result.
        let mut app = demo_app();
        app.gen_panel.variants = 2;
        app.do_generate();
        assert_eq!(app.gen_panel.selected, Some(0), "the winner is marked");
        app.show_global_chain();
        assert_eq!(
            app.gen_panel.selected, None,
            "no row is the audition while the chain is",
        );
        app.ab_swap();
        assert_eq!(
            app.gen_panel.selected,
            Some(0),
            "and the mark comes back with the row",
        );
    }

    #[test]
    fn the_chain_replays_from_history_after_a_later_generate() {
        // Source-aware replay: the chain's snapshot outlives the run that made
        // it, because history holds the score rather than a way to rebuild it.
        let mut app = demo_app();
        app.gen_panel.variants = 2;
        app.do_generate();
        app.show_global_chain();
        let chain_entry = app
            .history
            .entries()
            .iter()
            .find(|e| e.source == CandidateSource::GlobalChain)
            .expect("the chain was recorded")
            .id;
        let notes = app
            .history
            .get(chain_entry)
            .expect("entry")
            .score
            .master_bars
            .len();

        app.gen_panel.seed = 31_337;
        app.do_generate(); // a whole new run
        app.select_history(chain_entry);

        assert_eq!(
            app.history
                .get(chain_entry)
                .expect("entry")
                .score
                .master_bars
                .len(),
            notes,
            "the old chain snapshot is untouched by a later run",
        );
        assert_eq!(
            app.current,
            Some(AuditionCandidate::History(chain_entry)),
            "and it is what is playing",
        );
    }

    /// The summary of `run`'s chain — for building expectations, never for
    /// asking the panel what it shows.
    fn summary_of(app: &CockpitApp, run: GenerationRunId) -> GlobalChainSummary {
        app.history
            .chain_of(run)
            .and_then(global_chain_summary)
            .expect("the run planned a chain, so it has an explanation")
    }

    #[test]
    fn history_replay_shows_the_replayed_chains_explanation() {
        // The panel must explain what is *sounding*. Replaying chain A while
        // run B is active and reading B's record would put A's music beside B's
        // supplier map — every number true, and the pairing a lie.
        let mut app = demo_app();
        app.gen_panel.variants = 2;
        app.gen_panel.seed = 11;
        app.do_generate();
        let run_a = app.gen_panel.active.as_ref().expect("a run").context.run;
        app.show_global_chain();
        let chain_a = app
            .history
            .entries()
            .iter()
            .find(|e| e.source == CandidateSource::GlobalChain)
            .expect("recorded")
            .id;
        let summary_a = summary_of(&app, run_a);

        app.gen_panel.seed = 22;
        app.do_generate();
        let run_b = app.gen_panel.active.as_ref().expect("a run").context.run;
        let summary_b = summary_of(&app, run_b);
        assert_ne!(
            summary_a, summary_b,
            "the fixture needs two runs that chain differently — pick other seeds",
        );

        app.select_history(chain_a);
        let shown = app
            .displayed_chain_summary()
            .expect("the sounding chain has an explanation");
        assert_eq!(shown, summary_a, "the replayed chain's own explanation");
        assert_ne!(shown, summary_b, "not the active run's");
    }

    #[test]
    fn a_replayed_chain_explains_itself_even_when_the_active_run_refused() {
        // The sharper case: the active run has no chain at all. Reading the
        // active run would find a refusal and show nothing, so the chain that
        // is actually playing would go unexplained.
        let mut app = demo_app();
        app.gen_panel.variants = 2;
        app.gen_panel.seed = 11;
        app.do_generate();
        let run_a = app.gen_panel.active.as_ref().expect("a run").context.run;
        app.show_global_chain();
        let chain_a = app
            .history
            .entries()
            .iter()
            .find(|e| e.source == CandidateSource::GlobalChain)
            .expect("recorded")
            .id;
        let summary_a = summary_of(&app, run_a);

        // Run B: generated, but staged as a run whose chain was refused.
        app.gen_panel.seed = 22;
        app.do_generate();
        let run_b = app.history.begin_run();
        {
            let active = app.gen_panel.active.as_mut().expect("a run");
            active.context.run = run_b;
            active.chain = GlobalChainOutcome::Refused(ChainError::EmptySet);
        }
        app.history.record_chain(
            run_b,
            ChainOutcomeRecord::Refused {
                error: ChainError::EmptySet,
            },
        );

        app.select_history(chain_a);
        assert_eq!(
            app.displayed_chain_summary(),
            Some(summary_a),
            "the playing chain explains itself, whatever the active run came to",
        );
    }

    #[test]
    fn the_live_chain_is_explained_by_the_active_run() {
        // The other side of the rule: nothing replayed, so the active run's
        // chain is the one on show.
        let mut app = demo_app();
        app.gen_panel.variants = 2;
        app.do_generate();
        let run = app.gen_panel.active.as_ref().expect("a run").context.run;
        assert_eq!(app.displayed_chain_summary(), Some(summary_of(&app, run)));
        app.show_global_chain();
        assert_eq!(
            app.displayed_chain_summary(),
            Some(summary_of(&app, run)),
            "auditioning the live chain does not change whose explanation it is",
        );
    }

    #[test]
    fn replaying_a_candidate_still_explains_the_active_runs_chain() {
        // A replayed *candidate* is not a chain, so it says nothing about which
        // chain to explain: the active run's stands.
        let mut app = demo_app();
        app.gen_panel.variants = 2;
        app.do_generate();
        let run = app.gen_panel.active.as_ref().expect("a run").context.run;
        let candidate = app
            .history
            .entries()
            .iter()
            .find(|e| e.source == CandidateSource::Generate)
            .expect("recorded")
            .id;
        app.select_history(candidate);
        assert_eq!(app.displayed_chain_summary(), Some(summary_of(&app, run)));
    }

    #[test]
    fn an_equal_cost_delta_is_not_called_higher() {
        // The chain costing exactly what the intact winner costs is neither
        // lower nor higher, and a panel that says "higher" in confident
        // monospace is wrong in the one place it is trying hardest to be
        // trusted.
        assert_eq!(delta_relation(0.0), "equal");
        assert_eq!(delta_relation(-0.9), "lower");
        assert_eq!(delta_relation(0.9), "higher");
    }

    #[test]
    fn the_visible_explanation_comes_from_the_runs_record() {
        // Not from ActiveGenerateRun. The panel and a replayed history entry
        // must give the same answer about the same chain, and only one of them
        // has a live run to ask.
        let mut app = demo_app();
        app.gen_panel.variants = 2;
        app.do_generate();
        let run = app.gen_panel.active.as_ref().expect("a run").context.run;
        let summary = summary_of(&app, run);

        let GlobalChainOutcome::Planned(chain) =
            &app.gen_panel.active.as_ref().expect("a run").chain
        else {
            panic!("planned");
        };
        assert_eq!(
            summary.total_cost.to_bits(),
            chain.plan.total_cost.to_bits(),
            "the same number the capture holds, bit for bit",
        );
        assert_eq!(
            summary.baseline_cost.to_bits(),
            chain.baseline_cost.to_bits(),
        );
        assert_eq!(summary.bars.len(), app.gen_panel.bars, "one row per bar");
        assert_eq!(
            summary.boundaries.len(),
            app.gen_panel.bars.saturating_sub(1),
            "one row per bar line",
        );
    }

    #[test]
    fn an_old_chain_still_explains_itself_after_a_later_generate() {
        // The reason the trace lives on the run record. Replaying an old chain
        // must show that chain's own explanation, and by then its
        // ActiveGenerateRun is gone.
        let mut app = demo_app();
        app.gen_panel.variants = 2;
        app.gen_panel.seed = 11;
        app.do_generate();
        let run_a = app.gen_panel.active.as_ref().expect("a run").context.run;
        app.show_global_chain();
        let chain_a = app
            .history
            .entries()
            .iter()
            .find(|e| e.source == CandidateSource::GlobalChain)
            .expect("recorded")
            .id;
        let before = summary_of(&app, run_a);

        app.gen_panel.seed = 22;
        app.do_generate(); // run A's ActiveGenerateRun is gone
        app.select_history(chain_a);

        let entry_run = app.history.get(chain_a).expect("entry").run;
        assert_eq!(entry_run, run_a, "the entry knows which run made it");
        let after = summary_of(&app, entry_run);
        assert_eq!(
            after.total_cost.to_bits(),
            before.total_cost.to_bits(),
            "the old chain's explanation is the old chain's",
        );
        assert_eq!(
            after.bars.iter().map(|b| b.candidate).collect::<Vec<_>>(),
            before.bars.iter().map(|b| b.candidate).collect::<Vec<_>>(),
            "including its supplier map",
        );
    }

    #[test]
    fn moving_the_knobs_does_not_move_the_visible_explanation() {
        let mut app = demo_app();
        app.gen_panel.variants = 2;
        app.do_generate();
        let run = app.gen_panel.active.as_ref().expect("a run").context.run;
        let before = summary_of(&app, run);
        app.gen_panel.seed = 4_242;
        app.gen_panel.bars = 2;
        app.gen_panel.variants = 5;
        let after = summary_of(&app, run);
        assert_eq!(
            after, before,
            "the explanation belongs to the run, not the panel"
        );
    }

    #[test]
    fn a_refused_run_has_no_explanation_block_to_show() {
        let (app, _) = app_with_refused_chain();
        let run = app.gen_panel.active.as_ref().expect("a run").context.run;
        assert!(
            app.history
                .chain_of(run)
                .and_then(global_chain_summary)
                .is_none(),
            "nothing to explain, so nothing is drawn — not an empty block",
        );
    }

    #[test]
    fn a_runs_chain_outcome_is_recorded_against_the_run() {
        // Not against a candidate: a refusal has no score, and an entry needs
        // one. The run id is the link that survives the panel.
        let mut app = demo_app();
        app.gen_panel.variants = 2;
        app.do_generate();
        let run = app.gen_panel.active.as_ref().expect("a run").context.run;
        let Some(ChainOutcomeRecord::Planned {
            total_cost,
            baseline_cost,
            steps,
            policy_id,
            ..
        }) = app.history.chain_of(run)
        else {
            panic!(
                "the demo run planned a chain: {:?}",
                app.history.chain_of(run)
            );
        };
        assert_eq!(*policy_id, "candidate_chain");
        assert_eq!(steps.len(), app.gen_panel.bars, "one step per output bar");
        let GlobalChainOutcome::Planned(chain) =
            &app.gen_panel.active.as_ref().expect("a run").chain
        else {
            panic!("planned");
        };
        assert_eq!(
            total_cost.to_bits(),
            chain.plan.total_cost.to_bits(),
            "the recorded costs are the captured ones, bit for bit",
        );
        assert_eq!(baseline_cost.to_bits(), chain.baseline_cost.to_bits());
    }

    #[test]
    fn a_refused_runs_outcome_survives_a_later_generate() {
        // The whole reason the record is run-level. ActiveGenerateRun is
        // replaced by the next Generate; the reason run A had no chain must not
        // go with it.
        let (mut app, refusal) = app_with_refused_chain();
        let run_a = app.gen_panel.active.as_ref().expect("a run").context.run;

        app.gen_panel.seed = 24_601;
        app.do_generate(); // run B replaces the active run entirely
        let run_b = app.gen_panel.active.as_ref().expect("a run").context.run;
        assert_ne!(run_a, run_b, "a new run");

        let Some(ChainOutcomeRecord::Refused { error }) = app.history.chain_of(run_a) else {
            panic!(
                "run A's chain is still refused: {:?}",
                app.history.chain_of(run_a)
            );
        };
        assert_eq!(
            *error, refusal,
            "run A's refusal is still the typed error it always was",
        );
        assert!(
            matches!(
                app.history.chain_of(run_b),
                Some(&ChainOutcomeRecord::Planned { .. })
            ),
            "and run B recorded its own outcome",
        );
        assert!(
            !app.history
                .entries()
                .iter()
                .any(|e| e.run == run_a && e.source == CandidateSource::GlobalChain),
            "run A never got a chain entry — a refusal has no score to hang one on",
        );
        let intact_a = app
            .history
            .entries()
            .iter()
            .find(|e| e.run == run_a)
            .expect("run A's intact winner is still in history");
        assert!(
            !intact_a.score.master_bars.is_empty(),
            "and its music is untouched by run B",
        );
    }

    // ── export writes the snapshot, never a re-plan ──────────────────────────

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn the_chain_exports_the_captured_assembled_score() {
        use griff_core::midi::export_score;
        let mut app = demo_app();
        app.out_dir = env::temp_dir().join("griff-chain-export-test");
        app.gen_panel.variants = 2;
        app.do_generate();
        app.show_global_chain();
        let id = app
            .history
            .entries()
            .iter()
            .find(|e| e.source == CandidateSource::GlobalChain)
            .expect("the chain was recorded")
            .id;

        let path = app.write_chain_keep(id).expect("the chain exports");
        let written = fs::read(&path).expect("the file is there");
        let expected =
            export_score(&app.history.get(id).expect("entry").score).expect("the snapshot exports");
        assert_eq!(
            written, expected,
            "the bytes are the captured assembled score's, through the one canonical path",
        );
        drop(fs::remove_file(&path));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn exporting_an_old_chain_after_a_new_generate_writes_the_old_music() {
        // The immutability law. The UI moves on — a new run, new knobs, a
        // different candidate — and the old export is still the old music,
        // because it was never a recipe to re-derive, only a snapshot.
        use griff_core::midi::export_score;
        let mut app = demo_app();
        app.out_dir = env::temp_dir().join("griff-chain-export-old");
        app.gen_panel.variants = 2;
        app.do_generate();
        app.show_global_chain();
        let old = app
            .history
            .entries()
            .iter()
            .find(|e| e.source == CandidateSource::GlobalChain)
            .expect("recorded")
            .id;
        let old_bytes = export_score(&app.history.get(old).expect("entry").score).expect("exports");

        app.gen_panel.seed = 8_675_309;
        app.gen_panel.bars = 3;
        app.do_generate();
        app.show_candidate(1);

        let path = app
            .write_chain_keep(old)
            .expect("the old chain still exports");
        let written = fs::read(&path).expect("the file is there");
        assert_eq!(
            written, old_bytes,
            "the old entry exports the music it was recorded with",
        );
        drop(fs::remove_file(&path));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn exporting_the_chain_does_not_replan_it() {
        // The poisoned snapshot again, this time through the export path.
        let mut app = demo_app();
        app.out_dir = env::temp_dir().join("griff-chain-export-noreplan");
        app.gen_panel.variants = 2;
        app.do_generate();
        app.show_global_chain();
        let id = app
            .history
            .entries()
            .iter()
            .find(|e| e.source == CandidateSource::GlobalChain)
            .expect("recorded")
            .id;
        let poison = 4321.0_f64;
        {
            let run = app.gen_panel.active.as_mut().expect("a run");
            let GlobalChainOutcome::Planned(chain) = &mut run.chain else {
                panic!("planned");
            };
            chain.plan.total_cost = poison;
        }
        let path = app.write_chain_keep(id).expect("exports");
        let GlobalChainOutcome::Planned(chain) =
            &app.gen_panel.active.as_ref().expect("a run").chain
        else {
            panic!("planned");
        };
        assert_eq!(
            chain.plan.total_cost.to_bits(),
            poison.to_bits(),
            "export read the snapshot; it did not ask the planner to try again",
        );
        drop(fs::remove_file(&path));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn keeping_the_chain_while_s6_sounds_does_not_change_what_is_playing() {
        use griff_core::midi::export_score;
        // Export is a file action. It must not decide what the user is
        // listening to — pressing "keep" to get a .mid out should not swap the
        // audio out from under them mid-comparison.
        let mut app = demo_app();
        app.out_dir = env::temp_dir().join("griff-keep-no-side-effects");
        app.gen_panel.variants = 2;
        app.do_generate(); // S6 intact is showing and sounding
        app.show_candidate(1);
        app.show_candidate(0); // ab_other is Generate(1)
        app.vp.playing = true;
        app.advance_audio(0.3);
        let sounding = app.player.active_count();
        let tick = app.vp.play_tick;
        let selected_row = app.gen_panel.selected;
        let ab = app.ab_other;
        let selected_entry = app.history.selected();

        app.keep_chain();

        assert_eq!(
            app.current,
            Some(AuditionCandidate::Generate(0)),
            "the S6 winner is still the audition",
        );
        assert_eq!(app.player.active_count(), sounding, "still sounding");
        assert_eq!(app.vp.play_tick, tick, "the playhead did not move");
        assert!(app.vp.playing, "and it is still playing");
        assert_eq!(
            app.gen_panel.selected, selected_row,
            "the row is still marked"
        );
        assert_eq!(app.ab_other, ab, "and A/B still points where it did");
        assert_eq!(
            app.history.selected(),
            selected_entry,
            "the history selection did not move to the chain",
        );

        let chain = app
            .history
            .entries()
            .iter()
            .find(|e| e.source == CandidateSource::GlobalChain)
            .expect("the chain was recorded so the export has a result to name");
        assert!(
            app.gen_panel
                .status
                .as_deref()
                .is_some_and(|s| s.starts_with("kept -> ")),
            "and the file was written: {:?}",
            app.gen_panel.status,
        );
        let path = app
            .gen_panel
            .status
            .as_deref()
            .and_then(|s| s.strip_prefix("kept -> "))
            .expect("a path");
        let written = fs::read(path).expect("the file is there");
        let expected = export_score(&chain.score).expect("the snapshot exports");
        assert_eq!(
            written, expected,
            "the bytes are the chain's, not the S6 winner's"
        );
        drop(fs::remove_file(path));
        drop(fs::remove_file(path.replace(".mid", ".json")));
    }

    /// Generates a run with known knobs, auditions its chain, exports it, and
    /// returns the parsed sidecar beside the run's captured facts.
    #[cfg(not(target_arch = "wasm32"))]
    fn exported_chain_sidecar(dir: &str) -> (CockpitApp, HistoryId, serde_json::Value) {
        let mut app = demo_app();
        app.out_dir = env::temp_dir().join(dir);
        app.gen_panel.variants = 2;
        app.gen_panel.seed = 77;
        app.gen_panel.bars = 4;
        app.do_generate();
        app.show_global_chain();
        let id = app
            .history
            .entries()
            .iter()
            .find(|e| e.source == CandidateSource::GlobalChain)
            .expect("the chain was recorded")
            .id;
        let path = app.write_chain_keep(id).expect("exports");
        let json: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(path.replace(".mid", ".json")).expect("read"))
                .expect("the sidecar is json");
        drop(fs::remove_file(&path));
        drop(fs::remove_file(path.replace(".mid", ".json")));
        (app, id, json)
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn the_chain_sidecar_preserves_the_captured_run_contract() {
        // A sidecar exists to reproduce the export later, by someone who no
        // longer has the session. Every field of the run's captured contract
        // has to be in it, or "reproduce this" quietly means "regenerate
        // something and hope".
        let (app, _, json) = exported_chain_sidecar("griff-sidecar-contract");
        let run = app.gen_panel.active.as_ref().expect("a run").context.run;

        assert_eq!(json["origin"], "candidate_chain", "what this file is");
        assert_eq!(json["run"], run.0, "which run made it");
        assert!(json["source"].is_string(), "what seeded that run");
        assert_eq!(json["seed"], 77, "the ask seed");
        assert_eq!(json["bars"], 4, "the ask's bar count");
        assert_eq!(json["variants_per_strategy"], 2, "the ask's variant count");
        assert!(
            json["corpus"]["templates"].is_number()
                && json["corpus"]["references"].is_number()
                && json["corpus"]["gesture"].is_boolean(),
            "what the corpus actually contributed: {}",
            json["corpus"],
        );
        assert_eq!(json["policy_id"], "candidate_chain");
        assert_eq!(json["policy_version"], 1);
        assert!(json["baseline_cost"].is_number());
        assert!(json["total_cost"].is_number());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn the_chain_sidecars_bars_come_from_the_captured_ask() {
        // Not from suppliers.len(). They agree — and a law says so — but the
        // ask is a fact the run recorded, and deriving it from the result would
        // make a wrong result look self-consistent.
        let (app, id, json) = exported_chain_sidecar("griff-sidecar-bars");
        let GeneratorProvenance::GlobalChain {
            bars, suppliers, ..
        } = &app.history.get(id).expect("entry").provenance.generator
        else {
            panic!("a chain entry");
        };
        assert_eq!(json["bars"], *bars, "the sidecar's bars are the ask's");
        assert_eq!(
            *bars,
            suppliers.len(),
            "and the invariant holds: one supplier per asked bar",
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn the_chain_sidecar_keeps_the_costs_and_the_supplier_map_in_bar_order() {
        let (app, id, json) = exported_chain_sidecar("griff-sidecar-suppliers");
        let GeneratorProvenance::GlobalChain {
            suppliers,
            baseline_cost,
            total_cost,
            ..
        } = &app.history.get(id).expect("entry").provenance.generator
        else {
            panic!("a chain entry");
        };
        assert_eq!(
            json["baseline_cost"].as_f64().expect("a number").to_bits(),
            baseline_cost.to_bits(),
            "the captured baseline, through json, unchanged",
        );
        assert_eq!(
            json["total_cost"].as_f64().expect("a number").to_bits(),
            total_cost.to_bits(),
        );
        let written = json["suppliers"].as_array().expect("an array");
        assert_eq!(written.len(), suppliers.len());
        for (i, (got, want)) in written.iter().zip(suppliers.iter()).enumerate() {
            assert_eq!(got["bar"], want.bar, "supplier {i} is in output-bar order");
            assert_eq!(got["candidate"], want.candidate);
            assert_eq!(got["rank"], want.rank);
            assert_eq!(got["strategy"], want.strategy);
            assert_eq!(got["variant_seed"], want.variant_seed);
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn moving_the_knobs_after_a_run_does_not_move_its_sidecar() {
        // The sidecar describes the run that made the music, not the panel that
        // happens to be open when the file is written.
        let mut app = demo_app();
        app.out_dir = env::temp_dir().join("griff-sidecar-knobs");
        app.gen_panel.variants = 2;
        app.gen_panel.seed = 77;
        app.gen_panel.bars = 4;
        app.do_generate();
        app.show_global_chain();
        let id = app
            .history
            .entries()
            .iter()
            .find(|e| e.source == CandidateSource::GlobalChain)
            .expect("recorded")
            .id;

        app.gen_panel.seed = 5;
        app.gen_panel.bars = 9;
        app.gen_panel.variants = 7;

        let path = app.write_chain_keep(id).expect("exports");
        let json: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(path.replace(".mid", ".json")).expect("read"))
                .expect("json");
        assert_eq!(json["seed"], 77, "the run's seed, not the knob's");
        assert_eq!(json["bars"], 4, "the run's bars, not the knob's");
        assert_eq!(json["variants_per_strategy"], 2);
        drop(fs::remove_file(&path));
        drop(fs::remove_file(path.replace(".mid", ".json")));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn an_old_chains_sidecar_survives_a_later_generate() {
        let mut app = demo_app();
        app.out_dir = env::temp_dir().join("griff-sidecar-old");
        app.gen_panel.variants = 2;
        app.gen_panel.seed = 77;
        app.gen_panel.bars = 4;
        app.do_generate();
        app.show_global_chain();
        let old = app
            .history
            .entries()
            .iter()
            .find(|e| e.source == CandidateSource::GlobalChain)
            .expect("recorded")
            .id;
        let old_run = app.gen_panel.active.as_ref().expect("a run").context.run;

        app.gen_panel.seed = 999;
        app.do_generate(); // a whole new run

        let path = app
            .write_chain_keep(old)
            .expect("the old entry still exports");
        let json: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(path.replace(".mid", ".json")).expect("read"))
                .expect("json");
        assert_eq!(json["run"], old_run.0, "the old run's id");
        assert_eq!(json["seed"], 77, "and the old run's ask");
        drop(fs::remove_file(&path));
        drop(fs::remove_file(path.replace(".mid", ".json")));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn the_chain_sidecar_names_its_suppliers_and_both_costs() {
        // A sidecar that cannot say which candidate played which bar does not
        // reproduce the export; it just sits beside it looking official.
        let mut app = demo_app();
        app.out_dir = env::temp_dir().join("griff-chain-sidecar");
        app.gen_panel.variants = 2;
        app.do_generate();
        app.show_global_chain();
        let id = app
            .history
            .entries()
            .iter()
            .find(|e| e.source == CandidateSource::GlobalChain)
            .expect("recorded")
            .id;
        let path = app.write_chain_keep(id).expect("exports");
        let json = fs::read_to_string(path.replace(".mid", ".json")).expect("a sidecar");
        assert!(json.contains("\"suppliers\""), "the bar map: {json}");
        assert!(json.contains("\"total_cost\""), "the chain's cost: {json}");
        assert!(
            json.contains("\"baseline_cost\""),
            "and what it is compared against: {json}",
        );
        assert!(
            json.contains("\"policy_id\": \"candidate_chain\""),
            "under the policy that measured them: {json}",
        );
        drop(fs::remove_file(&path));
        drop(fs::remove_file(path.replace(".mid", ".json")));
    }

    // ── history replay does not leave a live row claiming to sound ───────────

    #[test]
    fn replaying_a_chain_from_history_clears_the_live_candidate_selection() {
        // The review's finding: the same defect through the side door. A row of
        // run B's live table stays highlighted while run A's chain snapshot
        // sounds, so the panel points at music that is not playing.
        let mut app = demo_app();
        app.gen_panel.variants = 2;
        app.do_generate();
        app.show_global_chain();
        let chain = app
            .history
            .entries()
            .iter()
            .find(|e| e.source == CandidateSource::GlobalChain)
            .expect("recorded")
            .id;

        app.gen_panel.seed = 1_234;
        app.do_generate(); // run B — the table now marks row 0
        assert_eq!(app.gen_panel.selected, Some(0), "run B marked its winner");

        app.select_history(chain); // run A's chain sounds
        assert_eq!(
            app.gen_panel.selected, None,
            "no live row is the active source while a snapshot plays",
        );
        assert_eq!(app.current, Some(AuditionCandidate::History(chain)));
    }

    #[test]
    fn replaying_any_history_entry_clears_the_live_candidate_selection() {
        // The general rule, not just the chain case: a live table selection is
        // a claim about the active source, and a replayed snapshot is not it.
        let mut app = demo_app();
        app.gen_panel.variants = 2;
        app.do_generate();
        let first = app.history.entries().first().expect("recorded").id;
        app.show_candidate(1);
        assert_eq!(app.gen_panel.selected, Some(1));
        app.select_history(first);
        assert_eq!(
            app.gen_panel.selected, None,
            "the row stops claiming to be what is sounding",
        );
    }

    #[test]
    fn switching_variants_while_paused_mid_note_silences_the_held_notes() {
        // The gap the review spotted in the matrix: seek() silences
        // unconditionally, so a paused mid-note switch cannot leave a note
        // hanging — but nothing said so.
        let mut app = demo_app();
        app.gen_panel.variants = 2;
        app.do_generate();
        app.vp.playing = true;
        app.advance_audio(0.4);
        app.vp.playing = false; // paused, mid-note
        assert!(
            app.player.active_count() > 0,
            "the fixture is paused with notes held",
        );
        app.show_global_chain();
        assert_eq!(
            app.player.active_count(),
            0,
            "the held notes are released by the switch, not left ringing",
        );
        assert!(!app.vp.playing, "and it is still paused");
    }

    #[test]
    fn the_generate_run_captures_its_global_chain_once() {
        // Both audition variants come from one run: the intact winner is row 0
        // of the captured set, the chain is planned from the same RankedSet at
        // capture time. Nothing later has a RankedSet to re-plan from.
        let mut app = demo_app();
        app.gen_panel.variants = 2;
        app.do_generate();
        let run = app.gen_panel.active.as_ref().expect("a run was captured");
        let GlobalChainOutcome::Planned(chain) = &run.chain else {
            panic!("the demo set is chain-compatible: {:?}", run.chain);
        };
        assert_eq!(
            chain.plan.steps.len(),
            app.gen_panel.bars,
            "one selected bar per asked bar, captured with the run",
        );
        assert!(
            !run.set.rows.is_empty(),
            "and the intact winner's set is captured beside it",
        );
    }

    #[test]
    fn auditioning_playback_and_history_never_replan_the_chain() {
        // The proof is a poisoned snapshot: if anything downstream re-plans,
        // the planted value is overwritten by a real one. A chain that is
        // recomputed on demand is a chain that can disagree with the set it
        // claims to be made of.
        let mut app = demo_app();
        app.gen_panel.variants = 2;
        app.do_generate();
        let poison = 1234.5_f64;
        {
            let run = app.gen_panel.active.as_mut().expect("a run");
            let GlobalChainOutcome::Planned(chain) = &mut run.chain else {
                panic!("chain-compatible");
            };
            chain.plan.total_cost = poison;
        }

        app.show_global_chain(); // audition the chain
        app.show_candidate(0); // back to the intact winner
        app.ab_swap(); // and A/B between them
        app.advance_audio(0.25); // play a little
        app.stop_playback();
        let id = app.history.entries().first().expect("recorded").id;
        app.select_history(id); // replay a history snapshot
        app.gen_panel.seed = 999; // and change the live knobs
        app.gen_panel.bars = 3;

        let run = app.gen_panel.active.as_ref().expect("still the same run");
        let GlobalChainOutcome::Planned(chain) = &run.chain else {
            panic!("chain-compatible");
        };
        assert_eq!(
            chain.plan.total_cost.to_bits(),
            poison.to_bits(),
            "the captured chain is the one planted at capture — nothing re-planned it",
        );
    }

    #[test]
    fn a_new_generate_run_replaces_the_chain_without_mutating_the_old_history() {
        // A new run is a new run: new id, newly planned chain. The entries the
        // old run recorded keep their own snapshots.
        let mut app = demo_app();
        app.gen_panel.variants = 2;
        app.do_generate();
        app.show_global_chain();
        let first_run = app.gen_panel.active.as_ref().expect("a run").context.run;
        let first_entries: Vec<(HistoryId, usize)> = app
            .history
            .entries()
            .iter()
            .map(|e| (e.id, e.score.master_bars.len()))
            .collect();
        assert!(!first_entries.is_empty(), "the first run recorded entries");

        app.gen_panel.seed = 4242;
        app.do_generate();
        let second_run = app.gen_panel.active.as_ref().expect("a run").context.run;
        assert_ne!(first_run, second_run, "a new Generate is a new run");

        for (id, bars) in first_entries {
            let entry = app.history.get(id).expect("the old entry survives");
            assert_eq!(
                entry.score.master_bars.len(),
                bars,
                "an old snapshot is not touched by a later run",
            );
            assert_eq!(entry.run, first_run, "and still belongs to its own run");
        }
    }

    #[test]
    fn ab_swaps_across_generate_and_swang_sources() {
        // #125 re-review 2: A/B remembers the last candidate viewed regardless
        // of source and swaps back to the RIGHT one — a Swang candidate after a
        // Generate candidate returns to the Swang set, not a same-index Generate
        // row.
        let mut app = demo_app();
        app.gen_panel.variants = 2;
        app.do_generate(); // shows Generate(0)
        app.swang.set = app.gen_panel.set().cloned(); // give Swang a set to paint
        app.swang_ctx = Some(SwangRunContext {
            run: app.history.begin_run(),
            program: app.swang.text.clone(),
            source_path: None,
        });
        app.swang_show(1); // now viewing Swang(1); leaving Generate(0)
        assert_eq!(
            app.ab_other,
            Some(AuditionCandidate::Generate(0)),
            "the Generate candidate we left is the A/B target",
        );
        app.ab_swap(); // back to the Generate set
        assert_eq!(app.gen_panel.selected, Some(0), "routed to Generate");
        assert_eq!(
            app.ab_other,
            Some(AuditionCandidate::Swang(1)),
            "and Swang(1) is now the other",
        );
        app.ab_swap(); // back to the Swang set
        assert_eq!(
            app.swang.selected,
            Some(1),
            "routed to Swang, not a Generate row"
        );
    }

    #[test]
    fn a_new_swang_run_resets_the_ab_session() {
        // #125 re-review 2: a fresh generation session (Generate OR Swang) clears
        // the A/B history, so `b` never swaps to a candidate from a stale run.
        let mut app = demo_app();
        app.gen_panel.variants = 2;
        app.do_generate();
        app.show_candidate(1); // A/B target is now Generate(0)
        assert!(app.ab_other.is_some());
        app.reset_audition(); // stands in for the reset a new run performs
        assert!(
            app.ab_other.is_none() && app.current.is_none(),
            "a new generation session starts A/B empty",
        );
    }

    // ── S8 Slice 3: history / favorite-reject / provenance ────────────────────

    #[test]
    fn showing_a_generate_candidate_records_it_with_generate_provenance() {
        use griff_ui_core::history::{CandidateSource, GeneratorProvenance};
        let mut app = demo_app();
        app.gen_panel.variants = 2;
        app.gen_panel.seed = 7;
        app.do_generate(); // shows candidate 0 → records it
        assert_eq!(
            app.history.entries().len(),
            1,
            "the shown candidate is recorded"
        );
        let entry = &app.history.entries()[0];
        assert_eq!(entry.source, CandidateSource::Generate);
        assert_eq!(entry.verdict, None, "recorded undecided");
        match &entry.provenance.generator {
            GeneratorProvenance::Generate { seed, bars, .. } => {
                assert_eq!(*seed, 7, "the ask seed is captured honestly");
                assert_eq!(*bars, app.gen_panel.bars);
            }
            other => panic!("a Generate candidate is not {other:?}"),
        }
    }

    #[test]
    fn a_no_corpus_generate_records_seed_only_provenance() {
        // Finding 3 (#1/#7): with no corpus attached, the recorded contribution
        // is seed-only and the UI summary says so — never "corpus".
        use griff_ui_core::history::GeneratorProvenance;
        let mut app = demo_app();
        assert!(app.material.is_none(), "the demo app has no corpus");
        app.gen_panel.variants = 2;
        app.do_generate();
        let entry = &app.history.entries()[0];
        match &entry.provenance.generator {
            GeneratorProvenance::Generate { corpus, .. } => {
                assert!(corpus.is_seed_only(), "no corpus contributes nothing");
                assert_eq!(corpus.templates, 0);
                assert_eq!(corpus.references, 0);
                assert!(!corpus.gesture);
            }
            other => panic!("a Generate candidate is not {other:?}"),
        }
        assert!(
            provenance_summary(&entry.provenance).contains("seed only"),
            "the summary reads seed-only, not corpus",
        );
    }

    // ── S8 Slice 3 re-review: Generate provenance is captured at run time ─────

    /// The Generate provenance of the last-recorded candidate.
    fn last_generate(app: &CockpitApp) -> (Option<String>, u64, usize, usize, bool) {
        use griff_ui_core::history::GeneratorProvenance;
        match &app
            .history
            .entries()
            .last()
            .expect("an entry")
            .provenance
            .generator
        {
            GeneratorProvenance::Generate {
                source,
                seed,
                bars,
                variants_per_strategy,
                corpus,
                ..
            } => (
                source.clone(),
                *seed,
                *bars,
                *variants_per_strategy,
                corpus.is_seed_only(),
            ),
            other => panic!("a Generate candidate is not {other:?}"),
        }
    }

    fn two_rhythm_material() -> CorpusMaterial {
        use griff_core::event::Ticks;
        use griff_core::generate::RhythmTemplate;
        CorpusMaterial {
            rhythms: vec![
                RhythmTemplate::from_durations(&[Ticks(480)]),
                RhythmTemplate::from_durations(&[Ticks(240)]),
            ],
            references: Vec::new(),
            gesture: None,
            skipped: Vec::new(),
        }
    }

    #[test]
    fn generate_source_is_the_run_seed_not_a_shown_candidate_title() {
        // P1: showing candidate 1 after candidate 0 must record the run's seed
        // score ("demo"), not candidate 0's display title (which show_score set).
        let mut app = demo_app();
        app.gen_panel.variants = 2;
        app.do_generate(); // shows candidate 0 → title becomes its own
        app.show_candidate(1);
        assert_eq!(
            last_generate(&app).0.as_deref(),
            Some("demo"),
            "the source is the seed score, not a shown candidate's title",
        );
    }

    #[test]
    fn generate_provenance_keeps_the_runs_seed_after_a_knob_change() {
        let mut app = demo_app();
        app.gen_panel.variants = 2;
        app.gen_panel.seed = 5;
        app.do_generate();
        app.gen_panel.seed = 99; // change the knob after the set was produced
        app.show_candidate(1);
        assert_eq!(last_generate(&app).1, 5, "the old row keeps the run's seed");
    }

    #[test]
    fn generate_provenance_keeps_the_runs_bars_and_variants() {
        let mut app = demo_app();
        app.gen_panel.bars = 4;
        app.gen_panel.variants = 2;
        app.do_generate();
        app.gen_panel.bars = 16;
        app.gen_panel.variants = 5;
        app.show_candidate(1);
        let (_, _, bars, variants, _) = last_generate(&app);
        assert_eq!(bars, 4, "the old row keeps the run's bars");
        assert_eq!(variants, 2, "the old row keeps the run's variants");
    }

    #[test]
    fn generate_provenance_keeps_seed_only_after_a_corpus_is_attached() {
        let mut app = demo_app();
        app.gen_panel.variants = 2;
        app.do_generate(); // no corpus → seed-only
        app.material = Some(two_rhythm_material()); // attach one afterwards
        app.show_candidate(1);
        assert!(
            last_generate(&app).4,
            "the old row's contribution stays seed-only — attachment is not the run",
        );
    }

    #[test]
    fn generate_source_keeps_the_run_tab_after_the_selection_changes() {
        let mut app = demo_app();
        app.gen_panel.sources = vec![generation::SourceTab {
            name: "tabA.mid".to_owned(),
            bytes: include_bytes!("../assets/demo.mid").to_vec(),
        }];
        app.gen_panel.source = Some(0);
        app.gen_panel.variants = 2;
        app.do_generate(); // seeded from tabA.mid
        app.gen_panel.source = None; // change the selection afterwards
        app.show_candidate(1);
        assert_eq!(
            last_generate(&app).0.as_deref(),
            Some("tabA.mid"),
            "the old row keeps the tab that seeded its run",
        );
    }

    #[test]
    fn re_showing_a_row_does_not_rewrite_its_provenance() {
        let mut app = demo_app();
        app.gen_panel.variants = 2;
        app.gen_panel.seed = 5;
        app.do_generate();
        let before = app.history.entries()[0].provenance.clone();
        app.gen_panel.seed = 999; // mutate, then re-show the same row
        app.show_candidate(0);
        assert_eq!(
            app.history.entries()[0].provenance,
            before,
            "re-showing a row of its run does not rewrite its origin",
        );
    }

    #[test]
    fn a_new_generate_run_captures_the_changed_settings() {
        let mut app = demo_app();
        app.gen_panel.variants = 2;
        app.gen_panel.seed = 5;
        app.do_generate();
        app.gen_panel.seed = 42;
        app.do_generate(); // a fresh run reads the new settings
        assert_eq!(
            last_generate(&app).1,
            42,
            "the new run captures the new seed"
        );
    }

    // ── S8 Slice 3 re-review: Keep exports the captured run, not live knobs ───

    /// A deterministic, freshly-emptied out dir for one keep test.
    fn keep_dir(tag: &str) -> PathBuf {
        let dir = env::temp_dir().join(format!("griff-keep-test-{tag}"));
        drop(fs::remove_dir_all(&dir)); // start clean, every run
        dir
    }

    /// Removes a keep test's out dir, whether or not it exists.
    fn drop_keep_dir(dir: &PathBuf) {
        drop(fs::remove_dir_all(dir));
    }

    /// The sidecar JSON written beside a kept `.mid`.
    fn read_sidecar(mid: &str) -> serde_json::Value {
        let stem = mid.strip_suffix(".mid").expect("a .mid path");
        let text = fs::read_to_string(format!("{stem}.json")).expect("the sidecar is written");
        serde_json::from_str(&text).expect("the sidecar is JSON")
    }

    #[test]
    fn keep_sidecar_and_filename_use_the_runs_seed_not_the_live_knob() {
        let dir = keep_dir("seed");
        let mut app = demo_app();
        app.set_out_dir(dir.clone());
        app.gen_panel.variants = 2;
        app.gen_panel.seed = 5;
        app.do_generate();
        app.gen_panel.seed = 99; // change the knob after the run
        let mid = app.write_keep(1).expect("keep writes");
        assert!(
            mid.contains("seed5_"),
            "the filename uses the run's seed: {mid}"
        );
        assert!(!mid.contains("seed99_"), "not the live knob: {mid}");
        assert_eq!(
            read_sidecar(&mid)["seed"],
            5,
            "the sidecar keeps the run's seed"
        );
        drop_keep_dir(&dir);
    }

    #[test]
    fn keep_sidecar_keeps_the_runs_bars_and_variants() {
        let dir = keep_dir("bars");
        let mut app = demo_app();
        app.set_out_dir(dir.clone());
        app.gen_panel.bars = 4;
        app.gen_panel.variants = 2;
        app.do_generate();
        app.gen_panel.bars = 16;
        app.gen_panel.variants = 5;
        let json = read_sidecar(&app.write_keep(0).expect("keep writes"));
        assert_eq!(json["bars"], 4, "the sidecar keeps the run's bars");
        assert_eq!(json["variants_per_strategy"], 2, "and its variants");
        drop_keep_dir(&dir);
    }

    #[test]
    fn keep_sidecar_keeps_the_run_source_tab() {
        let dir = keep_dir("source");
        let mut app = demo_app();
        app.set_out_dir(dir.clone());
        app.gen_panel.sources = vec![generation::SourceTab {
            name: "tabA.mid".to_owned(),
            bytes: include_bytes!("../assets/demo.mid").to_vec(),
        }];
        app.gen_panel.source = Some(0);
        app.gen_panel.variants = 2;
        app.do_generate();
        app.gen_panel.source = None; // change the selection afterwards
        let json = read_sidecar(&app.write_keep(0).expect("keep writes"));
        assert_eq!(
            json["source"], "tabA.mid",
            "the sidecar keeps the run's source"
        );
        drop_keep_dir(&dir);
    }

    #[test]
    fn keep_sidecar_stays_seed_only_when_a_corpus_is_attached_later() {
        let dir = keep_dir("attach");
        let mut app = demo_app();
        app.set_out_dir(dir.clone());
        app.gen_panel.variants = 2;
        app.do_generate(); // no corpus → seed-only
        app.material = Some(two_rhythm_material()); // attach afterwards
        let json = read_sidecar(&app.write_keep(0).expect("keep writes"));
        assert_eq!(
            json["corpus"], false,
            "attachment after the run is not contribution"
        );
        drop_keep_dir(&dir);
    }

    #[test]
    fn keep_sidecar_keeps_a_real_contribution_after_the_corpus_is_detached() {
        let dir = keep_dir("detach");
        let mut app = demo_app();
        app.set_out_dir(dir.clone());
        app.material = Some(two_rhythm_material()); // a corpus that contributes
        app.gen_panel.variants = 2;
        app.do_generate();
        app.material = None; // detach afterwards
        let json = read_sidecar(&app.write_keep(0).expect("keep writes"));
        assert_eq!(json["corpus"], true, "the run's real contribution survives");
        drop_keep_dir(&dir);
    }

    #[test]
    fn keep_midi_and_sidecar_come_from_the_same_run() {
        let dir = keep_dir("pair");
        let mut app = demo_app();
        app.set_out_dir(dir.clone());
        app.gen_panel.variants = 2;
        app.do_generate();
        let (strategy, variant_seed) = {
            let row = &app.gen_panel.set().expect("a set").rows[1];
            (row.strategy.clone(), row.variant_seed)
        };
        let mid = app.write_keep(1).expect("keep writes");
        assert!(Path::new(&mid).exists(), "the .mid is written");
        let json = read_sidecar(&mid);
        assert_eq!(
            json["strategy"], strategy,
            "the sidecar names the exported row"
        );
        assert_eq!(json["variant_seed"], variant_seed, "and its variant seed");
        drop_keep_dir(&dir);
    }

    #[test]
    fn a_new_generate_run_keeps_the_newly_changed_values() {
        let dir = keep_dir("newrun");
        let mut app = demo_app();
        app.set_out_dir(dir.clone());
        app.gen_panel.variants = 2;
        app.gen_panel.seed = 5;
        app.do_generate();
        app.gen_panel.seed = 42;
        app.do_generate(); // a fresh run
        let mid = app.write_keep(0).expect("keep writes");
        assert!(
            mid.contains("seed42_"),
            "the new run exports its own seed: {mid}"
        );
        assert_eq!(read_sidecar(&mid)["seed"], 42);
        drop_keep_dir(&dir);
    }

    // ── S8 Slice 3 re-review: Swang records the resolved frontend source ──────

    #[test]
    fn a_resolved_native_path_is_recorded_as_that_path() {
        assert_eq!(
            SwangSourceOrigin::ResolvedPath("riff.mid".to_owned()).provenance_path(),
            Some("riff.mid".to_owned()),
        );
    }

    #[test]
    fn a_displayed_score_origin_records_no_path() {
        assert_eq!(
            SwangSourceOrigin::DisplayedScore.provenance_path(),
            None,
            "the browser never read the declared path — it must not claim one",
        );
    }

    #[test]
    fn a_displayed_score_swang_summary_says_displayed_score() {
        use griff_ui_core::history::{GenerationRunId, GeneratorProvenance, Provenance};
        let p = Provenance::new(
            GenerationRunId(0),
            0,
            "auto#1".to_owned(),
            GeneratorProvenance::Swang {
                program: "swang 1".to_owned(),
                source_path: SwangSourceOrigin::DisplayedScore.provenance_path(),
                strategy: "auto".to_owned(),
                variant_seed: 1,
                rank: 1,
                aggregate: 0.5,
            },
        );
        let line = provenance_summary(&p);
        assert!(
            line.contains("source displayed score"),
            "the summary names the displayed score, not a path: {line}",
        );
    }

    #[test]
    fn a_web_style_swang_run_context_cannot_claim_the_declared_path() {
        let ctx = SwangRunContext {
            run: GenerationRunId(0),
            program: "swang 1\n\ngenerate { source \"declared.mid\" }".to_owned(),
            source_path: SwangSourceOrigin::DisplayedScore.provenance_path(),
        };
        assert_eq!(
            ctx.source_path, None,
            "a displayed-score run records no path, whatever the program declares",
        );
    }

    #[test]
    fn editing_the_program_does_not_change_the_captured_source_origin() {
        use griff_ui_core::history::GeneratorProvenance;
        let mut app = demo_app();
        app.gen_panel.variants = 2;
        app.do_generate();
        app.swang.set = app.gen_panel.set().cloned();
        app.swang_ctx = Some(SwangRunContext {
            run: app.history.begin_run(),
            program: "swang 1\n// evaluated\n".to_owned(),
            source_path: SwangSourceOrigin::ResolvedPath("riff.mid".to_owned()).provenance_path(),
        });
        app.swang_show(0);
        app.swang.text = "swang 1\n// EDITED, declaring another source\n".to_owned();
        app.swang_show(1); // another row of the same run
        let entry = app.history.entries().last().expect("a swang entry");
        match &entry.provenance.generator {
            GeneratorProvenance::Swang { source_path, .. } => {
                assert_eq!(
                    source_path.as_deref(),
                    Some("riff.mid"),
                    "the captured origin survives an edit",
                );
            }
            other => panic!("expected a Swang candidate, got {other:?}"),
        }
    }

    #[test]
    fn swang_provenance_is_tied_to_the_evaluated_program_not_live_text() {
        use griff_ui_core::history::GeneratorProvenance;
        let mut app = demo_app();
        app.gen_panel.variants = 2;
        app.do_generate(); // borrow a 2-row set to stand in for a Swang set
        app.swang.set = app.gen_panel.set().cloned();
        let evaluated = "swang 1\n\n// the program that was evaluated\n".to_owned();
        app.swang.text = evaluated.clone();
        app.swang_ctx = Some(SwangRunContext {
            run: app.history.begin_run(),
            program: evaluated.clone(),
            source_path: None,
        });
        app.swang_show(0); // record row 0 under the evaluated program
                           // The editor text changes; then A/B lands on another row of the SAME
                           // run — its provenance must be the evaluated program, not the live edit.
        app.swang.text = "swang 1\n\n// EDITED AFTERWARDS\n".to_owned();
        app.swang_show(1);
        let entry = app.history.entries().last().expect("a swang entry");
        match &entry.provenance.generator {
            GeneratorProvenance::Swang { program, .. } => {
                assert_eq!(
                    *program, evaluated,
                    "the program is the run's evaluated text"
                );
            }
            other => panic!("expected a Swang candidate, got {other:?}"),
        }
    }

    #[test]
    fn showing_a_swang_candidate_records_it_with_swang_provenance() {
        use griff_ui_core::history::{CandidateSource, GeneratorProvenance};
        let mut app = demo_app();
        app.gen_panel.variants = 2;
        app.do_generate();
        app.swang.set = app.gen_panel.set().cloned(); // give Swang a set to paint
        app.swang_ctx = Some(SwangRunContext {
            run: app.history.begin_run(),
            program: app.swang.text.clone(),
            source_path: None,
        });
        app.swang_show(0);
        let swang_entry = app
            .history
            .entries()
            .iter()
            .find(|e| e.source == CandidateSource::Swang)
            .expect("the Swang candidate is recorded");
        match &swang_entry.provenance.generator {
            GeneratorProvenance::Swang { program, .. } => {
                assert!(!program.is_empty(), "the program text is captured");
            }
            other => panic!("a Swang candidate is not {other:?}"),
        }
    }

    #[test]
    fn a_new_generation_appends_to_history_without_destroying_it() {
        let mut app = demo_app();
        app.gen_panel.variants = 2;
        app.gen_panel.seed = 1;
        app.do_generate();
        app.show_candidate(1); // a second distinct candidate this run
        let after_first = app.history.entries().len();
        assert!(after_first >= 2);
        app.gen_panel.seed = 999; // a different ask → different candidates
        app.do_generate();
        assert!(
            app.history.entries().len() > after_first,
            "the new generation appends; the earlier run's entries survive",
        );
    }

    #[test]
    fn re_running_the_same_generate_ask_makes_a_separate_history_entry() {
        // Finding 1: two runs of the identical ask share a candidate key but are
        // distinct runs, so the winner is recorded twice — never collapsed onto
        // the earlier entry (which would drop the new run's provenance).
        let mut app = demo_app();
        app.gen_panel.variants = 1;
        app.gen_panel.seed = 7;
        app.do_generate(); // run A → winner recorded
        let a = app.history.entries()[0].id;
        let a_run = app.history.entries()[0].run;
        let after_a = app.history.entries().len();
        app.do_generate(); // run B, identical ask → winner recorded again
        assert!(
            app.history.entries().len() > after_a,
            "the identical re-run appends a new entry, not a dedupe",
        );
        let b = app.history.entries().last().expect("run B entry").id;
        let b_run = app.history.entries().last().expect("run B entry").run;
        assert_ne!(a, b, "distinct history ids");
        assert_ne!(a_run, b_run, "distinct generation runs");
    }

    #[test]
    fn re_showing_a_row_within_the_same_generate_run_dedupes() {
        // Finding 1: re-showing the same row of the current set returns the same
        // entry (same run + key), so no duplicate accrues.
        let mut app = demo_app();
        app.gen_panel.variants = 2;
        app.do_generate();
        let before = app.history.entries().len();
        app.show_candidate(0); // re-show the same row of the same run
        assert_eq!(
            app.history.entries().len(),
            before,
            "re-showing a row of the live run does not duplicate it",
        );
    }

    #[test]
    fn a_swang_run_is_a_distinct_run_from_a_generate_run() {
        let mut app = demo_app();
        app.gen_panel.variants = 2;
        app.do_generate();
        let gen_run = app.history.entries()[0].run;
        app.swang.set = app.gen_panel.set().cloned();
        app.swang_ctx = Some(SwangRunContext {
            run: app.history.begin_run(), // a Swang run mints its own
            program: app.swang.text.clone(),
            source_path: None,
        });
        app.swang_show(0);
        let swang_entry = app
            .history
            .entries()
            .iter()
            .rev()
            .find(|e| e.source == CandidateSource::Swang)
            .expect("a Swang entry");
        assert_ne!(
            swang_entry.run, gen_run,
            "Swang and Generate are separate runs"
        );
    }

    #[test]
    fn a_fresh_load_clears_the_history_selection_but_keeps_the_record() {
        // Finding 2: after loading a new score, no history row may read as
        // selected or playing — but the entries and verdicts are preserved.
        let mut app = demo_app();
        app.gen_panel.variants = 2;
        app.do_generate(); // records + selects a history entry
        let id = app.history.entries()[0].id;
        app.history.set_verdict(id, Verdict::Favorite);
        assert!(
            app.history.selected().is_some(),
            "a row is selected pre-load"
        );
        let before = app.history.entries().len();

        app.load("fresh.mid".to_owned(), include_bytes!("../assets/demo.mid"))
            .expect("the demo bytes load");

        assert_eq!(
            app.history.entries().len(),
            before,
            "the record is preserved"
        );
        assert_eq!(
            app.history.get(id).unwrap().verdict,
            Some(Verdict::Favorite),
            "verdicts survive a fresh load",
        );
        assert_eq!(app.history.selected(), None, "no history row is selected");
        assert_eq!(app.current, None, "no active audition candidate");
        app.vp.playing = true;
        assert!(
            app.history.selected().is_none(),
            "with nothing selected, no row can read as playing",
        );
    }

    #[test]
    fn a_generate_after_a_fresh_load_selects_its_winner() {
        // Finding 2 regression: a new successful run after the reset still
        // selects the shown winner immediately.
        let mut app = demo_app();
        app.gen_panel.variants = 2;
        app.do_generate();
        app.load("fresh.mid".to_owned(), include_bytes!("../assets/demo.mid"))
            .expect("the demo bytes load");
        assert_eq!(app.history.selected(), None);
        app.do_generate(); // a fresh run after the load
        assert!(
            app.history.selected().is_some(),
            "the new run's winner becomes the selected history row",
        );
    }

    #[test]
    fn selecting_from_history_switches_the_score_and_silences_the_old() {
        let mut app = demo_app();
        app.gen_panel.variants = 2;
        app.do_generate(); // Generate(0) recorded as the first entry
        let first = app.history.entries()[0].id;
        app.show_candidate(1); // a second entry; now playing this one
        app.vp.play_tick = 300;
        app.vp.playing = true;
        app.advance_audio(0.2); // sound some notes
        app.select_history(first); // jump back to the first via history
        assert_eq!(
            app.player.active_count(),
            0,
            "the old score's notes are silenced on a history switch",
        );
        assert_eq!(app.history.selected(), Some(first), "the entry is selected");
        assert!(
            app.vp.play_tick <= app.ctx.tick_end,
            "the playhead belongs to the active score",
        );
    }

    #[test]
    fn ab_swaps_between_a_history_entry_and_a_panel_candidate() {
        let mut app = demo_app();
        app.gen_panel.variants = 2;
        app.do_generate();
        let first = app.history.entries()[0].id;
        app.select_history(first); // now current = History(first)
        app.show_candidate(1); // leaving History(first) for Generate(1)
        assert_eq!(
            app.ab_other,
            Some(AuditionCandidate::History(first)),
            "the history entry is the A/B target",
        );
        app.ab_swap(); // back to the history entry
        assert_eq!(
            app.history.selected(),
            Some(first),
            "routed to the history entry"
        );
    }

    #[test]
    fn selecting_a_shorter_history_snapshot_remaps_the_loop() {
        use griff_ui_core::history::GeneratorProvenance;
        let mut app = CockpitApp::from_score(n_bar_score(2), "A".to_owned());
        app.set_loop_bars(true, 1, 1); // loop A's second bar
        assert!(app.loop_range.is_some());
        // Inject a one-bar snapshot into history and jump to it.
        let run = app.history.begin_run();
        let short = app.history.record(
            run,
            "short#1".to_owned(),
            "B".to_owned(),
            n_bar_score(1),
            GeneratorProvenance::Generate {
                source: None,
                corpus: CorpusContribution {
                    templates: 0,
                    references: 0,
                    gesture: false,
                },
                seed: 0,
                bars: 1,
                variants_per_strategy: 1,
                strategy: "auto".to_owned(),
                variant_seed: 1,
                rank: 1,
                aggregate: 0.0,
            },
        );
        app.vp.playing = true;
        app.select_history(short);
        let end = app.ctx.tick_end;
        assert!(end <= 3840, "B is one bar");
        if let Some((lo, hi)) = app.loop_range {
            assert!(lo < hi && hi <= end, "a kept loop is clamped inside B");
        }
        for _ in 0..40 {
            app.advance_audio(0.05);
            assert!(app.vp.play_tick <= end, "playback never exceeds B's end");
        }
    }

    #[test]
    fn fast_successive_history_switches_leave_no_hung_notes() {
        let mut app = demo_app();
        app.gen_panel.variants = 3;
        app.do_generate();
        app.show_candidate(1);
        app.show_candidate(2); // three entries recorded
        let ids: Vec<_> = app.history.entries().iter().map(|e| e.id).collect();
        app.vp.playing = true;
        for &id in ids.iter().cycle().take(12) {
            app.advance_audio(0.03);
            app.select_history(id);
            assert_eq!(
                app.player.active_count(),
                0,
                "every rapid switch silences the old score",
            );
        }
        assert_eq!(app.history.selected(), ids.last().copied());
    }

    #[test]
    fn set_verdict_on_a_history_entry_toggles_favorite_and_reject() {
        use griff_ui_core::history::Verdict;
        let mut app = demo_app();
        app.gen_panel.variants = 2;
        app.do_generate();
        let id = app.history.entries()[0].id;
        app.history.set_verdict(id, Verdict::Favorite);
        assert_eq!(
            app.history.get(id).unwrap().verdict,
            Some(Verdict::Favorite)
        );
        app.history.set_verdict(id, Verdict::Rejected);
        assert_eq!(
            app.history.get(id).unwrap().verdict,
            Some(Verdict::Rejected),
            "reject supplants favorite",
        );
    }

    #[test]
    fn generate_provenance_summary_names_its_source() {
        use griff_ui_core::history::{
            CorpusContribution, GenerationRunId, GeneratorProvenance, Provenance,
        };
        // Built from the typed provenance alone — no live app or panel state.
        let summary_for = |source: Option<&str>| {
            provenance_summary(&Provenance::new(
                GenerationRunId(0),
                0,
                "auto#1".to_owned(),
                GeneratorProvenance::Generate {
                    source: source.map(ToOwned::to_owned),
                    corpus: CorpusContribution {
                        templates: 0,
                        references: 0,
                        gesture: false,
                    },
                    seed: 7,
                    bars: 8,
                    variants_per_strategy: 2,
                    strategy: "auto".to_owned(),
                    variant_seed: 1,
                    rank: 1,
                    aggregate: 0.5,
                },
            ))
        };
        let a = summary_for(Some("tabA.mid"));
        let b = summary_for(Some("tabB.mid"));
        assert!(a.contains("source tabA.mid"), "names its source: {a}");
        assert!(b.contains("source tabB.mid"), "names its source: {b}");
        assert_ne!(
            a, b,
            "two runs from different sources must not render identically",
        );
        let displayed = summary_for(None);
        assert!(
            displayed.contains("source displayed score"),
            "no captured source reads as the displayed score: {displayed}",
        );
    }

    #[test]
    fn provenance_summary_names_the_generator_and_the_ask() {
        use griff_ui_core::history::{GenerationRunId, GeneratorProvenance, Provenance};
        let g = Provenance::new(
            GenerationRunId(0),
            0,
            "auto#1".to_owned(),
            GeneratorProvenance::Generate {
                source: Some("riff.mid".to_owned()),
                corpus: CorpusContribution {
                    templates: 2,
                    references: 1,
                    gesture: true,
                },
                seed: 7,
                bars: 8,
                variants_per_strategy: 2,
                strategy: "auto".to_owned(),
                variant_seed: 1,
                rank: 3,
                aggregate: 0.5,
            },
        );
        let gen_line = provenance_summary(&g);
        assert!(
            gen_line.contains("generate"),
            "names the generator: {gen_line}"
        );
        assert!(gen_line.contains('7'), "carries the ask seed: {gen_line}");
        assert!(gen_line.contains('3'), "carries the rank: {gen_line}");
        assert!(
            gen_line.contains("corpus") && gen_line.contains("gesture"),
            "reports the actual corpus contribution: {gen_line}",
        );

        let s = Provenance::new(
            GenerationRunId(1),
            1,
            "auto#1".to_owned(),
            GeneratorProvenance::Swang {
                program: "swang 1".to_owned(),
                source_path: Some("riff.mid".to_owned()),
                strategy: "auto".to_owned(),
                variant_seed: 1,
                rank: 2,
                aggregate: 0.5,
            },
        );
        let swang_line = provenance_summary(&s);
        assert!(
            swang_line.contains("swang"),
            "names the Swang generator: {swang_line}"
        );
        assert!(
            !swang_line.contains("seed 7"),
            "a Swang line never shows a Generate-only ask: {swang_line}",
        );
    }

    /// #123 defect 4: a program whose declared `source` cannot be read must
    /// **error**, never silently seed the run from the displayed score — a
    /// program's provenance may not describe music made from a different file.
    #[test]
    fn a_missing_swang_source_errors_rather_than_using_the_displayed_score() {
        let app = demo_app(); // a score IS loaded — the tempting wrong fallback
        let program = "swang 1\n\npattern p {\n    ascii \"X.X/XX./.XX\"\n    \
             |> fractalize depth 1 max_cells 4096 density 9500bps seed 4\n    \
             |> linearize snake\n    |> map_rhythm unit 1/16 tail rest_pad\n    \
             |> generate {\n        source \"no-such-file-xyz.mid\"\n        bars 4\n        \
             seed 42\n        candidates 2\n        strategy auto\n    }\n    \
             |> export midi \"out.mid\"\n}\n";
        let compiled = eval::compile_program(program).expect("compiles");
        let result = app.resolve_swang_source(&compiled);
        assert!(
            result.is_err(),
            "a missing source must error, not fall back to the displayed score"
        );
        assert!(
            result.unwrap_err().contains("no-such-file-xyz"),
            "the error names the unreadable declared path"
        );
    }

    #[test]
    fn generating_ranks_a_set_and_shows_its_winner() {
        let mut app = demo_app();
        let before = app.title.clone();
        app.gen_panel.variants = 2;
        app.do_generate();

        let set = app.gen_panel.set().expect("the demo seeds a request");
        assert!(
            !set.rows.is_empty(),
            "five strategies contribute candidates"
        );
        assert_eq!(set.rows[0].rank, 1, "the table is rank-ordered");
        assert_eq!(
            app.gen_panel.selected,
            Some(0),
            "generating shows the winner — the candidate `griff generate` writes",
        );
        assert_ne!(app.title, before, "the roll now shows the candidate");
        assert!(
            app.title.contains(&set.rows[0].strategy),
            "the title names the shown candidate's strategy: {}",
            app.title,
        );
    }

    #[test]
    fn selecting_a_candidate_paints_that_candidate() {
        let mut app = demo_app();
        app.do_generate();
        let n = app.gen_panel.set().expect("generated").rows.len();
        assert!(n > 1, "more than one candidate to choose between");

        app.show_candidate(n - 1);
        assert_eq!(app.gen_panel.selected, Some(n - 1));
        let last = &app.gen_panel.set().expect("generated").rows[n - 1];
        assert!(
            app.title.contains(&last.strategy),
            "the roll follows the selection: {}",
            app.title,
        );
    }

    #[test]
    fn a_stale_source_pick_falls_back_to_the_displayed_score() {
        let mut app = demo_app();
        app.gen_panel.source = Some(3); // no sources loaded — an impossible pick
        app.do_generate();
        assert!(
            app.gen_panel.set().is_some(),
            "an out-of-range pick seeds from the displayed score, it does not fail",
        );
    }

    /// A hand-built view with two adjacent sections, so section navigation has
    /// somewhere to move regardless of the demo's classification.
    fn two_section_app() -> CockpitApp {
        use griff_ui_core::{Lane, NoteRect, Section};
        let view = PianoRollView {
            ppq: 480,
            tick_start: 0,
            tick_end: 7680,
            low_pitch: 52,
            high_pitch: 64,
            bar_lines: vec![0, 1920, 3840, 5760, 7680],
            lanes: vec![Lane {
                name: "lead".to_owned(),
                notes: vec![
                    NoteRect {
                        onset: 0,
                        end: 480,
                        pitch: 60,
                    },
                    NoteRect {
                        onset: 3840,
                        end: 4320,
                        pitch: 64,
                    },
                ],
            }],
            tempo_bpm: 120.0,
            bar_count: 4,
        };
        let analysis = Analysis {
            focus_track: 0,
            sections: vec![
                Section {
                    class: BarClass::Riff,
                    bar_start: 0,
                    bar_end: 2,
                    tick_start: 0,
                    tick_end: 3840,
                },
                Section {
                    class: BarClass::Breakdown,
                    bar_start: 2,
                    bar_end: 4,
                    tick_start: 3840,
                    tick_end: 7680,
                },
            ],
            metrics: None,
            complexity: None,
            boundaries: vec![],
        };
        CockpitApp::new(view, analysis, "two-section".to_owned())
    }

    #[test]
    fn space_toggles_playback_through_the_input_path() {
        let mut app = demo_app();
        assert!(!app.vp.playing, "starts paused");
        assert!(!press(&mut app, Key::Space), "play is not a quit");
        assert!(app.vp.playing, "Space starts playback");
        press(&mut app, Key::Space);
        assert!(!app.vp.playing, "Space again pauses");
    }

    #[test]
    fn the_inspector_key_toggles_the_inspector() {
        let mut app = demo_app();
        let before = app.vp.show_inspector;
        press(&mut app, Key::I);
        assert_eq!(app.vp.show_inspector, !before, "`i` toggles the inspector");
    }

    #[test]
    fn the_history_key_toggles_the_history_window() {
        let mut app = demo_app();
        assert!(!app.history_open, "the history window starts closed");
        press(&mut app, Key::Y);
        assert!(app.history_open, "`y` opens it");
        press(&mut app, Key::Y);
        assert!(!app.history_open, "`y` toggles it shut");
    }

    #[test]
    fn quit_keys_request_a_quit() {
        let mut app = demo_app();
        assert!(press(&mut app, Key::Q), "`q` quits");
        assert!(press(&mut app, Key::Escape), "Esc quits");
    }

    #[test]
    fn section_keys_move_the_selection() {
        let mut app = two_section_app();
        assert_eq!(app.vp.sel_section, 0, "starts on the first section");
        press(&mut app, Key::CloseBracket);
        assert_eq!(app.vp.sel_section, 1, "`]` selects the next section");
        press(&mut app, Key::OpenBracket);
        assert_eq!(app.vp.sel_section, 0, "`[` selects the previous section");
    }

    #[test]
    fn paint_never_panics_across_extreme_grid_sizes() {
        let mut app = demo_app();
        let ctx = egui::Context::default();
        // Sub-gutter, tall-and-thin, and oversized panels all exercise the
        // clamped pixel/grid arithmetic the crate-level lint allow vouches for.
        for (w, h) in [
            (1.0, 1.0),
            (20.0, 8.0),
            (90.0, 30.0),
            (4000.0, 40.0),
            (200.0, 3000.0),
        ] {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(w, h))),
                ..Default::default()
            };
            #[allow(deprecated)]
            let _frame = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| app.paint(ui));
            });
        }
    }

    #[test]
    fn paint_survives_a_dense_grid_of_panel_sizes() {
        let mut app = two_section_app();
        let ctx = egui::Context::default();
        // Odd strides hit a wide spread of column/row counts, including the
        // gutter-boundary and single-row cases, not just round numbers.
        for w in (0..1600).step_by(113) {
            for h in (0..1200).step_by(97) {
                let input = egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        egui::pos2(0.0, 0.0),
                        egui::vec2(w as f32, h as f32),
                    )),
                    ..Default::default()
                };
                #[allow(deprecated)]
                let _frame = ctx.run(input, |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| app.paint(ui));
                });
            }
        }
    }

    #[test]
    fn multiple_lanes_paint_in_distinct_lane_colours() {
        use griff_ui_core::{Lane, NoteRect, Section};
        use std::collections::HashSet;
        let view = PianoRollView {
            ppq: 480,
            tick_start: 0,
            tick_end: 1920,
            low_pitch: 52,
            high_pitch: 72,
            bar_lines: vec![0, 1920],
            lanes: vec![
                Lane {
                    name: "lead".to_owned(),
                    notes: vec![NoteRect {
                        onset: 0,
                        end: 480,
                        pitch: 60,
                    }],
                },
                Lane {
                    name: "harmony".to_owned(),
                    notes: vec![NoteRect {
                        onset: 0,
                        end: 480,
                        pitch: 67,
                    }],
                },
            ],
            tempo_bpm: 120.0,
            bar_count: 1,
        };
        let analysis = Analysis {
            focus_track: 0,
            sections: vec![Section {
                class: BarClass::Riff,
                bar_start: 0,
                bar_end: 1,
                tick_start: 0,
                tick_end: 1920,
            }],
            metrics: None,
            complexity: None,
            boundaries: vec![],
        };
        let app = CockpitApp::new(view, analysis, "multi".to_owned());
        let scene = resolve(
            &app.view,
            &app.analysis,
            &app.vp,
            GridSize { cols: 60, rows: 24 },
        );

        let lane_colours: HashSet<(u8, u8, u8)> = scene
            .plane
            .iter()
            .filter_map(|cell| match cell.role {
                CellRole::Note(_) => cell_style(*cell, &app.theme).fill.map(|c| (c.r, c.g, c.b)),
                _ => None,
            })
            .collect();
        assert!(
            lane_colours.len() >= 2,
            "two tracks should paint in ≥2 distinct lane colours, saw {}",
            lane_colours.len()
        );
    }

    #[test]
    fn load_swaps_the_displayed_score() {
        let mut app = demo_app();
        let demo_title = app.title().to_owned();
        app.load(
            "multi.mid".to_owned(),
            include_bytes!("../assets/multi_track.mid"),
        )
        .expect("multi_track.mid imports");
        assert_eq!(
            app.title(),
            "multi.mid",
            "the title follows the loaded source"
        );
        assert_ne!(app.title(), demo_title, "the source changed");
        assert_eq!(
            app.view.lanes.len(),
            1,
            "the roll shows one track at a time"
        );
        assert!(
            app.track_names.len() >= 2,
            "the multi-track file fills the track selector, got {}",
            app.track_names.len()
        );
    }

    #[test]
    fn focus_on_track_isolates_a_track_and_targets_capture() {
        let mut app = demo_app();
        app.load(
            "multi.mid".to_owned(),
            include_bytes!("../assets/multi_track.mid"),
        )
        .expect("multi_track.mid imports");
        let tracks = app.track_names.len();
        assert!(tracks >= 2, "needs a multi-track file");
        // Loading focuses the auto-picked track: one lane shown, not all overlaid.
        assert_eq!(app.view.lanes.len(), 1);

        // Capture must use the *selected* track: its JSON matches build_chunk at
        // that index, and switching tracks changes the result.
        app.focus_on_track(0);
        assert_eq!(app.selected_track, 0);
        assert_eq!(
            app.view.lanes.len(),
            1,
            "still a single lane after the switch"
        );
        let inputs = CaptureInputs {
            id: "t",
            created_at: "t",
            updated_at: "t",
            ..Default::default()
        };
        let score = app.score.clone().expect("score loaded");
        let expected0 =
            serde_json::to_string_pretty(&build_chunk(&score, 0, &inputs).expect("track 0"))
                .expect("json");
        assert_eq!(
            app.capture_json(&inputs).expect("captures"),
            expected0,
            "targets track 0"
        );

        app.focus_on_track(1);
        let expected1 =
            serde_json::to_string_pretty(&build_chunk(&score, 1, &inputs).expect("track 1"))
                .expect("json");
        assert_eq!(
            app.capture_json(&inputs).expect("captures"),
            expected1,
            "targets track 1"
        );

        // Out-of-range is a no-op, not a panic.
        app.focus_on_track(tracks + 9);
        assert_eq!(app.selected_track, 1, "an out-of-range track is ignored");
    }

    #[test]
    fn load_rejects_unparseable_bytes_and_keeps_the_score() {
        let mut app = demo_app();
        let kept = app.view.lanes.len();
        let err = app
            .load("junk.mid".to_owned(), b"definitely not a score")
            .expect_err("garbage must not import");
        assert!(
            err.contains("junk.mid"),
            "the error names the bad source: {err}"
        );
        assert_eq!(
            app.view.lanes.len(),
            kept,
            "a failed load leaves the current score intact"
        );
    }

    #[test]
    fn capture_json_builds_a_chunk_from_the_displayed_score() {
        // `from_score` keeps the imported score behind the view, so Capture works
        // on the initially-displayed file with no extra load (#98 review).
        let app = demo_app();
        let inputs = CaptureInputs {
            id: "demo_001",
            title: "Demo",
            redistributable: true,
            created_at: "2026-01-01T00:00:00Z",
            updated_at: "2026-01-01T00:00:00Z",
            ..CaptureInputs::default()
        };
        let json = app
            .capture_json(&inputs)
            .expect("captures the displayed score");
        assert!(json.contains("demo_001"), "the chunk carries its id");
        assert!(json.contains("\"rights\""), "rights are recorded");
    }

    #[test]
    fn capture_json_reports_no_score_for_a_synthetic_view() {
        // A hand-built view (via `new`, no imported score behind it) has nothing
        // to capture — the `None` path still reports cleanly.
        let app = two_section_app();
        app.capture_json(&CaptureInputs::default())
            .expect_err("a view with no backing score cannot capture");
    }

    #[test]
    fn loading_seeds_the_capture_form() {
        let mut app = demo_app();
        app.load(
            "path/to/Cool Riff.mid".to_owned(),
            include_bytes!("../assets/demo.mid"),
        )
        .expect("loads");
        assert_eq!(
            app.form.id, "cool_riff",
            "the id is a slug of the file stem"
        );
        assert_eq!(app.form.title, "Cool Riff");
        assert_eq!(app.form.filename, "Cool Riff.mid");

        let now = "2026-01-01T00:00:00Z";
        let json = app
            .capture_json(&app.form.inputs(now, now))
            .expect("captures from the form");
        assert!(
            json.contains("cool_riff"),
            "the captured chunk uses the form id"
        );
    }

    #[test]
    #[allow(deprecated)] // egui 0.34 flags `Context::run`; it still drives a CPU frame.
    fn load_corpus_fills_and_renders_the_dock() {
        use griff_core::import::import_score_auto;
        use griff_ui_core::{build_chunk, CaptureInputs};
        let score = import_score_auto(include_bytes!("../assets/demo.mid")).expect("demo imports");
        let chunk_json = |id: &str| {
            let inputs = CaptureInputs {
                id,
                created_at: "t",
                updated_at: "t",
                ..CaptureInputs::default()
            };
            serde_json::to_string(&build_chunk(&score, 0, &inputs).expect("builds")).expect("json")
        };

        let mut app = demo_app();
        assert!(!app.show_dock, "the dock starts hidden");
        app.load_corpus(&[
            chunk_json("riff_a"),
            chunk_json("riff_b"),
            "not json".to_owned(),
        ]);
        assert_eq!(
            app.corpus.len(),
            2,
            "valid chunks parse; the bad entry is skipped"
        );
        assert!(app.show_dock, "loading a corpus opens the dock");

        // The dock draws a CPU frame without panicking, with a filter applied.
        app.corpus_filter.query = "riff_a".to_owned();
        let ctx = egui::Context::default();
        let _frame = ctx.run(egui::RawInput::default(), |ctx| app.corpus_dock(ctx));
    }

    #[test]
    #[allow(deprecated)] // egui 0.34 flags `Context::run`; it still drives a CPU frame.
    fn curate_decides_renames_and_retags_the_selected_chunk() {
        use griff_core::import::import_score_auto;
        use griff_ui_core::{build_chunk, CaptureInputs};
        let score = import_score_auto(include_bytes!("../assets/demo.mid")).expect("demo imports");
        let chunk_json = |id: &str| {
            let inputs = CaptureInputs {
                id,
                created_at: "t",
                updated_at: "t",
                ..CaptureInputs::default()
            };
            serde_json::to_string(&build_chunk(&score, 0, &inputs).expect("builds")).expect("json")
        };

        let mut app = demo_app();
        app.load_corpus(&[chunk_json("riff_a")]);
        app.selected = Some("riff_a".to_owned());

        // approve → reviewer decision; rename → title; retag → exact tag set.
        app.curate("riff_a", &CurationAction::Decide(CurationDecision::Approve))
            .expect("approves");
        assert_eq!(
            app.corpus.first().expect("chunk").reviewer,
            Some(ReviewerDecision::Accepted),
        );
        app.curate("riff_a", &CurationAction::Rename("My Riff".to_owned()))
            .expect("renames");
        assert_eq!(app.corpus.first().expect("chunk").title, "My Riff");
        app.curate(
            "riff_a",
            &CurationAction::Retag(vec!["clean_riff".to_owned()]),
        )
        .expect("retags");
        assert_eq!(
            app.corpus.first().expect("chunk").tags,
            vec![SwancoreTag::CleanRiff]
        );

        // an unknown id is rejected, not a panic.
        app.curate("ghost", &CurationAction::Decide(CurationDecision::Reject))
            .expect_err("no such chunk");

        // the inspector renders for the selection without panicking.
        let ctx = egui::Context::default();
        let _frame = ctx.run(egui::RawInput::default(), |ctx| app.corpus_dock(ctx));
    }

    mod fuzz {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #![proptest_config(ProptestConfig::with_cases(96))]

            /// Any sequence of key presses leaves the cockpit paintable and in
            /// range: the input -> reduce -> resolve -> paint loop never panics
            /// or desyncs, whatever the user mashes.
            #[test]
            fn arbitrary_key_sequences_keep_the_cockpit_sound(
                picks in prop::collection::vec(0usize..15, 0..24),
            ) {
                // Ten mapped keys plus five unmapped ones, so the fuzz mixes
                // real intents with inert noise.
                let palette = [
                    Key::Space, Key::ArrowLeft, Key::ArrowRight, Key::ArrowUp, Key::ArrowDown,
                    Key::Plus, Key::Minus, Key::OpenBracket, Key::CloseBracket, Key::Home,
                    Key::I, Key::A, Key::Tab, Key::F1, Key::Enter,
                ];
                let mut app = two_section_app();
                for p in picks {
                    press(&mut app, palette[p % palette.len()]);
                }

                let sections = app.ctx.section_starts.len();
                prop_assert!(
                    sections == 0 || app.vp.sel_section < sections,
                    "selection {} escaped the {} sections",
                    app.vp.sel_section,
                    sections
                );

                let scene = resolve(&app.view, &app.analysis, &app.vp, GridSize { cols: 100, rows: 30 });
                prop_assert_eq!(scene.plane.len(), 100 * 30, "the plane stays cols×rows");

                let ctx = egui::Context::default();
                let input = egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        egui::pos2(0.0, 0.0),
                        egui::vec2(900.0, 420.0),
                    )),
                    ..Default::default()
                };
                #[allow(deprecated)]
                let _frame = ctx.run(input, |c| {
                    egui::CentralPanel::default().show(c, |ui| app.paint(ui));
                });
            }
        }
    }
}
