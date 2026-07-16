//! Renderer-agnostic UI core for griff previews (ADR-0016).
//!
//! One core, consumed by every frontend — the `ratatui` terminal preview
//! (`griff-preview`, S8) today, an `egui` cockpit next (ADR-0024/0027) — so a
//! change verified in one cannot silently diverge in another. Four layers, plus
//! the curation persistence seam:
//!
//! - [`view`] — a [`view::PianoRollView`]: a pure projection of a
//!   [`griff_core::score::Score`] onto a pitch × tick plane. No rendering, no
//!   I/O.
//! - [`analysis`] — named sections and structure metrics for the inspector.
//! - [`viewport`] — the interaction core: a semantic [`viewport::Intent`] and the
//!   pure reducer [`viewport::Viewport::apply`] (scroll, zoom, selection,
//!   playback). The *behavioural* half both frontends share.
//! - [`scene`] — [`scene::resolve`] lifts all layout math into one placed grid of
//!   [`scene::SceneCell`]s tagged by a semantic [`scene::CellRole`]. The *visual*
//!   half a renderer blits, not interprets.
//! - [`curation`] — the chunk-record persistence ops (decide / tag / rename /
//!   split / merge) a curating frontend drives.
//!
//! Renderers live in their own crates and depend only on this core's `Scene` and
//! `Intent` for the grid — never on the layout math — so they cannot re-derive
//! or diverge from it. Tests assert the `Scene` and the reducer headlessly; the
//! `ratatui` cell-snapshot is a human-readable witness of the same scene.

pub mod analysis;
pub mod capture;
pub mod corpus;
pub mod curation;
pub mod dock;
pub mod generate;
pub mod history;
pub mod playback;
pub mod scene;
pub mod theme;
pub mod view;
pub mod viewport;

pub use analysis::{analyze, Analysis, Section};
pub use capture::{build_chunk, detect_boundaries, CaptureInputs};
pub use corpus::{build_manifest, manifest_from_jsons};
pub use dock::{filter_chunks, CorpusFilter, CorpusStats};
pub use generate::{generate_set, CandidateRow, CandidateSet, SetSummary};
pub use scene::{resolve, CellRole, GridSize, Scene, SceneCell};
pub use theme::{cell_style, contrast_ratio, CellStyle, Rgb, Theme};
pub use view::{build_view, Lane, NoteRect, PianoRollView};
pub use viewport::{Intent, Step, ViewContext, Viewport};
