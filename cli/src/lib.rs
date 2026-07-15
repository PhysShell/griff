//! `griff-cli` internal library — the filesystem seam shared by the `griff`
//! binary and experimental A/B harnesses (see [`generation_input`]).
//!
//! This is **not** a stable public API: it exists so tooling reuses the exact
//! production corpus→generation compiler instead of reimplementing (and
//! drifting from) it. The compiler itself lives in
//! [`griff_core::generation_input`], shared with every frontend; what remains
//! here is the CLI's corpus-*directory* I/O over it. Everything here is
//! `#[doc(hidden)]` and stability-exempt.

#![doc(hidden)]

pub mod generation_input;
pub mod rhythm_pattern;

pub use griff_core::generation_input::primary_voice_note_count;
