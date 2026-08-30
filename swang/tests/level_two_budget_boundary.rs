//! SWG-INF-06: the level-2 budget stays off every level-1 path.
//!
//! Spec §5.11 allows input bounds at level 2 only, because level 1's
//! acceptance set is frozen (§5.5): a bound that rejected a source level 1
//! accepts would be an observable change to a frozen level. That makes "no
//! level-1 path consults the budget" a contract, not a coding preference,
//! and a contract nobody checks is a comment.
//!
//! These witnesses read the shipped source rather than the behaviour,
//! because the failure they guard against is a *call site*, and by the time
//! behaviour shows it the frozen level has already moved. Comment-only lines
//! are stripped first: this file's own subject matter is discussed in the
//! prose of the very modules it scans, and a witness that reads prose as
//! code is a witness that fires on a docstring.

// Reason: integration-test code. `unwrap`/`expect`/`panic` abort loudly with
// a clear message, which is exactly what a test harness wants.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::missing_assert_message
)]

use std::ffi::OsStr;
use std::fs::{read_dir, read_to_string};
use std::path::{Component, Path, PathBuf};

/// The level-1 parse path, end to end: the frozen header pre-parser, the
/// lexer it hands off to, the one parser module, and the formatter.
const LEVEL_ONE_PATH: &[(&str, &str)] = &[
    // `syntax.rs` is the crate's public re-export point and the natural home
    // for shared dispatch, which makes it the one file most worth scanning —
    // a budget consulted there, *before* the level 1/2 branch, would be
    // consulted on every level-1 parse while both of this suite's other
    // witnesses stayed silent.
    ("syntax.rs", include_str!("../src/syntax.rs")),
    ("header.rs", include_str!("../src/syntax/header.rs")),
    ("lexer.rs", include_str!("../src/syntax/lexer.rs")),
    ("parser/v1.rs", include_str!("../src/syntax/parser/v1.rs")),
    ("format/v1.rs", include_str!("../src/syntax/format/v1.rs")),
    ("ast/v1.rs", include_str!("../src/syntax/ast/v1.rs")),
    ("eval.rs", include_str!("../src/eval.rs")),
];

/// Every name the budget exports. A level-1 module naming any of them is
/// consulting a level-2 bound.
const BUDGET_NAMES: &[&str] = &[
    // Qualified, because bare `limits` is an ordinary English word and an
    // ordinary Rust identifier. Every real route to the module — `use
    // crate::syntax::limits::…`, `super::limits::…`, an inline
    // `limits::MAX_TOKENS` — carries the `::`; prose and unrelated locals do
    // not.
    "limits::",
    "Level2Budget",
    "Level2ResourceLimits",
    "MAX_SOURCE_BYTES",
    "MAX_TOKENS",
    "MAX_NESTING_DEPTH",
    "MAX_DIAGNOSTICS",
    "SWG0509",
    // Bare method names are still absent — they are ordinary identifiers,
    // and banning them failed CI on an unrelated `fn enter_block(...)`.
    // What is banned is the *call*, which is punctuated: a definition, a
    // binding, or a sentence never contains `.name(`.
    //
    // This closes the route the earlier form left open — a helper hands
    // back a `Level2Budget`, type inference supplies the type, and the call
    // site spells no name at all. Matching the call needs neither the type
    // nor a Rust parser.
    //
    // rustfmt breaks a long chain before the dot, so `.name(` stays
    // contiguous even across lines; the repository's own formatting is what
    // makes a plain substring enough.
    ".admit_source(",
    ".admit_token(",
    ".enter_block(",
    ".leave_block(",
    ".admit_diagnostic(",
];

/// The one line `syntax.rs` may contain: the module declaration itself.
const DECLARATION: &str = "mod limits;";

/// Strips comment-only lines, so prose about a name is not read as a use of
/// it, and — in `syntax.rs` alone — the bare module declaration.
///
/// Exempting `syntax.rs` wholesale was the earlier form, and it was wrong in
/// a way that only shows up next task: the file that declares the module is
/// also the file where level dispatch will live, so a blanket exemption
/// makes the shared dispatch point the one place a budget could be consulted
/// unwatched. The declaration is permitted by exact line; every other line
/// is scanned like any other module's.
fn code_of(source: &str) -> String {
    strip(source, false)
}

