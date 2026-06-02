//! Standalone preview for griff scores (S8, first slice).
//!
//! This crate turns a [`griff_core::score::Score`] into something a human can
//! *see*: a piano-roll. It is split into two pure, headless-testable layers:
//!
//! - [`view`] — derives a [`view::PianoRollView`] (note rectangles laid out on a
//!   pitch × tick plane, plus bar gridlines) from a score. No rendering, no I/O.
//! - [`render`] — rasterises a [`view::PianoRollView`] into a fixed-size grid of
//!   text rows ([`render::render_frame`]). No terminal, no I/O.
//!
//! The binary (`griff-preview`) is thin glue: read a `.mid`, import it via the
//! core MIDI importer, build the view, and print a rendered frame. An
//! interactive `ratatui` front-end (scroll/zoom) and MIDI playback are later
//! increments that build on these same two layers (see S8).

pub mod render;
pub mod view;

pub use render::render_frame;
pub use view::{build_view, Lane, NoteRect, PianoRollView};
