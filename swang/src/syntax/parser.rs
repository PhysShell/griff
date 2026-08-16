//! The parser, one module per language level.
//!
//! Level 1 is frozen (spec §3); a later level adds `v2` beside `v1` rather
//! than editing it.

pub(crate) mod v1;
