//! Standalone `ratatui` preview for griff scores (S8).
//!
//! The renderer-agnostic UI core — view-model, interaction core, scene, and the
//! curation persistence seam — now lives in [`griff_ui_core`] (ADR-0016). This
//! crate is the `ratatui` frontend over it:
//!
//! - [`render`] — rasterises a [`PianoRollView`] into a fixed-size grid of text
//!   rows ([`render::render_frame`]). No terminal, no I/O.
//! - [`tui`] — an interactive `ratatui` piano-roll ([`tui::App`]) with scroll,
//!   zoom, named-section bands, a metrics inspector, and a playhead. The same
//!   render path drives the live terminal and a headless [`tui::App::snapshot`].
//!
//! The core layers are re-exported so the `griff_preview::{view, analysis,
//! scene, viewport, curation}` paths stay stable for the binary and its tests;
//! `tui` and `main` consume the core through these aliases, unchanged by the
//! extraction.
//!
//! The binary (`griff-preview`) is thin glue: read a `.mid` / `.gp`, import it
//! via the core importer, build the view + analysis, and either launch the TUI
//! or print a headless snapshot frame.

pub mod render;
pub mod tui;

pub use griff_ui_core::analysis::{self, analyze, Analysis, Section};
pub use griff_ui_core::curation;
pub use griff_ui_core::scene::{self, resolve, CellRole, GridSize, Scene, SceneCell};
pub use griff_ui_core::theme::{self, cell_style, CellStyle, Rgb, Theme};
pub use griff_ui_core::view::{self, build_view, Lane, NoteRect, PianoRollView};
pub use griff_ui_core::viewport::{self, Intent, Step, ViewContext, Viewport};

pub use render::render_frame;
pub use tui::App;
