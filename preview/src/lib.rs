//! Standalone preview for griff scores (S8).
//!
//! This crate turns a [`griff_core::score::Score`] into something a human can
//! *see*: a piano-roll. It is split into pure, headless-testable layers plus an
//! interactive front-end:
//!
//! - [`view`] — derives a [`view::PianoRollView`] (note rectangles laid out on a
//!   pitch × tick plane, plus bar gridlines) from a score. No rendering, no I/O.
//! - [`analysis`] — derives named sections (via [`griff_core::classify`]) and
//!   structure metrics (via [`griff_core::structure`]) for the inspector.
//! - [`render`] — rasterises a [`view::PianoRollView`] into a fixed-size grid of
//!   text rows ([`render::render_frame`]). No terminal, no I/O.
//! - [`tui`] — an interactive `ratatui` piano-roll ([`tui::App`]) with scroll,
//!   zoom, named-section bands, a metrics inspector, and a playhead. The same
//!   render path drives the live terminal and a headless [`tui::App::snapshot`].
//!
//! The binary (`griff-preview`) is thin glue: read a `.mid`, import it via the
//! core MIDI importer, build the view + analysis, and either launch the TUI or
//! print a headless snapshot frame. MIDI playback is a later increment.

pub mod analysis;
pub mod render;
pub mod tui;
pub mod view;
pub mod viewport;

pub use analysis::{analyze, Analysis, Section};
pub use render::render_frame;
pub use tui::App;
pub use view::{build_view, Lane, NoteRect, PianoRollView};
pub use viewport::{Intent, Step, ViewContext, Viewport};
