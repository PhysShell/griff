//! Core musical model for griff.
//!
//! All musical data flows through the types defined here.
//! MIDI bytes are produced and consumed only at the import/export boundary;
//! the rest of the codebase works exclusively with these structured types.

pub mod boundary;
pub mod classify;
pub mod closure;
pub mod complement;
pub mod corpus;
pub mod event;
pub mod feature;
pub mod fretboard;
pub mod generate;
pub mod gp;
pub mod midi;
pub mod score;
pub mod scoring;
pub mod slice;
pub mod structure;
