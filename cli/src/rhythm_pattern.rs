//! The `--rhythm-*` adapter (spec §2).
//!
//! Kernel literal → `griff-pattern` expansion → `griff-swang` lowering → an
//! explicit palette for the shared generation compiler, plus the versioned
//! expansion artifact. This is the **temporary transport syntax** of S16
//! Phase 2, not early Swang grammar. Errors leave as [`PatternDiagnostic`]s —
//! a stable `SWG____` code, the offending flag, and a message — per the
//! spec's §1.5 registry.

use std::fmt;

use griff_core::event::{Ticks, TimeSignature};
use griff_core::generate::{explicit_rhythm_diagnostics, RhythmTemplate};
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
    pub density_bps: Option<u32>,
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
    bars: usize,
) -> Result<PatternPlan, PatternDiagnostic> {
    // Defense in depth behind clap's `requires`: no caller smuggles an
    // unseeded pruning through (spec §1.13).
    if args.density_bps.is_some() && args.rhythm_seed.is_none() {
        return Err(PatternDiagnostic {
            code: "SWG0303",
            flag: "--rhythm-seed",
            message: "density decay was given without a rhythm seed; pruning must be \
                      explicitly seeded"
                .to_owned(),
        });
    }

    let kernel = parse_kernel_literal(&args.kernel)?;
    let geometry = resolve_geometry(score)?;
    let unit = parse_unit(&args.unit, geometry.ppqn)?;
    let bar_duration = geometry.bar_duration;

    // The transport type is deliberately wider than the domain type, so an
    // out-of-scale density meets its registry code instead of a bare clap
    // range error.
    let prune = match (args.density_bps, args.rhythm_seed) {
        (Some(bps), Some(seed)) => {
            let out_of_scale = || PatternDiagnostic {
                code: "SWG0308",
                flag: "--rhythm-density-bps",
                message: format!("density {bps} bps is outside 0..=10000"),
            };
            let narrow = u16::try_from(bps).map_err(|_| out_of_scale())?;
            Some(griff_pattern::PruneSpec {
                seed,
                density: griff_pattern::DensityBps::new(narrow).map_err(|_| out_of_scale())?,
            })
        }
        _ => None,
    };
    let budget = griff_pattern::ExpansionBudget {
        max_depth: args.fractal_depth,
        max_cells: args.max_cells,
    };
    let expansion = griff_pattern::fractalize(&kernel, args.fractal_depth, prune, budget)
        .map_err(map_pattern_error)?;

    let traversal = match args.traversal {
        TraversalChoice::RowMajor => griff_pattern::Traversal::RowMajor,
        TraversalChoice::Snake => griff_pattern::Traversal::Snake,
    };
    let sequence = griff_pattern::linearize(&expansion, traversal);
    if sequence.onsets().is_empty() {
        return Err(PatternDiagnostic {
            code: "SWG0306",
            flag: "--rhythm-kernel",
            message: "the expansion produced no onsets — nothing to generate (change the \
                      kernel, depth, density, or rhythm seed)"
                .to_owned(),
        });
    }

    let tail = match args.tail {
        TailChoice::Reject => griff_swang::TailPolicy::Reject,
        TailChoice::RestPad => griff_swang::TailPolicy::RestPad,
    };
    let templates =
        griff_swang::map_rhythm(&sequence, bar_duration, unit, tail).map_err(map_lower_error)?;

    check_bars_window(&templates, bars)?;

    let artifact_json = render_artifact(&ArtifactContext {
        args,
        geometry: &geometry,
        kernel: &kernel,
        sequence: &sequence,
        templates: &templates,
        unit,
    });

    Ok(PatternPlan {
        templates,
        artifact_json,
    })
}

