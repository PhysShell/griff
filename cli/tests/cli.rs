//! S0 golden/characterization tests for the `griff` CLI.
//!
//! Every CLI subcommand (`import`, `inspect`, `export`, `classify`) is run
//! against every committed fixture and its stdout/stderr pinned to a golden
//! snapshot. These tests describe what the CLI *does* today; they must not be
//! "fixed" by changing expectations without a deliberate re-bless.
//!
//! Regenerate fixtures: `cargo test -p griff-cli -- --ignored regenerate`
//! Re-bless snapshots:  `GRIFF_BLESS=1 cargo test -p griff-cli`

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::missing_assert_message
)]

mod common;

use std::{env, fs};

use common::{assert_golden, fixture_path, fixtures, griff};

/// Committed `.mid` fixtures must match the generator byte-for-byte.
///
/// If this fails after a deliberate generator change, regenerate with
/// `cargo test -p griff-cli -- --ignored regenerate`.
#[test]
fn fixtures_in_sync() {
    for (name, bytes) in fixtures() {
        let path = fixture_path(name);
        let on_disk = fs::read(&path).unwrap_or_else(|_| {
            panic!(
                "missing fixture {}; create it with \
                 `cargo test -p griff-cli -- --ignored regenerate`",
                path.display()
            )
        });
        assert_eq!(
            on_disk, bytes,
            "committed fixture `{name}.mid` is stale; regenerate with \
             `cargo test -p griff-cli -- --ignored regenerate`"
        );
    }
}

/// Not a test: writes the synthetic `.mid` fixtures to disk on demand.
#[test]
#[ignore = "fixture generator; run explicitly to (re)write tests/fixtures"]
fn regenerate_fixtures() {
    for (name, bytes) in fixtures() {
        let path = fixture_path(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, &bytes).unwrap();
    }
}

#[test]
fn import_golden() {
    for (name, _) in fixtures() {
        let path = fixture_path(name);
        let out = griff(&["import", path.to_str().unwrap()], path.to_str());
        assert_golden(&format!("import__{name}"), &out);
    }
}

#[test]
fn inspect_golden() {
    for (name, _) in fixtures() {
        let path = fixture_path(name);
        let out = griff(&["inspect", path.to_str().unwrap()], path.to_str());
        assert_golden(&format!("inspect__{name}"), &out);
    }
}

#[test]
fn classify_golden() {
    for (name, _) in fixtures() {
        let path = fixture_path(name);
        let out = griff(&["classify", path.to_str().unwrap()], path.to_str());
        assert_golden(&format!("classify__{name}"), &out);
    }
}

#[test]
fn export_golden() {
    for (name, _) in fixtures() {
        let src = fixture_path(name);
        let dst = env::temp_dir().join(format!("griff_s0_export_{name}.mid"));
        fs::remove_file(&dst).ok();

        let out = griff(
            &["export", src.to_str().unwrap(), dst.to_str().unwrap()],
            dst.to_str(),
        );
        // The source path also appears nowhere in export stdout, but scrub it
        // too for safety so only the stable byte count remains.
        let out = out.replace(src.to_str().unwrap(), "<SRC>");
        assert_golden(&format!("export__{name}"), &out);

        assert!(
            dst.exists(),
            "export must have written the output file for `{name}`"
        );
        fs::remove_file(&dst).ok();
    }
}

/// A missing input file is observable CLI behavior worth pinning.
#[test]
fn missing_file_golden() {
    let missing = fixture_path("does_not_exist");
    let out = griff(&["import", missing.to_str().unwrap()], missing.to_str());
    assert_golden("error__missing_file", &out);
}
