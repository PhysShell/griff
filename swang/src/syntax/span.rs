//! Source spans: the half-open byte ranges a diagnostic points at.

/// A half-open byte range into the source text. Fixed-width offsets by the
/// determinism law (spec §1.2): no platform-sized integers in anything a
/// frontend may serialize.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    /// First byte of the range.
    pub start: u32,
    /// One past the last byte.
    pub end: u32,
}

/// Builds a [`Span`] from byte indices, saturating into the fixed-width
/// offsets the determinism law demands.
pub(crate) fn span_of(start: usize, end: usize) -> Span {
    Span {
        start: u32::try_from(start).unwrap_or(u32::MAX),
        end: u32::try_from(end).unwrap_or(u32::MAX),
    }
}
