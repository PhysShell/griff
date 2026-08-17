//! The exact canonical score text — level 2 (`docs/swang/exact-score-text.md`).
//!
//! This module is the writer half: a canonical `Score` in, one canonical
//! level-2 document out. The parser half arrives later and separately.
//!
//! Written incrementally by slice. SWG-4A-03 covers the header, `ppqn`, and
//! the master timeline; tracks and everything below them are SWG-4A-04 and
//! after, and a `Score` carrying them is refused with
//! [`ExactWriteError::NotYetWritten`] rather than half-written.

mod error;
mod write;

pub use error::ExactWriteError;
pub use write::write_score;
