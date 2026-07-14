//! The `--rhythm-*` adapter: kernel literal → `griff-pattern` expansion →
//! `griff-swang` lowering → an explicit palette for the shared generation
//! compiler, plus the versioned expansion artifact (spec §2).
//!
//! This is the **temporary transport syntax** of S16 Phase 2, not early Swang
//! grammar. Errors leave as [`PatternDiagnostic`]s — a stable `SWG____` code,
//! the offending flag, and a message — per the spec's §1.5 registry.

use std::fmt;

use griff_core::event::Ticks;
use griff_core::generate::RhythmTemplate;
use griff_core::score::Score;

/// How the expansion is read into a line (spec §1.9). Mirrors
/// `griff_pattern::Traversal` — mirrored here so clap can derive values
/// without the pattern crate learning about argument parsing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum TraversalChoice {
    /// Rows left-to-right, top-to-bottom.
    RowMajor,
    /// Boustrophedon: alternating rows reverse.
    Snake,
}

/// What happens to an incomplete final bar (spec §1.11).
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum TailChoice {
    /// An incomplete final bar is a typed error — the documented default.
    Reject,
    /// The final bar's missing slots become timed rests.
    RestPad,
}

/// Everything the `--rhythm-*` flags carry, already clap-parsed.
#[derive(Debug, Clone)]
pub struct RhythmPatternArgs {
    /// The kernel literal (`X.X/XX./.XX`).
    pub kernel: String,
    /// Exact expansion depth; doubles as the structural `max_depth`.
    pub fractal_depth: u8,
    /// Density decay in basis points, when pruning is asked for.
    pub density_bps: Option<u16>,
    /// The pruning seed — independent of the generation seed by law.
    pub rhythm_seed: Option<u64>,
    /// The explicit traversal.
    pub traversal: TraversalChoice,
    /// The time unit literal (`1/16`).
    pub unit: String,
    /// The cell budget — a documented *frontend* default, not a library one.
    pub max_cells: u64,
    /// The tail policy.
    pub tail: TailChoice,
}

/// A compiled pattern plan: the explicit palette for the shared compiler and
/// the canonical artifact bytes, produced **before** pitch generation.
#[derive(Debug, Clone)]
pub struct PatternPlan {
    /// The one-bar templates, palette order, silent bars included.
    pub templates: Vec<RhythmTemplate>,
    /// The versioned `griff.pattern-expansion` artifact — canonical field
    /// order, byte-stable, ends with a newline.
    pub artifact_json: String,
}

/// A user-facing diagnostic at the CLI boundary: the spec §1.5 code, the
/// offending flag, and a message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatternDiagnostic {
    /// The stable `SWG____` code.
    pub code: &'static str,
    /// The flag the user must fix.
    pub flag: &'static str,
    /// What went wrong, in the flag's own vocabulary.
    pub message: String,
}

impl fmt::Display for PatternDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "error[{}] ({}): {}", self.code, self.flag, self.message)
    }
}

/// Compiles the pattern flags against the seed score's master timeline into
/// an explicit palette plus its expansion artifact.
///
/// Bar geometry comes from the score: its PPQN and the time signature of its
/// first master bar; a meter change anywhere in the score is `SWG0304`, and a
/// zero/unrepresentable bar is `SWG0305`. An expansion with no onsets is
/// `SWG0306` — a deliberate typed error, never an empty candidate set.
///
/// # Errors
/// Every failure is a [`PatternDiagnostic`] carrying its registry code and
/// the offending flag.
pub fn compile_pattern(
    args: &RhythmPatternArgs,
    score: &Score,
) -> Result<PatternPlan, PatternDiagnostic> {
    let _ = (args, score);
    unimplemented!("red phase: S16 Phase 2 — the CLI adapter (spec §2)")
}

/// Parses the transport kernel literal: `/` separates rows, only `X` and `.`
/// are cells, whitespace is a typed error, never normalized.
///
/// # Errors
/// `SWG0101` (ragged), `SWG0102` (foreign cell), `SWG0103` (whitespace),
/// `SWG0307` (empty literal or empty row).
pub fn parse_kernel_literal(literal: &str) -> Result<griff_pattern::Kernel, PatternDiagnostic> {
    let _ = literal;
    unimplemented!("red phase: S16 Phase 2 — the CLI adapter (spec §2)")
}

/// Parses a unit literal (`1/16`) into ticks at `ppqn`, checking whole-tick
/// representability (`SWG0301`).
///
/// # Errors
/// `SWG0301` with a message naming the PPQN when the unit is malformed, zero,
/// or not representable in whole ticks.
pub fn parse_unit(literal: &str, ppqn: u16) -> Result<Ticks, PatternDiagnostic> {
    let _ = (literal, ppqn);
    unimplemented!("red phase: S16 Phase 2 — the CLI adapter (spec §2)")
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::missing_assert_message,
    clippy::arithmetic_side_effects,
    clippy::str_to_string
)]
mod tests {
    use griff_core::event::{NoteMarks, Pitch, Tempo, Ticks, TimeSignature, Tuning, Velocity};
    use griff_core::generate::explicit_rhythm_diagnostics;
    use griff_core::score::{
        AtomEvent, AtomNote, EventGroup, EventGroupKind, LossReport, MasterBar, RepeatMarker,
        Score, Track, Voice,
    };
    use griff_core::slice::TickRange;

