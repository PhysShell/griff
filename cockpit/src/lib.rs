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

use eframe::egui::{self, Align2, Color32, CornerRadius, FontId, Key, Rect};

use griff_core::classify::BarClass;
use griff_core::corpus::{ChunkMeta, ReviewerDecision, RightsStatus, StyleCohort, SwancoreTag};
use griff_core::import::import_score_auto;
use griff_core::score::Score;
use griff_ui_core::scene::{resolve, CellRole, GridSize, SceneCell, GUTTER};
use griff_ui_core::curation::{decide_record, rename_record, set_tags, tag_palette};
use griff_ui_core::viewport::CurationDecision;
use griff_ui_core::{
    analyze, build_chunk, build_view, filter_chunks, Analysis, CaptureInputs, CorpusFilter,
    CorpusStats, Intent, PianoRollView, Step, ViewContext, Viewport,
};

/// Pixel width of one grid cell.
const CELL_W: f32 = 9.0;
/// Pixel height of one grid cell (also the section-band row height).
const CELL_H: f32 = 16.0;

// ── palette (mirrors preview/design/index.html) ─────────────────────────────
const BG_BLACK_ROW: Color32 = Color32::from_rgb(0x20, 0x20, 0x24);
const STROKE: Color32 = Color32::from_rgb(0x46, 0x46, 0x4d);
const GRID_BAR: Color32 = Color32::from_rgb(0x45, 0x45, 0x4e);
const BOUNDARY: Color32 = Color32::from_rgb(0xff, 0x5d, 0x6c);
const PLAYHEAD: Color32 = Color32::from_rgb(0xff, 0xcf, 0x4d);
const PANEL: Color32 = Color32::from_rgb(0x24, 0x24, 0x27);
const LABEL_DIM: Color32 = Color32::from_rgb(0x9a, 0x9a, 0xa2);
const LABEL_FAINT: Color32 = Color32::from_rgb(0x6e, 0x6e, 0x76);

/// Colour for a bar classification (section marks and the section band).
const fn class_color(class: BarClass) -> Color32 {
    match class {
        BarClass::Riff => Color32::from_rgb(0x16, 0x68, 0xdc),
        BarClass::Breakdown => Color32::from_rgb(0xcf, 0x13, 0x22),
        BarClass::Solo => Color32::from_rgb(0xd4, 0x88, 0x06),
        BarClass::Clean => Color32::from_rgb(0x38, 0x9e, 0x0d),
        BarClass::Unknown => Color32::from_rgb(0x6e, 0x6e, 0x76),
    }
}

/// Note-lane colour, cycled by lane index (six lanes, then it wraps).
const fn lane_color(lane: u16) -> Color32 {
    match lane % 6 {
        0 => Color32::from_rgb(0xff, 0x7a, 0x45),
        1 => Color32::from_rgb(0x36, 0xcf, 0xc9),
        2 => Color32::from_rgb(0x92, 0x54, 0xde),
        3 => Color32::from_rgb(0x40, 0x96, 0xff),
        4 => Color32::from_rgb(0x73, 0xd1, 0x3d),
        _ => Color32::from_rgb(0xf7, 0x59, 0xab),
    }
}

/// The fill colour for a placed cell, or `None` to leave the panel background
/// showing (text-only or truly empty cells).
fn role_color(role: CellRole, shade: bool) -> Option<Color32> {
    match role {
        CellRole::Empty => shade.then_some(BG_BLACK_ROW),
        CellRole::Separator => Some(STROKE),
        CellRole::GridLine => Some(GRID_BAR),
        CellRole::SectionMark(class) => Some(class_color(class)),
        CellRole::BoundaryMark => Some(BOUNDARY),
        CellRole::Note(lane) => Some(lane_color(lane)),
        CellRole::Playhead => Some(PLAYHEAD),
        CellRole::PitchLabel => None,
        CellRole::BandFill { class, selected } => {
            let base = class_color(class);
            Some(if selected {
                base
            } else {
                base.gamma_multiply(0.55)
            })
        }
        CellRole::BandHeader => Some(PANEL),
    }
}

