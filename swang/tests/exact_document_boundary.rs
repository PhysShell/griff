//! SWG-4A-02: the four boundaries the exact document must not cross.
//!
//! `ExactScoreDocument` is a **transient** syntax form. The danger it carries
//! is not that it will be wrong — it is that it will be useful, and quietly
//! become a second durable musical model beside `griff_core::Score`. Every
//! step of that happening looks like an improvement at the time: make it
//! public so a caller can build one, derive `Serialize` so a tool can dump
//! one, let the evaluator take one instead of a `Score`.
//!
//! So the shape tests live inside the crate, where the type is visible, and
//! these four live out here, where its *absence* from the public world is
//! what can be observed:
//!
//! A. `griff-core` does not depend on `griff-swang`;
//! B. the document is not re-exported from `syntax`, or anywhere public;
//! C. nothing outside the AST mentions it — not the evaluator, not the
//!    pattern compiler, not `griff-core`, not the CLI;
//! D. it has no serialization format of its own.
//!
//! B, C and D read source text, which is a blunt instrument, so each one
//! also checks that its search *can* find something — a witness that cannot
//! fail is not a witness.

// Reason: integration-test code. `unwrap`/`expect`/`panic` abort loudly with
// a clear message, which is exactly what a test harness wants.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::missing_assert_message
)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::{env, fs};

/// The workspace root, from this crate's manifest directory.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the swang crate sits one level below the workspace root")
        .to_path_buf()
}

fn read(relative: &str) -> String {
    let path = workspace_root().join(relative);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("{} must be readable: {e}", path.display()))
}