/// `code_of`, optionally also dropping the module declaration.
fn strip(source: &str, is_root: bool) -> String {
    source
        .lines()
        .filter(|line| {
            let trimmed = line.trim_start();
            !(trimmed.starts_with("//") || is_root && trimmed.trim_end() == DECLARATION)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Whether this path is the crate's `syntax.rs` root module.
fn is_root(shown: &str) -> bool {
    shown.trim_start_matches('/') == "syntax.rs"
}

/// Whether `haystack` mentions `needle` as a whole token, so a longer
/// identifier containing it does not count as a mention.
///
/// A boundary is required only on the sides where the needle's own edge is a
/// word character. `limits::` ends in punctuation and is followed by the
/// name it qualifies, so demanding a non-word character after it would match
/// nothing at all.
fn mentions(haystack: &str, needle: &str) -> bool {
    let bytes = haystack.as_bytes();
    let is_word = |b: Option<&u8>| b.is_some_and(|&b| b.is_ascii_alphanumeric() || b == b'_');
    let edge_is_word = |b: Option<&u8>| b.is_some_and(|&b| b.is_ascii_alphanumeric() || b == b'_');
    let needs_before = edge_is_word(needle.as_bytes().first());
    let needs_after = edge_is_word(needle.as_bytes().last());
    haystack.match_indices(needle).any(|(at, _)| {
        let before = at.checked_sub(1).and_then(|i| bytes.get(i));
        let after = bytes.get(at.saturating_add(needle.len()));
        !(needs_before && is_word(before) || needs_after && is_word(after))
    })
}

/// Whether a source names any budget identifier, after stripping the lines a
/// scan must not read.
fn names_the_budget(source: &str) -> bool {
    let code = code_of(source);
    BUDGET_NAMES.iter().any(|name| mentions(&code, name))
}

#[test]
fn the_exemption_is_decided_by_path_components_not_by_a_slashed_string() {
    // `to_string_lossy` performs no separator normalisation, so a path built
    // on Windows stringifies as `syntax\limits.rs` and never equals the
    // literal `"syntax/limits.rs"`. The exempt budget module then stops
    // being exempt and the guard fails on the one file it must ignore —
    // which reads like a boundary breach and is not one.
    //
    // CI is `ubuntu-latest` on every job, so this is latent rather than
    // live. It is still wrong on a platform this crate builds for, and the
    // cure is to compare paths as paths rather than to translate separators
    // by hand.
    let native = Path::new("syntax").join("limits.rs");
    assert!(
        is_exempt(&native),
        "the budget module is exempt on any platform"
    );
    let also = Path::new("syntax").join("tests.rs");
    assert!(is_exempt(&also), "so are its tests");
    assert!(
        !is_exempt(Path::new("syntax").join("parser").join("v1.rs").as_path()),
        "nothing else is"
    );
}

#[test]
fn a_generic_word_in_prose_or_a_string_is_not_a_budget_reference() {
    // This witness reads source text, not a parsed tree, so its precision is
    // lexical and heuristic — never syntactic. That is an accepted trade:
    // pulling in a Rust parser to guard a boundary test would cost more than
    // the boundary is worth.
    //
    // What the trade must not buy is a guard that trips on English. `limits`
    // is an ordinary word and an ordinary identifier, and a check that fails
    // CI because someone wrote "no limits apply" in a string is a check the
    // next person weakens — at which point the frozen level loses its guard
    // for a reason that had nothing to do with the frozen level.
    for benign in [
        r#"const _NOTE: &str = "no limits apply here";"#,
        "const _N: u8 = 1; // nothing to do with limits",
        "let limits = compute_ui_limits();",
        "//! This module documents the limits elsewhere.",
        "struct Delimiters;",
        // A bare method name is an ordinary identifier and is not the
        // signal — banning one failed CI on an unrelated definition. The
        // call form is watched instead, which is why a definition and a
        // binding still read as benign here while `b.enter_block(at)` does
        // not.
        "fn enter_block(&mut self) -> bool { true }",
        "let admit_source = compute();",
    ] {
        assert!(
            !names_the_budget(benign),
            "benign source must not count as a budget reference: {benign}"
        );
    }

    // And what it must still buy is every real way to reach the module.
    for real in [
        "const _P: bool = limits::MAX_TOKENS > 0;",
        "use crate::syntax::limits::Level2Budget;",
        "use super::limits::Level2ResourceLimits;",
        "let b = Level2Budget::declared();",
        "if bytes > MAX_SOURCE_BYTES { refuse() }",
    ] {
        assert!(
            names_the_budget(real),
            "a real budget reference must count: {real}"
        );
    }
}

#[test]
fn a_budget_call_through_an_inferred_receiver_is_a_budget_reference() {
    // Matching names alone leaves one route open, and review named it
    // exactly: a helper hands back a `Level2Budget`, type inference supplies
    // the type, and the call site never spells the module, the type, or a
    // constant.
    //
    //     let mut b = budget_from_context();
    //     b.admit_token(at)?;
    //
    // No budget name appears, so the guard stayed silent while a level-1
    // module spent a level-2 bound. That is the failure this file exists to
    // refuse, reached by the one path it did not watch.
    //
    // The cure is not a parser. A *call* carries punctuation — `.name(` —
    // and a definition, a binding, or a sentence does not. That is why the
    // bare identifiers were removed and these are safe to add: the marker
    // that matched `fn enter_block(...)` is not the marker that matches
    // `b.enter_block(at)`.
    for call in [
        "let mut b = budget_from_context();\n    b.admit_source(src, at)?;",
        "let mut b = budget_from_context();\n    b.admit_token(at)?;",
        "b.enter_block(at)?;",
        "b.leave_block();",
        "self.budget.admit_diagnostic(at)?;",
        // rustfmt breaks a long chain before the dot, which keeps `.name(`
        // contiguous on the continuation line. The repository's own
        // formatting is therefore what makes a plain substring sufficient,
        // and no tolerance for optional whitespace is needed.
        "some_budget\n    .admit_token(at)\n    .map_err(one)?;",
    ] {
        assert!(
            names_the_budget(call),
            "a budget call through an inferred receiver must count: {call}"
        );
    }

    // And the bare identifiers must stay unmatched, because being noisy is
    // why they were dropped. A call marker that also fired on these would
    // have re-imported the false positives it was meant to shed.
    for benign in [
        "fn admit_token(&mut self, at: Span) -> bool { true }",
        "fn enter_block(&mut self) -> bool { true }",
        "let enter_block = compute();",
        "let admit_source = compute();",
        r#"const _NOTE: &str = "admit_token is explained below";"#,
    ] {
        assert!(
            !names_the_budget(benign),
            "a bare method identifier must not count: {benign}"
        );
    }
}

#[test]
fn no_level_one_module_consults_the_level_two_budget() {
    for (name, source) in LEVEL_ONE_PATH {
        let code = strip(source, is_root(name));
        for budget_name in BUDGET_NAMES {
            assert!(
                !mentions(&code, budget_name),
                "{name} names `{budget_name}`. Level 1's acceptance set is \
                 frozen (§5.5); a declared bound reaching it is a narrowing \
                 of a frozen level, not an optimisation."
            );
        }
    }
}

#[test]
fn the_witness_can_fail() {
    // A boundary test that cannot fail proves nothing about the boundary. If
    // `mentions` or `code_of` ever stopped seeing real code, the witness
    // above would pass for the wrong reason and no one would learn of it.
    let planted = "fn lex() { let b = Level2Budget::new(limits::declared()); }";
    assert!(names_the_budget(planted));
    assert!(mentions(&code_of(planted), "Level2Budget"));
    assert!(mentions(&code_of(planted), "limits::"));
    let prose = "//! The Level2Budget is discussed here but never called.";
    assert!(!mentions(&code_of(prose), "Level2Budget"));
    // The declaration is permitted in the root module and nowhere else, and
    // permitting it must not swallow a use on the same subject.
    assert!(!mentions(&strip("mod limits;", true), "limits"));
    assert!(mentions(&strip("mod limits;", false), "limits"));
    assert!(mentions(
        &strip("mod limits;\nlet b = Level2Budget::declared();", true),
        "Level2Budget"
    ));
}

#[test]
fn every_shipped_module_but_the_budget_itself_is_scanned() {
    // A hardcoded file list rots into a list of the files someone
    // remembered, so the real guard walks the crate instead: every shipped
    // `.rs` under `swang/src` is read, and any that names the budget must be
    // one of the three that legitimately may.
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/src");
    let mut scanned = 0_u32;
    for path in rust_sources(Path::new(root)) {
        let relative = path.strip_prefix(root).unwrap_or(&path);
        if is_exempt(relative) {
            continue;
        }
        // Only ever for the failure message — never for the decision.
        let shown = relative.display().to_string();
        let code = strip(
            &read_to_string(&path).expect("a shipped source"),
            relative == Path::new("syntax.rs"),
        );
        scanned = scanned.saturating_add(1);
        for budget_name in BUDGET_NAMES {
            assert!(
                !mentions(&code, budget_name),
                "{shown} names `{budget_name}`, and it is not one of the \
                 modules allowed to: {EXEMPT:?}"
            );
        }
    }
    assert!(scanned > 10, "only {scanned} modules were walked");
}

/// The two modules that may name the budget freely: the budget itself and
/// its tests. `syntax.rs` is deliberately absent — it is scanned, with only
/// its `mod limits;` declaration permitted.
///
/// When SWG-4A-06 adds level-2-specific modules (`parser/v2.rs` and the
/// like), those go on this list explicitly, one at a time. Shared dispatch
/// never joins it: a budget consulted before the level branch is a level-1
/// bound, whatever file it lives in.
const EXEMPT: &[&[&str]] = &[&["syntax", "limits.rs"], &["syntax", "tests.rs"]];

/// Whether a path relative to `swang/src` is one of the exempt modules.
///
/// Compared component by component. A separator is not the same character
/// on every platform this crate builds for, so the exemption must never be
/// decided by a string that contains one.
fn is_exempt(relative: &Path) -> bool {
    EXEMPT.iter().any(|parts| {
        relative
            .components()
            .map(Component::as_os_str)
            .eq(parts.iter().copied().map(OsStr::new))
    })
}

/// Every `.rs` file under `dir`, recursively, in a deterministic order.
fn rust_sources(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut entries: Vec<PathBuf> = read_dir(dir)
        .expect("the crate source directory")
        .map(|e| e.expect("a directory entry").path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            found.extend(rust_sources(&path));
        } else if path.extension().is_some_and(|e| e == "rs") {
            found.push(path);
        }
    }
    found
}
