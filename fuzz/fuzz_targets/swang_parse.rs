#![no_main]

//! Fuzz target: the Swang surface grammar (P1, S16 Phase 3, ADR-0010).
//!
//! Feeds arbitrary UTF-8 to the frozen §1.1 header pre-parser and the full
//! parser, then holds every `Ok` to the formatter laws the suite already
//! pins on hand-written programs (spec §3.5 laws 2–3).
//!
//! Oracle (normalised invariants):
//!   * No panic / hang / unbounded allocation (libFuzzer limits).
//!   * `header_level`: `Ok(level)` in `1..=LANGUAGE_LEVEL` xor a typed
//!     diagnostic.
//!   * `parse`: `Ok(Program)` xor a non-empty `Vec<Diagnostic>`.
//!   * Every diagnostic carries a registry code of exactly the shape
//!     `SWG\d{4}` and a span inside the source (`start <= end <= len`).
//!   * On `Ok`: `format` emits canonical text that reparses to the same AST
//!     (law 3) and is its own fixed point (law 2).
//!   * `parse_document`: the dispatched path — level 1 or level 2 — is
//!     `Ok(Document)` xor a non-empty `Vec<Diagnostic>`, every diagnostic
//!     carries a registry code and an in-source span, and `format_document`
//!     obeys the same two laws whichever level answered.
//!
//! SWG-INF-06 declared level 2's input bounds but could not honestly claim
//! an end-to-end breach oracle: public parsing could not reach a level-2
//! parser at all, so the oracle would have covered a path the binary could
//! not enter. SWG-4A-06 is the task that opens it, so the oracle lands
//! here. A budget breach is a typed `SWG0509` reaching the caller — not an
//! abort, not an allocation death, not a silent success.

use griff_swang::syntax::{
    format, format_document, header_level, parse, parse_document, Diagnostic, Document,
    LANGUAGE_LEVEL,
};
use libfuzzer_sys::fuzz_target;

/// The one diagnostic contract, applied to the header pre-parser and the
/// parser alike: a registry code of exactly the shape `SWG\d{4}` and a span
/// inside the source.
///
/// `starts_with("SWG")` was the weaker form this oracle shipped with, and it
/// accepted `SWG`, `SWGxyz`, and `SWG12345` as registry codes. The registry
/// has one shape (spec §1.5); the oracle now asserts that shape rather than
/// its first three bytes. No regex dependency is needed to say so.
fn assert_diagnostic(d: &Diagnostic, len: u32) {
    let digits = d.code.strip_prefix("SWG");
    assert!(
        digits.is_some_and(|rest| rest.len() == 4 && rest.bytes().all(|b| b.is_ascii_digit())),
        "a registry code is exactly `SWG` and four digits, never ad hoc: {}",
        d.code
    );
    assert!(
        d.span.start <= d.span.end && d.span.end <= len,
        "the span stays inside the source: {:?} in {len}",
        d.span
    );
}

fuzz_target!(|source: &str| {
    let len = u32::try_from(source.len()).unwrap_or(u32::MAX);

    match header_level(source) {
        Ok(level) => assert!(
            (1..=LANGUAGE_LEVEL).contains(&level),
            "an accepted level is in the supported range"
        ),
        Err(d) => assert_diagnostic(&d, len),
    }

    match parse(source) {
        Ok(program) => {
            let canonical = format(&program);
            let reparsed = parse(&canonical)
                .unwrap_or_else(|d| panic!("canonical text must reparse (law 2): {d:?}"));
            assert_eq!(reparsed, program, "parse(format(ast)) == ast (law 3)");
            assert_eq!(
                format(&reparsed),
                canonical,
                "format is its own fixed point (law 2)"
            );
        }
        Err(diagnostics) => {
            assert!(
                !diagnostics.is_empty(),
                "a refusal names at least one diagnostic"
            );
            for d in &diagnostics {
                assert_diagnostic(d, len);
            }
        }
    }

    match parse_document(source) {
        Ok(document) => {
            let canonical = format_document(&document);
            let reparsed = parse_document(&canonical)
                .unwrap_or_else(|d| panic!("canonical text must reparse (law 2): {d:?}"));
            assert_eq!(
                format_document(&reparsed),
                canonical,
                "format_document is its own fixed point (law 2), at either level"
            );
            match (&document, &reparsed) {
                (Document::Pattern(a), Document::Pattern(b)) => {
                    assert_eq!(a, b, "parse(format(ast)) == ast (law 3)");
                }
                (Document::Score(a), Document::Score(b)) => {
                    assert_eq!(a, b, "parse(format(ast)) == ast (law 3)");
                }
                _ => panic!("the canonical text of a document keeps its root"),
            }
        }
        Err(diagnostics) => {
            assert!(
                !diagnostics.is_empty(),
                "a dispatched refusal names at least one diagnostic"
            );
            for d in &diagnostics {
                assert_diagnostic(d, len);
            }
        }
    }
});