/// The code of a Rust source file: every line that is not a comment.
///
/// The claims below are about what the module *does*, and a doc comment
/// explaining that it does not serialize is not an implementation of
/// serialization. Searching raw text would make the prose that documents a
/// boundary indistinguishable from a breach of it — which is exactly the
/// failure this witness family exists to avoid, one level down.
fn code_of(source: &str) -> String {
    source
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Whether `haystack` mentions `needle` as a whole token, so that a longer
/// identifier containing it does not count as a mention.
///
/// Byte-wise rather than char-wise: every identifier these witnesses look for
/// is ASCII, and any non-ASCII byte is a boundary anyway.
fn mentions(haystack: &str, needle: &str) -> bool {
    let bytes = haystack.as_bytes();
    let is_word = |b: Option<&u8>| b.is_some_and(|&b| b.is_ascii_alphanumeric() || b == b'_');
    haystack.match_indices(needle).any(|(at, _)| {
        let before = at.checked_sub(1).and_then(|i| bytes.get(i));
        let after = bytes.get(at.saturating_add(needle.len()));
        !is_word(before) && !is_word(after)
    })
}

// ── A. the dependency arrow points one way ──────────────────────────────────

#[test]
fn griff_core_does_not_depend_on_griff_swang() {
    // The lowering direction is text -> document -> builder -> Score. If the
    // model crate could see the syntax form, the arrow could quietly reverse
    // and the "transient" claim would be unenforceable.
    //
    // Asked of the resolver rather than of `Cargo.toml`: a manifest grep
    // cannot see a dependency arriving through a feature, a rename, a target
    // table, or a path two crates long. `cargo tree` walks what is actually
    // built, dev-dependencies included.
    let core_tree = dependency_tree("griff-core");
    assert!(
        !core_tree.contains("griff-swang"),
        "griff-core must not depend on griff-swang, directly or transitively:\n{core_tree}"
    );

    // The same command against the other crate must find the arrow that does
    // exist, or the check above proves only that the command printed nothing.
    let swang_tree = dependency_tree("griff-swang");
    assert!(
        swang_tree.contains("griff-core"),
        "the witness must be able to see a real dependency, and \
         griff-swang -> griff-core is the one it should find:\n{swang_tree}"
    );
}

/// The full dependency tree of one workspace crate, as the resolver sees it.
fn dependency_tree(package: &str) -> String {
    let output = Command::new(env::var("CARGO").unwrap_or_else(|_| String::from("cargo")))
        .args(["tree", "--package", package, "--prefix", "none"])
        .current_dir(workspace_root())
        .output()
        .expect("cargo tree must run");
    assert!(
        output.status.success(),
        "cargo tree -p {package} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("cargo tree emits UTF-8")
}

// ── B. the document is not part of the public surface ───────────────────────

/// Whether the code of a `syntax` module re-exports the level-2 **AST**.
///
/// The property is "the transient AST is not re-exported", not "the token
/// `v2` never appears". SWG-4A-06 adds level-2 parsing and formatting, and a
/// `pub use parser::v2::…` or `pub use format::v2::…` is precisely the wiring
/// that task exists to add — it exposes no part of the document. A check that
/// banned the version number outright would fail on legitimate work while its
/// message went on claiming the document had escaped, which is worse than not
/// checking: a red test that lies costs more than a missing one.
fn re_exports_the_level_two_ast(syntax_code: &str) -> bool {
    syntax_code.contains("pub use ast::v2")
}

#[test]
fn the_re_export_check_reads_the_boundary_and_not_the_version_number() {
    // Both halves, because narrowing a check is only safe if it still fires
    // on the thing it was narrowed away from.
    assert!(
        re_exports_the_level_two_ast("pub use ast::v2::ExactScoreDocument;"),
        "re-exporting the document itself is a breach and must be seen"
    );
    assert!(
        re_exports_the_level_two_ast("pub use ast::v2 as level_two;"),
        "so is re-exporting the module under any name"
    );
    assert!(
        !re_exports_the_level_two_ast("pub use parser::v2::parse_exact;"),
        "a level-2 parser is not the AST, and 4A-06 must not be blocked by \
         this test"
    );
    assert!(
        !re_exports_the_level_two_ast("pub use format::v2::format_exact;"),
        "nor a level-2 formatter"
    );
}

#[test]
fn the_exact_document_is_not_re_exported() {
    let syntax = code_of(&read("swang/src/syntax.rs"));
    assert!(
        syntax.contains("pub use ast::v1::{"),
        "the witness must be able to see a real re-export: if this line \
         moved, the check below stopped meaning anything"
    );
    assert!(
        !re_exports_the_level_two_ast(&syntax),
        "`syntax` re-exports level 1's AST only; the exact document and its \
         module stay crate-private"
    );
    assert!(
        !mentions(&syntax, "ExactScoreDocument"),
        "and it is not named on the public surface by any other route — a \
         rename in the re-export path does not get around this"
    );

    let lib = code_of(&read("swang/src/lib.rs"));
    assert!(
        !mentions(&lib, "ExactScoreDocument"),
        "nor re-exported from the crate root"
    );
}

#[test]
fn the_module_itself_is_crate_private() {
    let ast = code_of(&read("swang/src/syntax/ast.rs"));
    assert!(
        ast.contains("pub(crate) mod v1;"),
        "the witness must be able to see the visibility it is checking for"
    );
    assert!(
        ast.contains("pub(crate) mod v2;"),
        "v2 is declared beside v1, and at the same crate-private visibility"
    );
    assert!(
        !ast.contains("pub mod v2;"),
        "not `pub mod` — that would put the transient form in the public API"
    );
}

// ── C. nothing outside the AST is coupled to it ─────────────────────────────

/// Every `.rs` file under `relative`, at any depth.
fn rust_files_under(relative: &str) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![workspace_root().join(relative)];
    while let Some(dir) = pending.pop() {
        let entries = fs::read_dir(&dir)
            .unwrap_or_else(|e| panic!("{} must be readable: {e}", dir.display()));
        for entry in entries {
            let path = entry.expect("a readable directory entry").path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                found.push(path);
            }
        }
    }
    found
}