    use super::{
        compile_pattern, parse_kernel_literal, parse_unit, PatternDiagnostic, RhythmPatternArgs,
        TailChoice, TraversalChoice,
    };

    const BAR: u32 = 1920;

    fn seed_score(bar_count: usize) -> Score {
        seed_score_with_meters(&vec![(4, 4); bar_count])
    }

    /// A quarters-sounding score whose bar `i` carries `meters[i]`.
    fn seed_score_with_meters(meters: &[(u8, u8)]) -> Score {
        let mut start = 0_u32;
        let master_bars = meters
            .iter()
            .enumerate()
            .map(|(i, &(numerator, denominator))| {
                let len = 480 * 4 * u32::from(numerator) / (4 * u32::from(denominator));
                let mb = MasterBar {
                    index: i,
                    tick_range: TickRange::new(Ticks(start), Ticks(start + len))
                        .expect("ordered range"),
                    time_signature: TimeSignature {
                        numerator,
                        denominator,
                    },
                    tempo: Tempo::new(120.0).expect("valid tempo"),
                    repeat: RepeatMarker::default(),
                };
                start += len;
                mb
            })
            .collect();

        let groups = (0..4_u32)
            .map(|beat| EventGroup {
                kind: EventGroupKind::Single,
                atoms: vec![AtomEvent::Note(AtomNote {
                    absolute_start: Ticks(beat * 480),
                    duration: Ticks(480),
                    pitch: Pitch::new(40 + u8::try_from(beat).unwrap()).expect("valid pitch"),
                    velocity: Velocity::new(90).expect("valid velocity"),
                    marks: NoteMarks::empty(),
                    position: None,
                })],
                technique_spans: Vec::new(),
            })
            .collect();

        Score {
            ticks_per_quarter: 480,
            master_bars,
            tracks: vec![Track {
                name: Some("seed".to_string()),
                channel: 0,
                voices: vec![Voice {
                    id: 0,
                    event_groups: groups,
                }],
                tuning: Tuning::standard_e(),
            }],
            source_meta: None,
            loss: LossReport::new(),
        }
    }

    fn args(kernel: &str) -> RhythmPatternArgs {
        RhythmPatternArgs {
            kernel: kernel.to_string(),
            fractal_depth: 0,
            density_bps: None,
            rhythm_seed: None,
            traversal: TraversalChoice::RowMajor,
            unit: "1/16".to_string(),
            max_cells: 4096,
            tail: TailChoice::RestPad,
        }
    }

    fn code_and_flag(d: &PatternDiagnostic) -> (&'static str, &'static str) {
        (d.code, d.flag)
    }

    // ── kernel literal (transport contract, spec §2.1) ───────────────────────

    #[test]
    fn a_ragged_literal_is_swg0101_at_the_kernel_flag() {
        let d = parse_kernel_literal("X.X/XX").expect_err("ragged");
        assert_eq!(code_and_flag(&d), ("SWG0101", "--rhythm-kernel"));
    }

    #[test]
    fn a_foreign_cell_is_swg0102() {
        let d = parse_kernel_literal("X.X/XO.").expect_err("O is not a cell");
        assert_eq!(code_and_flag(&d), ("SWG0102", "--rhythm-kernel"));
    }

    #[test]
    fn whitespace_is_swg0103_never_normalized() {
        let d = parse_kernel_literal("X.X/ XX").expect_err("whitespace");
        assert_eq!(code_and_flag(&d), ("SWG0103", "--rhythm-kernel"));
    }

    #[test]
    fn an_empty_literal_is_swg0307() {
        assert_eq!(parse_kernel_literal("").expect_err("empty").code, "SWG0307");
        assert_eq!(
            parse_kernel_literal("X.X//XX.")
                .expect_err("empty row")
                .code,
            "SWG0307"
        );
    }

    // ── unit literal ─────────────────────────────────────────────────────────

    #[test]
    fn a_sixteenth_at_480_is_120_ticks() {
        assert_eq!(parse_unit("1/16", 480).expect("parses"), Ticks(120));
    }

    #[test]
    fn an_unrepresentable_unit_is_swg0301_naming_the_ppqn() {
        let d = parse_unit("1/7", 480).expect_err("1/7 at 480 is not whole ticks");
        assert_eq!(code_and_flag(&d), ("SWG0301", "--rhythm-unit"));
        assert!(d.message.contains("480"), "the message names the PPQN");
    }

