//! The surface AST, one module per language level.
//!
//! Level 1 is frozen (spec §3); a later level adds `v2` beside `v1` rather
//! than editing it.

pub(crate) mod v1;

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::missing_assert_message
)]
mod v2_tests;