/// The whole-expansion onset check is necessary but not sufficient: the
/// scheduler rotates the palette over `--bars`, so with fewer bars than
/// templates only a prefix is ever used. A silent prefix would send every
/// strategy into silence and the reranker into an empty set — *after* the
/// artifact was written. Stop it at `SWG0306` instead (#116 review).
fn check_bars_window(templates: &[RhythmTemplate], bars: usize) -> Result<(), PatternDiagnostic> {
    if bars == 0 {
        return Ok(()); // the generator's own BarCountZero owns this case
    }
    let used = bars.min(templates.len());
    let window_sounds = templates
        .iter()
        .take(used)
        .any(|template| !template.notes.is_empty());
    if window_sounds {
        Ok(())
    } else {
        Err(PatternDiagnostic {
            code: "SWG0306",
            flag: "--rhythm-kernel",
            message: format!(
                "the first {used} template(s) the --bars window rotates over are all \
                 silent — nothing to generate (raise --bars past the silent prefix, \
                 or change the kernel, depth, density, or rhythm seed)"
            ),
        })
    }
}

/// Bar geometry resolved from the seed score's master timeline (spec §1.11).
#[derive(Debug, Clone, Copy)]
struct BarGeometry {
    meter: TimeSignature,
    ppqn: u16,
    bar_duration: Ticks,
}

/// PPQN plus the first master bar's meter, constant across the score in
/// v0.1: a meter change is `SWG0304`, a zero/unrepresentable bar `SWG0305`.
#[allow(
    clippy::arithmetic_side_effects,
    // The multiply is u64 over u16/u8 inputs and the divisor is validated
    // non-zero before use.
    reason = "geometry math over validated non-zero inputs"
)]
fn resolve_geometry(score: &Score) -> Result<BarGeometry, PatternDiagnostic> {
    let Some(first) = score.master_bars.first() else {
        return Err(PatternDiagnostic {
            code: "SWG0305",
            flag: "INPUT",
            message: "the source has no master bars; there is no bar to map onto".to_owned(),
        });
    };
    let meter = first.time_signature;
    if let Some(changed) = score
        .master_bars
        .iter()
        .find(|mb| mb.time_signature != meter)
    {
        return Err(PatternDiagnostic {
            code: "SWG0304",
            flag: "INPUT",
            message: format!(
                "the meter changes at bar {} ({}/{} after {}/{}); v0.1 maps onto a \
                 constant meter",
                changed.index,
                changed.time_signature.numerator,
                changed.time_signature.denominator,
                meter.numerator,
                meter.denominator,
            ),
        });
    }
    let ppqn = score.ticks_per_quarter;
    let whole_bar = u64::from(ppqn) * 4 * u64::from(meter.numerator);
    let denominator = u64::from(meter.denominator);
    if denominator == 0 || !whole_bar.is_multiple_of(denominator) || whole_bar / denominator == 0 {
        return Err(PatternDiagnostic {
            code: "SWG0305",
            flag: "INPUT",
            message: format!(
                "a {}/{} bar at PPQN {ppqn} has no whole-tick duration",
                meter.numerator, meter.denominator,
            ),
        });
    }
    Ok(BarGeometry {
        meter,
        ppqn,
        bar_duration: Ticks(u32::try_from(whole_bar / denominator).unwrap_or(u32::MAX)),
    })
}

/// Maps a pattern-core error to its registry code at the offending flag.
fn map_pattern_error(e: griff_pattern::PatternError) -> PatternDiagnostic {
    match e {
        griff_pattern::PatternError::MaxCellsExceeded {
            needed, max_cells, ..
        } => PatternDiagnostic {
            code: "SWG0201",
            flag: "--rhythm-max-cells",
            message: format!("the expansion needs {needed} cells, over the budget's {max_cells}"),
        },
        griff_pattern::PatternError::MaxDepthExceeded {
            depth, max_depth, ..
        } => PatternDiagnostic {
            code: "SWG0202",
            flag: "--rhythm-fractal-depth",
            message: format!("depth {depth} exceeds the budget's max_depth {max_depth}"),
        },
        other => PatternDiagnostic {
            code: "SWG0101",
            flag: "--rhythm-kernel",
            message: other.to_string(),
        },
    }
}