/// The glyph colour for a textual cell, or `None` when the cell draws as a
/// solid block (no glyph).
const fn glyph_color(role: CellRole) -> Option<Color32> {
    match role {
        CellRole::PitchLabel => Some(LABEL_DIM),
        CellRole::BandHeader => Some(LABEL_FAINT),
        _ => None,
    }
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
            Some(ReviewerDecision::NeedsReview) => "? ",
            None => "",
        };
        let dup = if chunk.duplicate.is_some() { "≈ " } else { "" };
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
    let current = options.iter().find(|opt| opt.0 == *value).map_or("", |opt| opt.1);
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
            cohort: 0,             // core
            rights_status: 3,      // copyrighted — the safe default until stated
            acquisition: 0,        // community tab
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
            .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' })
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
    js_sys::Date::new_0().to_iso_string().as_string().unwrap_or_default()
}

/// The current time as an RFC3339 timestamp, from the system clock (no `chrono`:
/// Howard Hinnant's civil-from-days over the Unix epoch).
#[cfg(not(target_arch = "wasm32"))]
fn now_rfc3339() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
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
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' })
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
    serde_json::to_value(tag).ok()?.as_str().map(ToOwned::to_owned)
}

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
}

impl CockpitApp {
    /// Builds the app from a view and its analysis; `title` labels the window.
    #[must_use]
    pub fn new(view: PianoRollView, analysis: Analysis, title: String) -> Self {
        let ctx = build_context(&view, &analysis);
        let mut vp = Viewport::new(&ctx, view.high_pitch);
        vp.show_inspector = false; // the capture panel starts hidden (the `i` key shows it)
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
        }
    }

    /// Builds the app over an imported `score`, **keeping it** behind the view so
    /// Capture works on the displayed file (ADR-0026). Use this for every real
    /// score — the CLI entry and the web demo — so the initially-shown score is
    /// capturable without first re-loading it; `title` labels the window.
    #[must_use]
    pub fn from_score(score: Score, title: String) -> Self {
        let mut app = Self::new(build_view(&score), analyze(&score), title);
        app.score = Some(score);
        app
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
        let view = build_view(&score);
        let analysis = analyze(&score);
        let ctx = build_context(&view, &analysis);
        let mut vp = Viewport::new(&ctx, view.high_pitch);
        vp.show_inspector = false;
        self.view = view;
        self.analysis = analysis;
        self.ctx = ctx;
        self.vp = vp;
        self.form.seed_from(&source);
        self.title = source;
        self.score = Some(score);
        self.fitted = false;
        Ok(())
    }

    /// Captures the focused track of the loaded score as a `chunk.json` string
    /// (ADR-0026), through the shared [`griff_ui_core::capture::build_chunk`] —
    /// byte-compatible with what `griff manifest` reads.
    ///
    /// # Errors
    /// Returns a message if no score is loaded yet, or if measuring fails.
    pub fn capture_json(&self, inputs: &CaptureInputs<'_>) -> Result<String, String> {
        let score = self.score.as_ref().ok_or_else(|| "no score loaded".to_owned())?;
        let chunk = build_chunk(score, self.analysis.focus_track, inputs)?;
        serde_json::to_string_pretty(&chunk).map_err(|err| err.to_string())
    }

    /// Builds and saves a `chunk.json` for the focused track from the form,
    /// recording the outcome in the form's status line.
    fn do_capture(&mut self) {
        let now = now_rfc3339();
        let result = self.capture_json(&self.form.inputs(&now, &now)).and_then(|json| {
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
        let idx = self.corpus.iter().position(|c| c.id.0 == id).ok_or("no such chunk")?;
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

    /// Curates the selected chunk and persists it back to the corpus, recording
    /// the outcome in the dock's status line.
    fn apply_curation(&mut self, action: &CurationAction) {
        let Some(id) = self.selected.clone() else {
            return;
        };
        self.dock_status = Some(match self.curate(&id, action) {
            Ok(json) => {
                let filename = chunk_filename(&id);
                match persist_chunk(&filename, &json) {
                    Ok(()) => format!("saved {filename}"),
                    Err(err) => format!("save failed: {err}"),
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
        let mut action: Option<CurationAction> = None;
        egui::Window::new("corpus").default_width(360.0).show(ctx, |ui| {
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
                let top: Vec<String> =
                    tags.iter().take(4).map(|(tag, n)| format!("{tag:?}×{n}")).collect();
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
            egui::ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
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
                    inspector(ui, chunk, &mut self.rename_buf, &mut action);
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
        if let Some(action) = action {
            self.apply_curation(&action);
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
        // `c` toggles the corpus dock — a shell concern, not a viewport `Intent`.
        if ctx.input(|i| i.key_pressed(Key::C)) {
            self.show_dock = !self.show_dock;
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
            self.vp.fit(u32::from(cols.saturating_sub(GUTTER)), &self.ctx);
            self.fitted = true;
        }
        if self.vp.playing {
            self.vp.autoscroll(u32::from(cols.saturating_sub(GUTTER)));
        }

        let scene = resolve(&self.view, &self.analysis, &self.vp, GridSize { cols, rows });
        let painter = ui.painter();
        for col in 0..scene.cols {
            if let Some(cell) = scene.band_cell(col) {
                paint_cell(painter, origin, col, 0, *cell);
            }
        }
        for row in 0..scene.rows {
            for col in 0..scene.cols {
                if let Some(cell) = scene.plane_cell(row, col) {
                    paint_cell(painter, origin, col, row.saturating_add(1), *cell);
                }
            }
        }
    }
}

/// Paints one placed cell at grid position (`col`, `vis_row`).
fn paint_cell(painter: &egui::Painter, origin: egui::Pos2, col: u16, vis_row: u16, cell: SceneCell) {
    let x = origin.x + f32::from(col) * CELL_W;
    let y = origin.y + f32::from(vis_row) * CELL_H;
    let rect = Rect::from_min_size(egui::pos2(x, y), egui::vec2(CELL_W, CELL_H));
    if let Some(bg) = role_color(cell.role, cell.shade) {
        painter.rect_filled(rect, CornerRadius::ZERO, bg);
    }
    if cell.glyph != ' ' {
        if let Some(fg) = glyph_color(cell.role) {
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                cell.glyph,
                FontId::monospace(CELL_H * 0.8),
                fg,
            );
        }
    }
}

impl eframe::App for CockpitApp {
    // eframe's default `update` wraps this in a central panel; we draw the
    // resolved scene straight into the provided `ui`.
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // Apply a file or capture request the page handed us, if any.
        #[cfg(target_arch = "wasm32")]
        web::drain(self);
        let egui_ctx = ui.ctx().clone();
        if self.handle_input(&egui_ctx) {
            egui_ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if self.vp.playing {
            let dt = f64::from(egui_ctx.input(|i| i.stable_dt)).min(0.1);
            self.vp.advance_playback(dt, &self.ctx);
            egui_ctx.request_repaint();
        }
        self.paint(ui);
        if self.vp.show_inspector {
            self.capture_panel(&egui_ctx);
        }
        if self.show_dock {
            self.corpus_dock(&egui_ctx);
        }
    }
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
        let document = web_sys::window().and_then(|w| w.document()).ok_or("no document")?;
        let parts = js_sys::Array::of1(&JsValue::from_str(contents));
        let blob = web_sys::Blob::new_with_str_sequence(&parts).map_err(|_| "blob".to_owned())?;
        let url =
            web_sys::Url::create_object_url_with_blob(&blob).map_err(|_| "object url".to_owned())?;
        let anchor = document
            .create_element("a")
            .and_then(|el| el.dyn_into::<web_sys::HtmlAnchorElement>().map_err(Into::into))
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
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

    use super::*;
    use eframe::egui;

    #[test]
    fn every_bar_class_has_a_distinct_colour() {
        let classes = [
            BarClass::Riff,
            BarClass::Breakdown,
            BarClass::Solo,
            BarClass::Clean,
            BarClass::Unknown,
        ];
        for (i, a) in classes.iter().enumerate() {
            for b in classes.iter().skip(i + 1) {
                assert_ne!(class_color(*a), class_color(*b), "{a:?}/{b:?} share a colour");
            }
        }
    }

    #[test]
    fn lane_colour_cycles_and_never_panics() {
        assert_eq!(lane_color(0), lane_color(6), "the six-lane palette wraps");
        assert_ne!(lane_color(0), lane_color(1));
        let _ = lane_color(u16::MAX); // index math stays in range
    }

    #[test]
    fn notes_fill_but_text_labels_do_not() {
        assert!(role_color(CellRole::Note(0), false).is_some());
        assert!(role_color(CellRole::Playhead, false).is_some());
        assert!(
            role_color(CellRole::PitchLabel, false).is_none(),
            "labels draw as text, not a filled block"
        );
        assert_eq!(role_color(CellRole::Empty, false), None);
        assert_eq!(
            role_color(CellRole::Empty, true),
            Some(BG_BLACK_ROW),
            "black-key rows shade"
        );
    }

    #[test]
    fn selected_band_differs_from_unselected() {
        let sel = role_color(
            CellRole::BandFill {
                class: BarClass::Riff,
                selected: true,
            },
            false,
        );
        let unsel = role_color(
            CellRole::BandFill {
                class: BarClass::Riff,
                selected: false,
            },
            false,
        );
        assert_ne!(sel, unsel);
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
            assert_eq!(key_to_intent(key), Some(intent), "{key:?} should map to {intent:?}");
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
                    NoteRect { onset: 0, end: 480, pitch: 60 },
                    NoteRect { onset: 960, end: 1440, pitch: 64 },
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
        let raw = egui::RawInput { events: vec![key_event(key)], ..Default::default() };
        let mut quit = false;
        let _frame = ctx.run(raw, |ctx| quit = app.handle_input(ctx));
        quit
    }

    fn demo_app() -> CockpitApp {
        use griff_core::import::import_score_auto;
        let score = import_score_auto(include_bytes!("../assets/demo.mid")).expect("demo imports");
        CockpitApp::from_score(score, "demo".to_owned())
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
                    NoteRect { onset: 0, end: 480, pitch: 60 },
                    NoteRect { onset: 3840, end: 4320, pitch: 64 },
                ],
            }],
            tempo_bpm: 120.0,
            bar_count: 4,
        };
        let analysis = Analysis {
            focus_track: 0,
            sections: vec![
                Section { class: BarClass::Riff, bar_start: 0, bar_end: 2, tick_start: 0, tick_end: 3840 },
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
        for (w, h) in [(1.0, 1.0), (20.0, 8.0), (90.0, 30.0), (4000.0, 40.0), (200.0, 3000.0)] {
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
    fn lane_colour_is_periodic_and_six_valued_for_every_index() {
        use std::collections::HashSet;
        let palette: HashSet<(u8, u8, u8)> = (0u16..6)
            .map(|lane| {
                let c = lane_color(lane);
                (c.r(), c.g(), c.b())
            })
            .collect();
        assert_eq!(palette.len(), 6, "the six lanes are distinct");
        for lane in 0u16..=u16::MAX {
            assert_eq!(lane_color(lane), lane_color(lane % 6), "lane {lane} follows the mod-6 palette");
            let c = lane_color(lane);
            assert!(palette.contains(&(c.r(), c.g(), c.b())), "lane {lane} is one of the six colours");
        }
    }

    #[test]
    fn multiple_lanes_paint_in_distinct_lane_colours() {
        use std::collections::HashSet;
        use griff_ui_core::{Lane, NoteRect, Section};
        let view = PianoRollView {
            ppq: 480,
            tick_start: 0,
            tick_end: 1920,
            low_pitch: 52,
            high_pitch: 72,
            bar_lines: vec![0, 1920],
            lanes: vec![
                Lane { name: "lead".to_owned(), notes: vec![NoteRect { onset: 0, end: 480, pitch: 60 }] },
                Lane {
                    name: "harmony".to_owned(),
                    notes: vec![NoteRect { onset: 0, end: 480, pitch: 67 }],
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
        let scene = resolve(&app.view, &app.analysis, &app.vp, GridSize { cols: 60, rows: 24 });

        let lane_colours: HashSet<(u8, u8, u8)> = scene
            .plane
            .iter()
            .filter_map(|cell| match cell.role {
                CellRole::Note(_) => role_color(cell.role, cell.shade).map(|c| (c.r(), c.g(), c.b())),
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
        app.load("multi.mid".to_owned(), include_bytes!("../assets/multi_track.mid"))
            .expect("multi_track.mid imports");
        assert_eq!(app.title(), "multi.mid", "the title follows the loaded source");
        assert_ne!(app.title(), demo_title, "the source changed");
        assert!(
            app.view.lanes.len() >= 2,
            "the multi-track file loads its several tracks, got {}",
            app.view.lanes.len()
        );
    }

    #[test]
    fn load_rejects_unparseable_bytes_and_keeps_the_score() {
        let mut app = demo_app();
        let kept = app.view.lanes.len();
        let err = app
            .load("junk.mid".to_owned(), b"definitely not a score")
            .expect_err("garbage must not import");
        assert!(err.contains("junk.mid"), "the error names the bad source: {err}");
        assert_eq!(app.view.lanes.len(), kept, "a failed load leaves the current score intact");
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
        let json = app.capture_json(&inputs).expect("captures the displayed score");
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
        app.load("path/to/Cool Riff.mid".to_owned(), include_bytes!("../assets/demo.mid"))
            .expect("loads");
        assert_eq!(app.form.id, "cool_riff", "the id is a slug of the file stem");
        assert_eq!(app.form.title, "Cool Riff");
        assert_eq!(app.form.filename, "Cool Riff.mid");

        let now = "2026-01-01T00:00:00Z";
        let json = app.capture_json(&app.form.inputs(now, now)).expect("captures from the form");
        assert!(json.contains("cool_riff"), "the captured chunk uses the form id");
    }

    #[test]
    #[allow(deprecated)] // egui 0.34 flags `Context::run`; it still drives a CPU frame.
    fn load_corpus_fills_and_renders_the_dock() {
        use griff_core::import::import_score_auto;
        use griff_ui_core::{build_chunk, CaptureInputs};
        let score = import_score_auto(include_bytes!("../assets/demo.mid")).expect("demo imports");
        let chunk_json = |id: &str| {
            let inputs =
                CaptureInputs { id, created_at: "t", updated_at: "t", ..CaptureInputs::default() };
            serde_json::to_string(&build_chunk(&score, 0, &inputs).expect("builds")).expect("json")
        };

        let mut app = demo_app();
        assert!(!app.show_dock, "the dock starts hidden");
        app.load_corpus(&[chunk_json("riff_a"), chunk_json("riff_b"), "not json".to_owned()]);
        assert_eq!(app.corpus.len(), 2, "valid chunks parse; the bad entry is skipped");
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
            let inputs =
                CaptureInputs { id, created_at: "t", updated_at: "t", ..CaptureInputs::default() };
            serde_json::to_string(&build_chunk(&score, 0, &inputs).expect("builds")).expect("json")
        };

        let mut app = demo_app();
        app.load_corpus(&[chunk_json("riff_a")]);
        app.selected = Some("riff_a".to_owned());

        // approve → reviewer decision; rename → title; retag → exact tag set.
        app.curate("riff_a", &CurationAction::Decide(CurationDecision::Approve)).expect("approves");
        assert_eq!(
            app.corpus.first().expect("chunk").reviewer,
            Some(ReviewerDecision::Accepted),
        );
        app.curate("riff_a", &CurationAction::Rename("My Riff".to_owned())).expect("renames");
        assert_eq!(app.corpus.first().expect("chunk").title, "My Riff");
        app.curate("riff_a", &CurationAction::Retag(vec!["clean_riff".to_owned()])).expect("retags");
        assert_eq!(app.corpus.first().expect("chunk").tags, vec![SwancoreTag::CleanRiff]);

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
