//! Shared support for the S0 core characterization suite.
//!
//! The fixtures are the *same bytes* the CLI suite generates and commits; they
//! are embedded here at compile time so the library and the binary are pinned
//! against one identical corpus. Regenerate them from the CLI crate:
//! `cargo test -p griff-cli -- --ignored regenerate`.

// Reason: integration-test support code; loud failure is the desired behavior.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::missing_assert_message
)]

use std::{env, fs, path::PathBuf};

macro_rules! fixture {
    ($name:literal) => {
        (
            $name,
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../cli/tests/fixtures/",
                $name,
                ".mid"
            ))
            .as_slice(),
        )
    };
}

/// The canonical S0 fixture corpus, embedded at compile time.
pub(crate) fn fixtures() -> Vec<(&'static str, &'static [u8])> {
    vec![
        fixture!("simple_4_4"),
        fixture!("seven_eight"),
        fixture!("multi_track"),
        fixture!("tempo_change"),
    ]
}

fn snapshot_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("snapshots")
        .join(format!("{name}.txt"))
}

/// Compare `actual` to the stored golden snapshot, or write it when
/// `GRIFF_BLESS=1`.
pub(crate) fn assert_golden(name: &str, actual: &str) {
    let path = snapshot_path(name);
    if env::var("GRIFF_BLESS").as_deref() == Ok("1") {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, actual).unwrap();
        return;
    }
    let expected = fs::read_to_string(&path).unwrap_or_else(|_| {
        panic!(
            "missing golden snapshot {}; create it with \
             `GRIFF_BLESS=1 cargo test -p griff-core`",
            path.display()
        )
    });
    assert_eq!(
        actual, expected,
        "core output drifted from golden snapshot `{name}`. If intended, \
         re-bless with `GRIFF_BLESS=1 cargo test -p griff-core`."
    );
}