/// Maps a lowering error to its registry code at the offending flag.
fn map_lower_error(e: griff_swang::LowerError) -> PatternDiagnostic {
    match e {
        griff_swang::LowerError::UnitDoesNotDivideBar { bar_duration, unit } => PatternDiagnostic {
            code: "SWG0301",
            flag: "--rhythm-unit",
            message: format!(
                "unit {} does not divide the {}-tick bar exactly",
                unit.0, bar_duration.0
            ),
        },
        griff_swang::LowerError::ZeroUnit => PatternDiagnostic {
            code: "SWG0301",
            flag: "--rhythm-unit",
            message: "the rhythm unit is zero ticks".to_owned(),
        },
        griff_swang::LowerError::IncompleteFinalBar {
            have_slots,
            slots_per_bar,
        } => PatternDiagnostic {
            code: "SWG0302",
            flag: "--rhythm-tail",
            message: format!(
                "the final bar holds {have_slots} of {slots_per_bar} slots; pass \
                 rest-pad to pad the tail with timed rests"
            ),
        },
    }
}

/// Everything the artifact serializer needs, in one place.
struct ArtifactContext<'a> {
    args: &'a RhythmPatternArgs,
    geometry: &'a BarGeometry,
    kernel: &'a griff_pattern::Kernel,
    sequence: &'a griff_pattern::ActivitySequence,
    templates: &'a [RhythmTemplate],
    unit: Ticks,
}

/// Serializes the versioned expansion artifact (spec §1.14).
///
/// Alphabetical key order (`serde_json`'s map order — a canonical order),
/// pretty-printed, newline-terminated, produced before pitch generation.
/// Fingerprints come from the public explicit diagnostics — no duplicated
/// hashing.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "slot arithmetic over already-validated non-zero unit and bar"
)]
fn render_artifact(ctx: &ArtifactContext<'_>) -> String {
    let ArtifactContext {
        args,
        geometry,
        kernel,
        sequence,
        templates,
        unit,
    } = *ctx;
    let bar_duration = geometry.bar_duration;
    let meter = format!(
        "{}/{}",
        geometry.meter.numerator, geometry.meter.denominator
    );
    let activity: String = sequence
        .cells()
        .iter()
        .map(|&active| if active { 'X' } else { '.' })
        .collect();
    let diagnostics = explicit_rhythm_diagnostics(templates, bar_duration);
    let fingerprints: Vec<String> = diagnostics
        .fingerprints
        .iter()
        .map(|fp| format!("{fp:016x}"))
        .collect();
    let bars: Vec<serde_json::Value> = templates
        .iter()
        .enumerate()
        .map(|(bar, template)| {
            serde_json::json!({
                "bar": bar,
                "notes": template
                    .notes
                    .iter()
                    .map(|n| serde_json::json!({"offset": n.offset.0, "duration": n.duration.0}))
                    .collect::<Vec<_>>(),
            })
        })
        .collect();

    let artifact = serde_json::json!({
        "schema": "griff.pattern-expansion",
        "version": 1,
        "kernel": {
            "width": kernel.width(),
            "height": kernel.height(),
            "cells": args.kernel,
        },
        "fractal_depth": args.fractal_depth,
        "density_bps": args.density_bps,
        "rhythm_seed": args.rhythm_seed,
        "traversal": match args.traversal {
            TraversalChoice::RowMajor => "row_major",
            TraversalChoice::Snake => "snake",
        },
        "unit": args.unit,
        "unit_ticks": unit.0,
        "ppqn": geometry.ppqn,
        "meter": meter,
        "bar_duration_ticks": bar_duration.0,
        "slots_per_bar": bar_duration.0 / unit.0,
        "tail_policy": match args.tail {
            TailChoice::Reject => "reject",
            TailChoice::RestPad => "rest-pad",
        },
        "max_cells": args.max_cells,
        "expanded_width": expansion_dim(kernel.width(), args.fractal_depth),
        "expanded_height": expansion_dim(kernel.height(), args.fractal_depth),
        "activity": activity,
        "templates": bars,
        "fingerprints": fingerprints,
    });
    let mut text = serde_json::to_string_pretty(&artifact).unwrap_or_else(|_| String::from("{}"));
    text.push('\n');
    text
}

/// `base ^ (depth + 1)` — depth 0 is the kernel itself.
fn expansion_dim(base: usize, depth: u8) -> u64 {
    u64::try_from(base)
        .unwrap_or(u64::MAX)
        .checked_pow(u32::from(depth).saturating_add(1))
        .unwrap_or(u64::MAX)
}

