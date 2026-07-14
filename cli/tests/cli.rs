//! S0 golden/characterization tests for the `griff` CLI.
//!
//! Every CLI subcommand (`import`, `inspect`, `export`, `classify`,
//! `structure`, `phrases`, `generate`, `complement`) is run against every
//! committed fixture and its stdout/stderr pinned to a golden snapshot. These
//! tests describe what the CLI *does* today; they must not be "fixed" by
//! changing expectations without a deliberate re-bless.
//!
//! Regenerate fixtures: `cargo test -p griff-cli -- --ignored regenerate`
//! Re-bless snapshots:  `GRIFF_BLESS=1 cargo test -p griff-cli`

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::missing_assert_message,
    clippy::str_to_string
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
fn structure_golden() {
    for (name, _) in fixtures() {
        let path = fixture_path(name);
        let out = griff(&["structure", path.to_str().unwrap()], path.to_str());
        assert_golden(&format!("structure__{name}"), &out);
    }
}

#[test]
fn phrases_golden() {
    for (name, _) in fixtures() {
        let path = fixture_path(name);
        let out = griff(&["phrases", path.to_str().unwrap()], path.to_str());
        assert_golden(&format!("phrases__{name}"), &out);
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

#[test]
fn generate_golden() {
    for (name, _) in fixtures() {
        let src = fixture_path(name);
        let dst = env::temp_dir().join(format!("griff_s0_generate_{name}.mid"));
        fs::remove_file(&dst).ok();

        let out = griff(
            &["generate", src.to_str().unwrap(), dst.to_str().unwrap()],
            dst.to_str(),
        );
        let out = out.replace(src.to_str().unwrap(), "<SRC>");
        assert_golden(&format!("generate__{name}"), &out);

        assert!(
            dst.exists(),
            "generate must have written the output file for `{name}`"
        );
        fs::remove_file(&dst).ok();
    }
}

#[test]
fn complement_golden() {
    for (name, _) in fixtures() {
        let src = fixture_path(name);
        let dst = env::temp_dir().join(format!("griff_s0_complement_{name}.mid"));
        fs::remove_file(&dst).ok();

        let out = griff(
            &["complement", src.to_str().unwrap(), dst.to_str().unwrap()],
            dst.to_str(),
        );
        let out = out.replace(src.to_str().unwrap(), "<SRC>");
        assert_golden(&format!("complement__{name}"), &out);

        assert!(
            dst.exists(),
            "complement must have written the output file for `{name}`"
        );
        fs::remove_file(&dst).ok();
    }
}

/// Pins the non-default argument path: a different `--mode`, an explicit
/// `--seed`, and a negative `--offset` (which clap must accept as a value).
#[test]
fn complement_options_golden() {
    let src = fixture_path("simple_4_4");
    let dst = env::temp_dir().join("griff_s0_complement_opts.mid");
    fs::remove_file(&dst).ok();

    let out = griff(
        &[
            "complement",
            src.to_str().unwrap(),
            dst.to_str().unwrap(),
            "--mode",
            "counter_melody",
            "--seed",
            "42",
            "--offset",
            "-12",
        ],
        dst.to_str(),
    );
    let out = out.replace(src.to_str().unwrap(), "<SRC>");
    assert_golden("complement__opts_counter_melody", &out);

    assert!(dst.exists(), "complement must have written the output file");
    fs::remove_file(&dst).ok();
}

/// Pins the invalid-mode failure path: a clear argument error, no output file.
#[test]
fn complement_invalid_mode_golden() {
    let src = fixture_path("simple_4_4");
    let dst = env::temp_dir().join("griff_s0_complement_invalid.mid");
    fs::remove_file(&dst).ok();

    let out = griff(
        &[
            "complement",
            src.to_str().unwrap(),
            dst.to_str().unwrap(),
            "--mode",
            "bad_mode",
        ],
        dst.to_str(),
    );
    let out = out.replace(src.to_str().unwrap(), "<SRC>");
    assert_golden("error__complement_invalid_mode", &out);

    assert!(
        !dst.exists(),
        "complement must not write an output file when the mode is invalid"
    );
}

/// A missing input file is observable CLI behavior worth pinning.
#[test]
fn missing_file_golden() {
    let missing = fixture_path("does_not_exist");
    let out = griff(&["import", missing.to_str().unwrap()], missing.to_str());
    assert_golden("error__missing_file", &out);
}

/// S16 Phase 2: the `--rhythm-*` transport syntax drives the explicit
/// scheduler end to end, and the expansion artifact is byte-stable and
/// seed-independent on the generation axis.
#[test]
fn rhythm_pattern_generate_golden() {
    let src = fixture_path("simple_4_4");
    let dst = env::temp_dir().join("griff_s16_pattern_generate.mid");
    let art = env::temp_dir().join("griff_s16_pattern_expansion.json");
    fs::remove_file(&dst).ok();
    fs::remove_file(&art).ok();

    let args = |seed: &'static str, rhythm_seed: &'static str| {
        vec![
            "generate".to_string(),
            src.to_str().unwrap().to_string(),
            dst.to_str().unwrap().to_string(),
            "--bars".to_string(),
            "4".to_string(),
            "--seed".to_string(),
            seed.to_string(),
            "--rhythm-kernel".to_string(),
            "X.X/XX./.XX".to_string(),
            "--rhythm-fractal-depth".to_string(),
            "1".to_string(),
            "--rhythm-density-bps".to_string(),
            "8000".to_string(),
            "--rhythm-seed".to_string(),
            rhythm_seed.to_string(),
            "--rhythm-traversal".to_string(),
            "snake".to_string(),
            "--rhythm-unit".to_string(),
            "1/16".to_string(),
            "--rhythm-tail".to_string(),
            "rest-pad".to_string(),
            "--emit-rhythm-expansion".to_string(),
            art.to_str().unwrap().to_string(),
        ]
    };

    let run = |seed: &'static str, rhythm_seed: &'static str| -> (String, String) {
        let argv_owned = args(seed, rhythm_seed);
        let refs: Vec<&str> = argv_owned.iter().map(String::as_str).collect();
        let out = griff(&refs, dst.to_str());
        let artifact = fs::read_to_string(&art).expect("artifact written");
        (out, artifact)
    };

    let (out, artifact_first) = run("42", "17");
    let out = out.replace(src.to_str().unwrap(), "<SRC>");
    let out = out.replace(art.to_str().unwrap(), "<ART>");
    assert_golden("generate__rhythm_pattern", &out);
    assert!(dst.exists(), "the riff must be written");

    // Byte-stable across identical runs.
    let (_, artifact_second) = run("42", "17");
    assert_eq!(
        artifact_first, artifact_second,
        "the artifact must be byte-stable"
    );

    // The generation seed never touches the expansion artifact…
    let (_, artifact_other_seed) = run("7", "17");
    assert_eq!(
        artifact_first, artifact_other_seed,
        "--seed must not change the expansion artifact"
    );

    // …while the rhythm seed changes the structure.
    let (_, artifact_other_rhythm_seed) = run("42", "99");
    assert_ne!(
        artifact_first, artifact_other_rhythm_seed,
        "--rhythm-seed must change the pruning"
    );

    fs::remove_file(&dst).ok();
    fs::remove_file(&art).ok();
}

/// A broken kernel literal leaves as its registry code at the offending flag.
#[test]
fn rhythm_pattern_ragged_kernel_golden() {
    let src = fixture_path("simple_4_4");
    let dst = env::temp_dir().join("griff_s16_pattern_ragged.mid");

    let out = griff(
        &[
            "generate",
            src.to_str().unwrap(),
            dst.to_str().unwrap(),
            "--rhythm-kernel",
            "X.X/XX",
            "--rhythm-fractal-depth",
            "0",
            "--rhythm-traversal",
            "row-major",
            "--rhythm-unit",
            "1/16",
        ],
        dst.to_str(),
    );
    let out = out.replace(src.to_str().unwrap(), "<SRC>");
    assert_golden("generate__rhythm_pattern_ragged", &out);
    assert!(!dst.exists(), "no riff may be written on a broken kernel");
}
