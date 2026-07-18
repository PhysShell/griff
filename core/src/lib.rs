//! Core musical model for griff.
//!
//! All musical data flows through the types defined here.
//! MIDI bytes are produced and consumed only at the import/export boundary;
//! the rest of the codebase works exclusively with these structured types.

pub mod boundary;
pub mod candidate_chain;
pub mod classify;
pub mod closure;
pub mod complement;
pub mod corpus;
pub mod curation;
pub mod dump;
pub mod event;
pub mod feature;
pub mod fretboard;
pub mod generate;
pub mod generation_input;
pub mod gesture;
#[cfg(feature = "gp")]
pub mod gp;
pub mod harmony;
pub mod import;
pub mod ingest;
pub mod layered_path;
pub mod midi;
pub mod novelty;
pub mod pitch;
pub mod rerank;
pub mod score;
pub mod scoring;
pub mod similarity;
pub mod slice;
pub mod split;
pub mod structure;
pub mod syncopation;
pub mod technique;
pub mod tonal;
pub mod unfold;
