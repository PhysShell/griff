//! The frozen §1.1 header pre-parser and the language level it pins.
//!
//! Frozen means frozen: this module's acceptance set never changes across
//! releases, so a newer script is refused here rather than half-parsed.

use std::str::from_utf8;

use super::diagnostic::Diagnostic;
use super::span::span_of;

/// The newest language level this build parses (spec §1.1). Levels are
/// additive-only and never enter any content hash.
///
/// SWG-4A-06 raised this to 2. The pre-parser below is unchanged — that is
/// what "frozen" means — and what moved is the range it reports, which has
/// to be the range the build actually has or `SWG0001` would name a lie.
pub const LANGUAGE_LEVEL: u32 = 2;

/// The frozen §1.1 pre-parser: reads at most 64 bytes of the first line and
/// returns the pinned language level.
///
/// # Errors
/// `SWG0003` for a byte-order mark, `SWG0002` for a missing or malformed
/// header (wrong casing, wrong spacing, leading zeros, a sign, more than
/// nine digits, a missing EOL, or a first line longer than 64 bytes), and
/// `SWG0001` — naming the supported range — for a level newer than
/// [`LANGUAGE_LEVEL`].
pub fn header_level(source: &str) -> Result<u32, Diagnostic> {
    let bytes = source.as_bytes();
    if bytes.get(..3) == Some(b"\xef\xbb\xbf") {
        return Err(Diagnostic {
            code: "SWG0003",
            span: span_of(0, 3),
            message: "byte-order mark before the header; Swang is UTF-8 without a BOM".to_owned(),
        });
    }
    let window = bytes.len().min(HEADER_WINDOW);
    let malformed = || Diagnostic {
        code: "SWG0002",
        span: span_of(0, window),
        message: "missing or malformed header line; a script begins `swang <level>`".to_owned(),
    };
    let Some(lf) = bytes.iter().take(HEADER_WINDOW).position(|&b| b == b'\n') else {
        return Err(malformed());
    };
    let mut line = bytes.get(..lf).unwrap_or_default();
    if let Some((b'\r', rest)) = line.split_last() {
        line = rest;
    }
    let digits = line.strip_prefix(b"swang ").ok_or_else(malformed)?;
    let first = digits.first().ok_or_else(malformed)?;
    if !(b'1'..=b'9').contains(first) || digits.len() > 9 || !digits.iter().all(u8::is_ascii_digit)
    {
        return Err(malformed());
    }
    let level: u32 = from_utf8(digits)
        .ok()
        .and_then(|d| d.parse().ok())
        .ok_or_else(malformed)?;
    if level > LANGUAGE_LEVEL {
        return Err(Diagnostic {
            code: "SWG0001",
            span: span_of(6, 6_usize.saturating_add(digits.len())),
            message: format!(
                "language level {level} is newer than this build supports (1..={LANGUAGE_LEVEL})"
            ),
        });
    }
    Ok(level)
}

/// The pre-parser reads at most this many bytes of the first line (spec
/// §1.1, frozen).
pub(crate) const HEADER_WINDOW: usize = 64;
