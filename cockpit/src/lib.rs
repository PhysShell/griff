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
use griff_ui_core::scene::{resolve, CellRole, GridSize, SceneCell, GUTTER};
use griff_ui_core::{Analysis, Intent, PianoRollView, Step, ViewContext, Viewport};

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

/// The egui cockpit application: a `Scene` renderer over the shared core.
#[derive(Debug)]
pub struct CockpitApp {
    view: PianoRollView,
    analysis: Analysis,
    title: String,
    vp: Viewport,
    ctx: ViewContext,
    fitted: bool,
}

impl CockpitApp {
    /// Builds the app from a view and its analysis; `title` labels the window.
    #[must_use]
    pub fn new(view: PianoRollView, analysis: Analysis, title: String) -> Self {
        let ctx = build_context(&view, &analysis);
        let vp = Viewport::new(&ctx, view.high_pitch);
        Self {
            view,
            analysis,
            title,
            vp,
            ctx,
            fitted: false,
        }
    }

    /// The source label shown in the window title.
    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Drains the frame's key presses into the reducer; returns whether the
    /// user asked to quit.
    fn handle_input(&mut self, ctx: &egui::Context) -> bool {
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
    use wasm_bindgen::prelude::*;

    use griff_core::import::import_score_auto;
    use griff_ui_core::{analyze, build_view};

    use crate::CockpitApp;

    /// A tiny MIDI baked into the app so the web front paints a real,
    /// importer-parsed score on first load — no file pick yet (that is Slice 3).
    const DEMO_SCORE: &[u8] = include_bytes!("../assets/demo.mid");

    /// Builds the cockpit over the baked demo score.
    fn demo_app() -> CockpitApp {
        let score = import_score_auto(DEMO_SCORE).expect("the baked demo score must import");
        CockpitApp::new(build_view(&score), analyze(&score), "demo".to_owned())
    }

    /// Boots the cockpit on `canvas`. The page's ES module calls this after the
    /// generated wasm initialises (`wasm-bindgen --target web`); eframe then
    /// drives the frame loop through `requestAnimationFrame`.
    #[wasm_bindgen]
    pub fn start(canvas: web_sys::HtmlCanvasElement) {
        let app = demo_app();
        let options = eframe::WebOptions::default();
        wasm_bindgen_futures::spawn_local(async move {
            eframe::WebRunner::new()
                .start(canvas, options, Box::new(|_cc| Ok(Box::new(app))))
                .await
                .expect("failed to start the cockpit web runner");
        });
    }
}

#[cfg(test)]
mod tests {
    // Tests build views from known-good fixtures and may `expect` on them.
    #![allow(clippy::expect_used)]

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
        use griff_ui_core::{analyze, build_view};
        let score = import_score_auto(include_bytes!("../assets/demo.mid")).expect("demo imports");
        CockpitApp::new(build_view(&score), analyze(&score), "demo".to_owned())
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
}
