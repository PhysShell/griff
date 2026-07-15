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
//!   * Every diagnostic carries an `SWG`-prefixed registry code and a span
//!     inside the source (`start <= end <= len`).
//!   * On `Ok`: `format` emits canonical text that reparses to the same AST
//!     (law 3) and is its own fixed point (law 2).

use griff_swang::syntax::{format, header_level, parse, LANGUAGE_LEVEL};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|source: &str| {
    match header_level(source) {
        Ok(level) => assert!(
            (1..=LANGUAGE_LEVEL).contains(&level),
            "an accepted level is in the supported range"
        ),
        Err(d) => assert!(d.code.starts_with("SWG"), "typed header code: {}", d.code),
    }

    let len = u32::try_from(source.len()).unwrap_or(u32::MAX);
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
                assert!(
                    d.code.starts_with("SWG"),
                    "stable registry code, never ad hoc: {}",
                    d.code
                );
                assert!(
                    d.span.start <= d.span.end && d.span.end <= len,
                    "the span stays inside the source: {:?} in {len}",
                    d.span
                );
            }
        }
    }
});
