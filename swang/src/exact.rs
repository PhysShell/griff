//! The exact canonical score text — level 2 (`docs/swang/exact-score-text.md`).
//!
//! This module is the writer half: a canonical `Score` in, one canonical
//! level-2 document out. The parser half arrives later and separately.
//!
//! Built in slices — SWG-4A-03 the transport, 4A-04 the structure, 4A-05 the
//! leaves and metadata — and **complete** since the last of them. There is no
//! part of the canonical tree it declines to spell, so the only refusal left
//! is [`ExactWriteError::OutsideWriterDomain`]: your `Score` breaks an
//! invariant `griff-core` itself declares.

mod error;
mod write;

pub use error::ExactWriteError;
pub use write::write_score;