#[test]
fn no_production_code_outside_the_ast_mentions_the_document() {
    // 4A-02 adds a type; it does not wire one in.
    //
    // Two different prohibitions, because they really are different, and
    // collapsing them into one crate-wide scan would break the next two
    // tasks. `griff-core` and the CLI must never see this type at any depth,
    // now or later — so those are walked recursively and a module added
    // tomorrow is covered without anyone remembering to list it. Inside
    // `griff-swang` the prohibition is *not* crate-wide: 4A-08's parser will
    // legitimately produce an `ExactScoreDocument` and 4A-09's builder will
    // legitimately consume one. They are the pipeline this task exists to
    // feed. Banning the name across `swang/src` would fail on exactly the
    // work 4A-02 is a prerequisite for — the same shape of defect as a check
    // that banned the token `v2` outright.
    let mut checked = 0_usize;

    for tree in ["core/src", "cli/src"] {
        let files = rust_files_under(tree);
        assert!(
            files.len() >= 3,
            "{tree} should hold several modules; a walk that found {} has \
             stopped being a witness",
            files.len()
        );
        for path in files {
            let text = code_of(&fs::read_to_string(&path).expect("a readable source file"));
            assert!(
                !mentions(&text, "ExactScoreDocument"),
                "{} must not mention the transient syntax form: the document \
                 never crosses into the model crate or the CLI",
                path.display()
            );
            checked = checked.saturating_add(1);
        }
    }

    // The named `griff-swang` surfaces that stay clear of it: the evaluator
    // and the pattern compiler keep taking what they took before, the exact
    // writer works from a `Score`, and neither entry point re-exports it.
    for file in [
        "swang/src/eval.rs",
        "swang/src/pattern_compile.rs",
        "swang/src/lib.rs",
        "swang/src/exact.rs",
        "swang/src/syntax.rs",
    ] {
        let text = code_of(&read(file));
        assert!(
            !mentions(&text, "ExactScoreDocument"),
            "{file} must not mention the transient syntax form"
        );
        checked = checked.saturating_add(1);
    }

    assert!(
        checked >= 15,
        "only {checked} files were examined; the witness is not reaching the \
         tree it claims to cover"
    );
}

// ── D. no serialization format of its own ───────────────────────────────────

#[test]
fn the_document_has_no_serialization_format() {
    // The exact score text *is* the document's serialization. A second one —
    // a JSON schema, a bincode dump, a hash contract — would be a format
    // nobody agreed to freeze, and the first bug report against it would be
    // about a compatibility promise this task never made.
    let v2 = code_of(&read("swang/src/syntax/ast/v2.rs"));
    for forbidden in ["Serialize", "Deserialize", "serde"] {
        assert!(
            !mentions(&v2, forbidden),
            "the exact document must not derive or implement {forbidden}"
        );
    }
    assert!(
        v2.contains("#[derive(Debug, Clone, PartialEq, Eq)]"),
        "the witness must be able to see the derive list it is constraining, \
         and to see it in code rather than in a comment about code"
    );
    assert!(
        !mentions(&v2, "Hash"),
        "and no hashing contract either — nothing keys anything by this form"
    );
}

#[test]
fn the_document_performs_no_conversion() {
    // No `From`, no `to_score`, no `build`, no `validate`. Lowering is
    // 4A-09's whole job; a conversion here would either duplicate it or
    // pre-empt it, and either way the refusals would stop being attributable.
    let v2 = code_of(&read("swang/src/syntax/ast/v2.rs"));
    assert!(
        v2.contains("pub(crate) struct ExactScoreDocument {"),
        "the witness must be able to see the declarations it is constraining"
    );
    for forbidden in [
        "impl From",
        "fn to_score",
        "fn build",
        "fn validate",
        "fn parse",
        "fn format",
        "fn normalize",
        "fn canonicalize",
    ] {
        assert!(
            !v2.contains(forbidden),
            "the exact document must not carry `{forbidden}`"
        );
    }
    assert!(
        !mentions(&v2, "griff_core"),
        "the syntax form names no canonical type at all: it is raw shapes, \
         so that `Pitch`, `Velocity` and their checked constructors stay \
         4A-09's business"
    );
}