/// Parses the transport kernel literal: `/` separates rows, only `X` and `.`
/// are cells, whitespace is a typed error, never normalized.
///
/// # Errors
/// `SWG0101` (ragged), `SWG0102` (foreign cell), `SWG0103` (whitespace),
/// `SWG0307` (empty literal or empty row).
pub fn parse_kernel_literal(literal: &str) -> Result<griff_pattern::Kernel, PatternDiagnostic> {
    const FLAG: &str = "--rhythm-kernel";
    if literal.chars().any(char::is_whitespace) {
        return Err(PatternDiagnostic {
            code: "SWG0103",
            flag: FLAG,
            message: "whitespace inside the kernel literal; rows are separated by `/` alone"
                .to_owned(),
        });
    }
    let rows: Vec<&str> = literal.split('/').collect();
    if rows.iter().any(|row| row.is_empty()) {
        return Err(PatternDiagnostic {
            code: "SWG0307",
            flag: FLAG,
            message: "empty kernel literal or empty row".to_owned(),
        });
    }
    griff_pattern::Kernel::from_rows(&rows).map_err(|e| match e {
        griff_pattern::PatternError::RaggedKernel { row, expected, got } => PatternDiagnostic {
            code: "SWG0101",
            flag: FLAG,
            message: format!("ragged kernel: row {row} has {got} cells, expected {expected}"),
        },
        griff_pattern::PatternError::InvalidCell { row, col, cell } => PatternDiagnostic {
            code: "SWG0102",
            flag: FLAG,
            message: format!(
                "invalid kernel cell {cell:?} at row {row}, col {col}: only `X` and `.`"
            ),
        },
        griff_pattern::PatternError::EmptyKernel => PatternDiagnostic {
            code: "SWG0307",
            flag: FLAG,
            message: "empty kernel literal or empty row".to_owned(),
        },
        other => PatternDiagnostic {
            code: "SWG0101",
            flag: FLAG,
            message: other.to_string(),
        },
    })
}