    #[test]
    fn a_malformed_or_zero_unit_is_swg0301() {
        assert_eq!(
            parse_unit("banana", 480).expect_err("malformed").code,
            "SWG0301"
        );
        assert_eq!(parse_unit("0/16", 480).expect_err("zero").code, "SWG0301");
        assert_eq!(
            parse_unit("1/0", 480).expect_err("zero div").code,
            "SWG0301"
        );
    }

    // ── geometry from the master timeline ────────────────────────────────────

    #[test]
    fn a_meter_change_is_swg0304() {
        let score = seed_score_with_meters(&[(4, 4), (7, 8)]);
        let d = compile_pattern(&args("X.X/XX./.XX"), &score).expect_err("meter changes");
        assert_eq!(d.code, "SWG0304");
    }

    #[test]
    fn a_unit_that_does_not_divide_the_bar_is_swg0301() {
        // 7/8 bar = 1680 ticks; unit 1/4 = 480 does not divide it.
        let score = seed_score_with_meters(&[(7, 8)]);
        let mut a = args("X.X/XX./.XX");
        a.unit = "1/4".to_string();
        let d = compile_pattern(&a, &score).expect_err("480 does not divide 1680");
        assert_eq!(code_and_flag(&d), ("SWG0301", "--rhythm-unit"));
    }

    // ── the silent-expansion obligation (#115 review) ────────────────────────

    #[test]
    fn a_silent_expansion_is_swg0306_not_an_empty_candidate_set() {
        let d = compile_pattern(&args("..."), &seed_score(2)).expect_err("no onsets");
        assert_eq!(d.code, "SWG0306");
    }

    // ── budgets map to their registry codes ──────────────────────────────────

    #[test]
    fn a_cell_budget_breach_is_swg0201_at_the_max_cells_flag() {
        let mut a = args("X.X/XX./.XX");
        a.fractal_depth = 2; // 729 cells
        a.max_cells = 80;
        let d = compile_pattern(&a, &seed_score(2)).expect_err("729 > 80");
        assert_eq!(code_and_flag(&d), ("SWG0201", "--rhythm-max-cells"));
    }

    // ── the happy path: spec worked example, end to end ──────────────────────

    #[test]
    fn the_spec_kernel_compiles_into_one_padded_bar() {
        let plan = compile_pattern(&args("X.X/XX./.XX"), &seed_score(2)).expect("compiles");
        assert_eq!(plan.templates.len(), 1, "9 slots rest-pad into one 4/4 bar");
        let offsets: Vec<u32> = plan.templates[0].notes.iter().map(|n| n.offset.0).collect();
        assert_eq!(offsets, vec![0, 240, 360, 480, 840, 960]);
    }

    #[test]
    fn the_artifact_is_byte_stable_and_carries_the_geometry() {
        let a = compile_pattern(&args("X.X/XX./.XX"), &seed_score(2)).expect("compiles");
        let b = compile_pattern(&args("X.X/XX./.XX"), &seed_score(2)).expect("compiles");
        assert_eq!(a.artifact_json, b.artifact_json, "byte-stable");
        assert!(a.artifact_json.ends_with('\n'));

        let v: serde_json::Value = serde_json::from_str(&a.artifact_json).expect("valid JSON");
        assert_eq!(v["schema"], "griff.pattern-expansion");
        assert_eq!(v["version"], 1);
        assert_eq!(v["ppqn"], 480);
        assert_eq!(v["meter"], "4/4");
        assert_eq!(v["bar_duration_ticks"], 1920);
        assert_eq!(v["slots_per_bar"], 16);
        assert_eq!(v["activity"], "X.XXX..XX");
        assert_eq!(v["tail_policy"], "rest-pad");
    }

    #[test]
    fn artifact_fingerprints_equal_the_explicit_diagnostics() {
        let plan = compile_pattern(&args("X.X/XX./.XX"), &seed_score(2)).expect("compiles");
        let v: serde_json::Value = serde_json::from_str(&plan.artifact_json).expect("valid JSON");
        let from_artifact: Vec<String> = v["fingerprints"]
            .as_array()
            .expect("array")
            .iter()
            .map(|f| f.as_str().expect("hex string").to_string())
            .collect();
        let diag = explicit_rhythm_diagnostics(&plan.templates, Ticks(BAR));
        let expected: Vec<String> = diag
            .fingerprints
            .iter()
            .map(|fp| format!("{fp:016x}"))
            .collect();
        assert_eq!(from_artifact, expected, "no duplicated hashing anywhere");
    }

    #[test]
    fn the_density_flag_without_a_seed_is_swg0303_in_depth_defense() {
        // clap enforces the pairing at the surface; the module re-checks so
        // no other caller can smuggle an unseeded pruning through.
        let mut a = args("X.X/XX./.XX");
        a.density_bps = Some(8000);
        let d = compile_pattern(&a, &seed_score(2)).expect_err("no seed");
        assert_eq!(code_and_flag(&d), ("SWG0303", "--rhythm-seed"));
    }
}