/// Parses a unit literal (`1/16`) into ticks at `ppqn`, checking whole-tick
/// representability (`SWG0301`).
///
/// # Errors
/// `SWG0301` with a message naming the PPQN when the unit is malformed, zero,
/// or not representable in whole ticks.
#[allow(
    clippy::arithmetic_side_effects,
    // ppqn, numerator and denominator are all validated non-zero before the
    // u64 multiply/divide.
    reason = "unit math over validated non-zero inputs"
)]
pub fn parse_unit(literal: &str, ppqn: u16) -> Result<Ticks, PatternDiagnostic> {
    const FLAG: &str = "--rhythm-unit";
    let malformed = |message: String| PatternDiagnostic {
        code: "SWG0301",
        flag: FLAG,
        message,
    };
    let Some((numerator, denominator)) = literal.split_once('/') else {
        return Err(malformed(format!(
            "malformed unit {literal:?}: expected a note value like 1/16"
        )));
    };
    let numerator: u64 = numerator.parse().map_err(|_| {
        malformed(format!(
            "malformed unit {literal:?}: expected a note value like 1/16"
        ))
    })?;
    let denominator: u64 = denominator.parse().map_err(|_| {
        malformed(format!(
            "malformed unit {literal:?}: expected a note value like 1/16"
        ))
    })?;
    if numerator == 0 || denominator == 0 {
        return Err(malformed(format!("unit {literal} has a zero part")));
    }
    let whole = u64::from(ppqn) * 4 * numerator;
    if !whole.is_multiple_of(denominator) {
        return Err(malformed(format!(
            "unit {literal} is not representable in whole ticks at PPQN {ppqn}"
        )));
    }
    let ticks = whole / denominator;
    u32::try_from(ticks)
        .ok()
        .filter(|&t| t > 0)
        .map(Ticks)
        .ok_or_else(|| {
            malformed(format!(
                "unit {literal} leaves no whole tick at PPQN {ppqn}"
            ))
        })
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
        let d = compile_pattern(&args("X.X/XX./.XX"), &score, 2).expect_err("meter changes");
        assert_eq!(d.code, "SWG0304");
    }

    #[test]
    fn a_unit_that_does_not_divide_the_bar_is_swg0301() {
        // 7/8 bar = 1680 ticks; unit 1/4 = 480 does not divide it.
        let score = seed_score_with_meters(&[(7, 8)]);
        let mut a = args("X.X/XX./.XX");
        a.unit = "1/4".to_string();
        let d = compile_pattern(&a, &score, 2).expect_err("480 does not divide 1680");
        assert_eq!(code_and_flag(&d), ("SWG0301", "--rhythm-unit"));
    }

    // ── the silent-expansion obligation (#115 review) ────────────────────────

    #[test]
    fn a_silent_expansion_is_swg0306_not_an_empty_candidate_set() {
        let d = compile_pattern(&args("..."), &seed_score(2), 2).expect_err("no onsets");
        assert_eq!(d.code, "SWG0306");
    }

    #[test]
    fn the_bars_window_guards_swg0306_too() {
        // 17 cells at 16 slots per bar, rest-padded: template 0 is wholly
        // silent, template 1 carries the single onset. With --bars 1 only
        // template 0 is ever used — every strategy would emit silence, the
        // reranker would drop every candidate, and the user would meet the
        // old mysterious "no candidate survived scoring" *after* the
        // artifact was written. The window check stops that at SWG0306.
        let kernel = "................X";
        let d = compile_pattern(&args(kernel), &seed_score(2), 1).expect_err("bars 1 is silent");
        assert_eq!(d.code, "SWG0306");
        assert!(
            d.message.contains("--bars"),
            "the message must name the window"
        );

        // With two bars the sounding template enters the rotation.
        compile_pattern(&args(kernel), &seed_score(2), 2).expect("two bars reach the onset");
    }

    #[test]
    fn an_empty_score_is_swg0305() {
        let d = compile_pattern(&args("X.X/XX./.XX"), &seed_score_with_meters(&[]), 2)
            .expect_err("no master bars");
        assert_eq!(d.code, "SWG0305");
    }

    #[test]
    fn density_above_the_scale_is_swg0308_even_past_u16() {
        for bps in [10_001_u32, 70_000] {
            let mut a = args("X.X/XX./.XX");
            a.density_bps = Some(bps);
            a.rhythm_seed = Some(17);
            let d = compile_pattern(&a, &seed_score(2), 2).expect_err("out of scale");
            assert_eq!(code_and_flag(&d), ("SWG0308", "--rhythm-density-bps"));
        }
    }

    // ── budgets map to their registry codes ──────────────────────────────────

    #[test]
    fn a_cell_budget_breach_is_swg0201_at_the_max_cells_flag() {
        let mut a = args("X.X/XX./.XX");
        a.fractal_depth = 2; // 729 cells
        a.max_cells = 80;
        let d = compile_pattern(&a, &seed_score(2), 2).expect_err("729 > 80");
        assert_eq!(code_and_flag(&d), ("SWG0201", "--rhythm-max-cells"));
    }

    // ── the happy path: spec worked example, end to end ──────────────────────

    #[test]
    fn the_spec_kernel_compiles_into_one_padded_bar() {
        let plan = compile_pattern(&args("X.X/XX./.XX"), &seed_score(2), 2).expect("compiles");
        assert_eq!(plan.templates.len(), 1, "9 slots rest-pad into one 4/4 bar");
        let offsets: Vec<u32> = plan.templates[0].notes.iter().map(|n| n.offset.0).collect();
        assert_eq!(offsets, vec![0, 240, 360, 480, 840, 960]);
    }

    #[test]
    fn the_artifact_is_byte_stable_and_carries_the_geometry() {
        let a = compile_pattern(&args("X.X/XX./.XX"), &seed_score(2), 2).expect("compiles");
        let b = compile_pattern(&args("X.X/XX./.XX"), &seed_score(2), 2).expect("compiles");
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
        let plan = compile_pattern(&args("X.X/XX./.XX"), &seed_score(2), 2).expect("compiles");
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
        let d = compile_pattern(&a, &seed_score(2), 2).expect_err("no seed");
        assert_eq!(code_and_flag(&d), ("SWG0303", "--rhythm-seed"));
    }
}
