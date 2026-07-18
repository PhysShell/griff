use std::{
    collections::HashSet,
    fmt, fs,
    io::{self, Error as IoError, Write as IoWrite},
    ops::Range,
    path::{Path, PathBuf},
    process::ExitCode,
};

use clap::{Parser, Subcommand};
use griff_cli::generation_input::{load_corpus_material, CorpusMaterial, GenerationInputError};
use griff_cli::primary_voice_note_count;
use griff_cli::rhythm_pattern;
use griff_core::generation_input::{ranked_candidates, GenerationAsk, RankedSet};
use griff_core::{
    boundary,
    classify::{self, BarClass},
    complement,
    corpus::{
        Acquisition, BoundaryEntry, ChunkId, ChunkMeta, CorpusManifest, EnsembleGroup, EnsembleRef,
        PairRelation, QualityFlag, ReviewerDecision, RightsInfo, RightsStatus, SourceFormat,
        SourceRef, StyleCohort, SwancoreTag, SCHEMA_VERSION,
    },
    event::{NoteMarks, NotePosition, TechniqueSource, Ticks},
    generate, gesture, harmony,
    import::{self, ImportError},
    ingest,
    midi::{self, MidiError},
    novelty, rerank,
    score::{AtomEvent, Score, Track, Voice},
    scoring,
    slice::{self, TickRange},
    split, structure, syncopation, technique, unfold,
};
use griff_pattern::NodePath;
use griff_swang::{eval, syntax};

/// griff — guitar riff engine.
#[derive(Debug, Parser)]
#[command(name = "griff", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Parse a MIDI or Guitar Pro file and print a one-line summary per track.
    Import {
        /// Path to the MIDI (`.mid`) or Guitar Pro (`.gp3`/`.gp4`/`.gp5`/`.gpx`) file.
        #[arg(value_name = "FILE")]
        path: PathBuf,
    },

    /// Print a detailed bar-by-bar inspection of a MIDI or Guitar Pro file.
    Inspect {
        /// Path to the MIDI (`.mid`) or Guitar Pro (`.gp3`/`.gp4`/`.gp5`/`.gpx`) file.
        #[arg(value_name = "FILE")]
        path: PathBuf,
        /// Expand repeats (`|: … :|×N`) into the as-played bar sequence.
        #[arg(long)]
        unfold: bool,
    },

    /// Import a MIDI or Guitar Pro file and write it back out as MIDI.
    Export {
        /// Input MIDI or Guitar Pro file.
        #[arg(value_name = "INPUT")]
        input: PathBuf,
        /// Output `.mid` file.
        #[arg(value_name = "OUTPUT")]
        output: PathBuf,
    },

    /// Classify each bar of a MIDI or Guitar Pro file as Riff, Solo, Breakdown, Clean, or Unknown.
    Classify {
        /// Path to the MIDI (`.mid`) or Guitar Pro (`.gp3`/`.gp4`/`.gp5`/`.gpx`) file.
        #[arg(value_name = "FILE")]
        path: PathBuf,
    },

    /// Measure each track's structural character (S14): pattern period,
    /// repeatability, loopability, and complexity — a "song map".
    Structure {
        /// Path to the MIDI (`.mid`) or Guitar Pro (`.gp3`/`.gp4`/`.gp5`/`.gpx`) file.
        #[arg(value_name = "FILE")]
        path: PathBuf,
    },

    /// Detect phrase boundaries per track (S4): where one musical phrase ends
    /// and the next begins, with the heuristic signals that fired.
    Phrases {
        /// Path to the MIDI (`.mid`) or Guitar Pro (`.gp3`/`.gp4`/`.gp5`/`.gpx`) file.
        #[arg(value_name = "FILE")]
        path: PathBuf,
    },

    /// Generate a fresh riff (S6) seeded from a tab's scale, meter, and pitch
    /// range: every strategy contributes seed variants to a candidate set,
    /// the set is reranked on the closure + novelty axes (ADR-0017), and the
    /// winner is written to a MIDI file. With `--corpus`, rhythm templates,
    /// novelty references, and the gesture ask come from curated chunks
    /// instead of the input's first bar.
    Generate {
        /// Source MIDI or Guitar Pro file whose material seeds the generator.
        #[arg(value_name = "INPUT")]
        input: PathBuf,
        /// Output `.mid` file for the generated riff.
        #[arg(value_name = "OUTPUT")]
        output: PathBuf,
        /// Deterministic seed — the same seed always yields the same riff.
        #[arg(long, default_value_t = 0)]
        seed: u64,
        /// Number of bars to generate.
        #[arg(long, default_value_t = 8)]
        bars: usize,
        /// Directory of curated `*.chunk.json` records sitting next to their
        /// source tabs: supplies rhythm templates, novelty references, and
        /// the burst/rest gesture ask.
        #[arg(long, value_name = "DIR")]
        corpus: Option<PathBuf>,
        /// Seed variants *per strategy* in the candidate set. The reranked set
        /// holds this many × 5 strategies candidates (fewer only when
        /// rhythm-copy is skipped for want of a template) — e.g. `10` ranks 50.
        #[arg(long, default_value_t = 2)]
        candidates: usize,
        /// Skip burst/rest gesture carving even when the corpus provides
        /// gesture statistics (wall-to-wall writing).
        #[arg(long)]
        no_gesture: bool,
        /// ASCII kernel literal (S16 transport syntax): rows of `X`/`.`
        /// separated by `/`, e.g. `X.X/XX./.XX`. Compiles into an explicit
        /// rhythm palette that overrides corpus and source rhythms.
        #[arg(
            long,
            value_name = "KERNEL",
            requires = "rhythm_fractal_depth",
            requires = "rhythm_traversal",
            requires = "rhythm_unit"
        )]
        rhythm_kernel: Option<String>,
        /// Exact fractal expansion depth (depth 0 is the kernel itself).
        #[arg(long, value_name = "DEPTH", requires = "rhythm_kernel")]
        rhythm_fractal_depth: Option<u8>,
        /// Density decay in basis points (0..=10000); requires --rhythm-seed.
        #[arg(
            long,
            value_name = "BPS",
            requires = "rhythm_kernel",
            requires = "rhythm_seed"
        )]
        rhythm_density_bps: Option<u32>,
        /// Structural pruning seed — independent of --seed by law.
        #[arg(long, value_name = "SEED", requires = "rhythm_kernel")]
        rhythm_seed: Option<u64>,
        /// How the expansion reads into a line: row-major or snake.
        #[arg(long, value_name = "ORDER", requires = "rhythm_kernel")]
        rhythm_traversal: Option<rhythm_pattern::CliTraversal>,
        /// Time unit per pattern slot, e.g. 1/16.
        #[arg(long, value_name = "NOTE", requires = "rhythm_kernel")]
        rhythm_unit: Option<String>,
        /// Cell budget for the expansion (CLI default: 4096).
        #[arg(long, value_name = "CELLS", requires = "rhythm_kernel")]
        rhythm_max_cells: Option<u64>,
        /// Incomplete-final-bar policy: reject (default) or rest-pad.
        #[arg(long, value_name = "POLICY", requires = "rhythm_kernel")]
        rhythm_tail: Option<rhythm_pattern::CliTail>,
        /// Write the versioned expansion artifact (JSON) to this path.
        #[arg(long, value_name = "PATH", requires = "rhythm_kernel")]
        emit_rhythm_expansion: Option<PathBuf>,
    },

    /// Arrange a complementary part (S13) for a tab's primary track — a second
    /// guitar/bass derived from it — and write both parts to a MIDI file.
    Complement {
        /// Source MIDI or Guitar Pro file whose primary track is part A.
        #[arg(value_name = "INPUT")]
        input: PathBuf,
        /// Output `.mid` file for A plus the generated part B.
        #[arg(value_name = "OUTPUT")]
        output: PathBuf,
        /// Relation mode: `rhythm_lock`, `register_contrast`, `call_response`,
        /// `support_layer`, `octave_double`, or `counter_melody`.
        #[arg(long, default_value = "rhythm_lock")]
        mode: String,
        /// Deterministic seed — the same seed always yields the same part.
        #[arg(long, default_value_t = 0)]
        seed: u64,
        /// Semitone shift of B's register relative to A (e.g. -12 = octave
        /// down). Defaults per mode: `octave_double` and `register_contrast`
        /// reject a zero shift, so they default to -12; others to 0.
        #[arg(long, allow_hyphen_values = true)]
        offset: Option<i8>,
    },

    /// Interactively curate a MIDI or Guitar Pro file into a corpus `ChunkMeta` JSON record.
    Curate {
        /// Path to the MIDI or Guitar Pro file to curate.
        #[arg(value_name = "FILE")]
        path: PathBuf,
        /// Output path for the `ChunkMeta` JSON (default: `<file>.chunk.json`).
        /// In ensemble mode this is the output *stem*: parts land at
        /// `<stem>.p<N>.chunk.json` and the group at `<stem>.group.json`.
        #[arg(short, long, value_name = "OUTPUT")]
        output: Option<PathBuf>,
        /// Curate every note-bearing track as one linked ensemble group
        /// (corpus schema v4): one chunk per part plus a group record with
        /// measured pairwise relation axes.
        #[arg(long)]
        ensemble: bool,
    },

    /// Split a MIDI or Guitar Pro file into one corpus `ChunkMeta` per detected
    /// phrase: each chunk is a standalone bar-range slice carrying its own
    /// measurements and `bar_range` provenance. Chunks land at
    /// `<stem>.p<N>.chunk.json`.
    Split {
        /// Path to the MIDI or Guitar Pro file to split.
        #[arg(value_name = "FILE")]
        path: PathBuf,
        /// Output *stem* (default: `<file>`): phrase chunks land at
        /// `<stem>.p<N>.chunk.json`.
        #[arg(short, long, value_name = "OUTPUT")]
        output: Option<PathBuf>,
    },

    /// Bulk-ingest a directory of MIDI / Guitar Pro files into corpus chunks:
    /// each file's guitar (and optional bass) tracks are phrase-split, linked
    /// as one per-file ensemble group, and stamped with community-tab rights.
    /// Chunks are uncurated candidates — tags and reviewer are filled later.
    Ingest {
        /// Directory of source tab files to ingest.
        #[arg(value_name = "DIR")]
        dir: PathBuf,
        /// Output directory for the chunk / group records (default: `corpus`).
        #[arg(short, long, value_name = "OUT")]
        output: Option<PathBuf>,
        /// Also ingest bass tracks (kept as separate parts, never mixed with
        /// guitar).
        #[arg(long)]
        with_bass: bool,
    },

    /// Build a corpus manifest from a directory of curated `*.chunk.json` /
    /// `*.group.json` records and print a coverage summary (count toward the
    /// S7 ~100-phrase gate, cohort mix, rights, and review status).
    Manifest {
        /// Directory holding the curated chunk and group JSON records.
        #[arg(value_name = "DIR")]
        dir: PathBuf,
        /// Output path for the manifest JSON (default: `<dir>/manifest.json`).
        #[arg(short, long, value_name = "OUTPUT")]
        output: Option<PathBuf>,
    },

    /// Swang script tools (S16 Phase 3): `check` diagnoses, `fmt` prints
    /// the canonical text, `expand` emits the expansion artifact, and
    /// `build` runs a program end to end into its own `export`.
    Swang {
        #[command(subcommand)]
        command: SwangCommand,
    },
}

#[derive(Debug, Subcommand)]
enum SwangCommand {
    /// Parse a Swang script and report its diagnostics: a stable `SWG____`
    /// code located at `<path>:<line>:<col>`. Says nothing when the program
    /// is well-formed.
    Check {
        /// Path to the `.swg` script.
        #[arg(value_name = "INPUT")]
        input: PathBuf,
    },
    /// Parse a Swang script and print its one canonical text to stdout.
    /// Emits nothing for a program that does not parse.
    Fmt {
        /// Path to the `.swg` script.
        #[arg(value_name = "INPUT")]
        input: PathBuf,
    },
    /// Run the program's pattern pipeline up to `map_rhythm` and print the
    /// canonical `griff.pattern-expansion` JSON to stdout — byte-identical
    /// to the Phase-2 `--emit-rhythm-expansion` artifact for the equivalent
    /// command. No pitch generation happens; the program's `export` stays
    /// untouched.
    Expand {
        /// Path to the `.swg` script.
        #[arg(value_name = "INPUT")]
        input: PathBuf,
    },
    /// Run the program end to end — expansion, generation, strategy
    /// selection (spec §3.5 law 5: `auto` matches `griff generate`; a named
    /// strategy selects from the unchanged ranked set), and the program's
    /// own `export`. No output flag exists: the program is the output's
    /// single owner.
    Build {
        /// Path to the `.swg` script.
        #[arg(value_name = "INPUT")]
        input: PathBuf,
    },
}

fn run() -> Result<(), CliError> {
    let cli = Cli::parse();
    match cli.command {
        Command::Import { path } => cmd_import(&path),
        Command::Inspect { path, unfold } => cmd_inspect(&path, unfold),
        Command::Export { input, output } => cmd_export(&input, &output),
        Command::Classify { path } => cmd_classify(&path),
        Command::Structure { path } => cmd_structure(&path),
        Command::Phrases { path } => cmd_phrases(&path),
        Command::Generate {
            input,
            output,
            seed,
            bars,
            corpus,
            candidates,
            no_gesture,
            rhythm_kernel,
            rhythm_fractal_depth,
            rhythm_density_bps,
            rhythm_seed,
            rhythm_traversal,
            rhythm_unit,
            rhythm_max_cells,
            rhythm_tail,
            emit_rhythm_expansion,
        } => {
            // clap's `requires` guarantees depth/traversal/unit accompany the
            // kernel; the unwraps below never fire without it.
            let rhythm = rhythm_kernel.map(|kernel| rhythm_pattern::RhythmPatternArgs {
                kernel,
                fractal_depth: rhythm_fractal_depth.unwrap_or(0),
                density_bps: rhythm_density_bps,
                rhythm_seed,
                traversal: rhythm_traversal
                    .map_or(rhythm_pattern::TraversalChoice::RowMajor, Into::into),
                unit: rhythm_unit.unwrap_or_else(|| "1/16".to_owned()),
                max_cells: rhythm_max_cells.unwrap_or(4096),
                tail: rhythm_tail.map_or(rhythm_pattern::TailChoice::Reject, Into::into),
            });
            cmd_generate(
                &input,
                &output,
                &GenerateOpts {
                    seed,
                    bars,
                    corpus: corpus.as_deref(),
                    candidates,
                    no_gesture,
                    rhythm: rhythm.as_ref(),
                    emit_rhythm_expansion: emit_rhythm_expansion.as_deref(),
                },
            )
        }
        Command::Complement {
            input,
            output,
            mode,
            seed,
            offset,
        } => cmd_complement(&input, &output, &mode, seed, offset),
        Command::Curate {
            path,
            output,
            ensemble,
        } => cmd_curate(&path, output.as_deref(), ensemble),
        Command::Split { path, output } => cmd_split(&path, output.as_deref()),
        Command::Ingest {
            dir,
            output,
            with_bass,
        } => cmd_ingest(&dir, output.as_deref(), with_bass),
        Command::Manifest { dir, output } => cmd_manifest(&dir, output.as_deref()),
        Command::Swang { command } => match command {
            SwangCommand::Check { input } => cmd_swang_check(&input),
            SwangCommand::Fmt { input } => cmd_swang_fmt(&input),
            SwangCommand::Expand { input } => cmd_swang_expand(&input),
            SwangCommand::Build { input } => cmd_swang_build(&input),
        },
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

// ── swang (S16 Phase 3: the grammar's CLI edge) ─────────────────────────────

/// `griff swang check`: parse only, say nothing on success.
fn cmd_swang_check(path: &Path) -> Result<(), CliError> {
    let source = fs::read_to_string(path)?;
    eval::compile_program(&source)
        .map(|_| ())
        .map_err(|diagnostics| swang_error(path, &source, &diagnostics))
}

/// `griff swang fmt`: print the one canonical text, or fail with `check`'s
/// own diagnostic — never emit text for a program that does not parse.
fn cmd_swang_fmt(path: &Path) -> Result<(), CliError> {
    let source = fs::read_to_string(path)?;
    let compiled = eval::compile_program(&source)
        .map_err(|diagnostics| swang_error(path, &source, &diagnostics))?;
    print!("{}", syntax::format(compiled.program()));
    Ok(())
}

/// `griff swang expand`: the pattern pipeline up to `map_rhythm`, printing
/// the canonical expansion artifact to stdout (spec §3.5's CLI contract).
/// The shared evaluator drives the same compiler the transport does, so law
/// 1's byte parity holds by construction; only the rendering differs.
fn cmd_swang_expand(path: &Path) -> Result<(), CliError> {
    let source = fs::read_to_string(path)?;
    let compiled = eval::compile_program(&source)
        .map_err(|diagnostics| swang_error(path, &source, &diagnostics))?;
    let score_bytes = fs::read(compiled.source_path())?;
    let score = import::import_score_auto(&score_bytes)?;
    let plan = eval::expand_program(&compiled, &score)
        .map_err(|diagnostics| render_eval_diagnostics(&diagnostics, path, &source))?;
    print!("{}", plan.artifact_json);
    Ok(())
}

/// `griff swang build`: the program end to end through the shared evaluator.
/// Under `strategy auto` the export's bytes match `griff generate` for the
/// equivalent command (law 5's first half); a named strategy selects that
/// strategy's first ranked candidate from the unchanged set (the second
/// half). The output path is the program's own `export`, and only it — the
/// evaluator returns the result in memory and this shell writes it.
fn cmd_swang_build(path: &Path) -> Result<(), CliError> {
    let source = fs::read_to_string(path)?;
    let compiled = eval::compile_program(&source)
        .map_err(|diagnostics| swang_error(path, &source, &diagnostics))?;

    let score_bytes = fs::read(compiled.source_path())?;
    let score = import::import_score_auto(&score_bytes)?;
    let corpus = compiled
        .corpus_path()
        .map(|corpus| load_corpus_material(Path::new(corpus)))
        .transpose()?;
    if let Some(m) = &corpus {
        print_corpus_summary(m, false);
    }

    let inputs = eval::ResolvedProgramInputs {
        source_score: score,
        corpus,
    };
    let result = eval::evaluate_program(&compiled, &inputs)
        .map_err(|diagnostics| render_eval_diagnostics(&diagnostics, path, &source))?;

    let set = &result.ranked;
    print_explicit_rhythm_diagnostics(&set.source_rhythms, &set.base.constraints);
    print_ranking(&set.ranked, &set.policy);

    let out_bytes = midi::export_score(result.selected_score())?;
    fs::write(&result.export.path, &out_bytes)?;
    println!(
        "built {bars} bars ({strategy:?}, seed {seed}) from a {tones}-tone scale \
         ({n} bytes) -> {out}",
        bars = compiled.program().pattern.generate.bars,
        strategy = result.selected_strategy(),
        seed = compiled.program().pattern.generate.seed,
        tones = set.base.pitch_material.intervals.len(),
        n = out_bytes.len(),
        out = result.export.path,
    );
    Ok(())
}

/// Renders the evaluator's first diagnostic in program vocabulary at §1.5's
/// layered locations: a source span becomes `<path>:<line>:<col>`, a
/// structural `NodePath` becomes `<path>: node <words>`.
fn render_eval_diagnostics(
    diagnostics: &[eval::EvalDiagnostic],
    path: &Path,
    source: &str,
) -> CliError {
    let Some(d) = diagnostics.first() else {
        return CliError::Swang("error[SWG0401]: the evaluator reported no diagnostic".to_owned());
    };
    match &d.location {
        eval::DiagLocation::Span(span) => {
            let (line, col) = line_col(source, span.start);
            CliError::Swang(format!(
                "error[{}] ({}:{line}:{col}): {}",
                d.code,
                path.display(),
                d.message
            ))
        }
        eval::DiagLocation::Node(node) => CliError::Swang(format!(
            "error[{}] ({}: node {}): {}",
            d.code,
            path.display(),
            node_words(node),
            d.message
        )),
    }
}

/// A `NodePath` in words: `root` for the empty path, dotted child indices
/// otherwise.
fn node_words(node: &NodePath) -> String {
    if node.as_slice().is_empty() {
        "root".to_owned()
    } else {
        node.as_slice()
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(".")
    }
}

/// Renders the parser's first diagnostic against the script: the stable
/// registry code at `<path>:<line>:<col>` (spec §1.5 — a source span is the
/// location class of the grammar boundary; the flag class retired with the
/// transport).
fn swang_error(path: &Path, source: &str, diagnostics: &[syntax::Diagnostic]) -> CliError {
    let Some(diagnostic) = diagnostics.first() else {
        return CliError::Swang("error[SWG0401]: the parser reported no diagnostic".to_owned());
    };
    let (line, col) = line_col(source, diagnostic.span.start);
    CliError::Swang(format!(
        "error[{}] ({}:{line}:{col}): {}",
        diagnostic.code,
        path.display(),
        diagnostic.message
    ))
}

/// 1-based line and column of a byte offset; columns count Unicode scalar
/// values from the line start. Rendering-edge arithmetic only — spans stay
/// byte offsets everywhere else.
fn line_col(source: &str, offset: u32) -> (u32, u32) {
    let target = usize::try_from(offset).unwrap_or(usize::MAX);
    let mut line = 1_u32;
    let mut col = 1_u32;
    for (at, c) in source.char_indices() {
        if at >= target {
            break;
        }
        if c == '\n' {
            line = line.saturating_add(1);
            col = 1;
        } else {
            col = col.saturating_add(1);
        }
    }
    (line, col)
}

// ── commands ──────────────────────────────────────────────────────────────────

/// Returns the first (primary) voice of a track, if any.
fn primary_voice(track: &Track) -> Option<&Voice> {
    track.voices.first()
}

/// Counts note atoms in a voice whose onset falls in `range`.
fn note_count_in_range(voice: &Voice, range: TickRange) -> usize {
    voice
        .event_groups
        .iter()
        .flat_map(|g| &g.atoms)
        .filter(|a| matches!(a, AtomEvent::Note(_)))
        .filter(|a| {
            let onset = a.absolute_start().0;
            onset >= range.start.0 && onset < range.end.0
        })
        .count()
}

/// Total note atoms across all voices of a track.
fn track_note_count(track: &Track) -> usize {
    track
        .voices
        .iter()
        .flat_map(|v| &v.event_groups)
        .flat_map(|g| &g.atoms)
        .filter(|a| matches!(a, AtomEvent::Note(_)))
        .count()
}

fn cmd_import(path: &Path) -> Result<(), CliError> {
    let data = fs::read(path)?;
    let score = import::import_score_auto(&data)?;

    println!("PPQN: {}", score.ticks_per_quarter);
    println!("Bars: {}", score.master_bars.len());
    println!("Tracks: {}", score.tracks.len());
    for (idx, track) in score.tracks.iter().enumerate() {
        let name = track.name.as_deref().unwrap_or("<unnamed>");
        println!(
            "  [{idx}] ch={ch:02}  notes={notes:5}  \"{name}\"",
            ch = track.channel,
            notes = track_note_count(track),
        );
    }
    Ok(())
}

fn cmd_inspect(path: &Path, unfold: bool) -> Result<(), CliError> {
    let data = fs::read(path)?;
    let score = import::import_score_auto(&data)?;

    println!("PPQN: {}", score.ticks_per_quarter);
    // The bar order is the same for every track: the written bars by default,
    // the as-played sequence (repeats expanded) with `--unfold`. Compute once.
    let order: Vec<usize> = if unfold {
        unfold::played_bar_order(&score)
    } else {
        (0..score.master_bars.len()).collect()
    };
    for (ti, track) in score.tracks.iter().enumerate() {
        let name = track.name.as_deref().unwrap_or("<unnamed>");
        let tuning: Vec<String> = track
            .tuning
            .open_strings()
            .iter()
            .map(|p| p.0.to_string())
            .collect();
        println!(
            "Track {ti} ch={ch} \"{name}\"  tuning={}",
            tuning.join(","),
            ch = track.channel,
        );
        let voice = primary_voice(track);
        for (play_pos, &src) in order.iter().enumerate() {
            let Some(mb) = score.master_bars.get(src) else {
                continue;
            };
            let notes = voice.map_or(0, |v| note_count_in_range(v, mb.tick_range));
            if unfold {
                println!(
                    "  Play {play_pos:4}  src Bar {bi:4}  {num}/{den}  {bpm:.1} BPM  {notes} notes",
                    bi = mb.index,
                    num = mb.time_signature.numerator,
                    den = mb.time_signature.denominator,
                    bpm = mb.tempo.0,
                );
            } else {
                println!(
                    "  Bar {bi:4}  {num}/{den}  {bpm:.1} BPM  {notes} notes",
                    bi = mb.index,
                    num = mb.time_signature.numerator,
                    den = mb.time_signature.denominator,
                    bpm = mb.tempo.0,
                );
            }
        }
        if let Some(v) = voice {
            for group in &v.event_groups {
                for atom in &group.atoms {
                    if let AtomEvent::Note(n) = atom {
                        println!(
                            "    t={t:<6} p={p:<3} pos={pos:<16} marks={marks}",
                            t = n.absolute_start.0,
                            p = n.pitch.0,
                            pos = fmt_position(n.position),
                            marks = fmt_marks(n.marks),
                        );
                    }
                }
            }
        }
    }
    Ok(())
}

/// Renders an optional fretboard position as `string/fret source`, or `-`.
fn fmt_position(pos: Option<NotePosition>) -> String {
    pos.map_or_else(
        || "-".to_owned(),
        |np| {
            let source = match np.evidence.source {
                TechniqueSource::Explicit => "explicit",
                TechniqueSource::InferredFromMidi => "inferred",
            };
            format!("{}/{} {source}", np.position.string, np.position.fret)
        },
    )
}

/// Renders a note's marks as a comma list, or `-` when empty.
fn fmt_marks(marks: NoteMarks) -> String {
    let names: Vec<String> = marks.iter().map(|m| format!("{m:?}")).collect();
    if names.is_empty() {
        "-".to_owned()
    } else {
        names.join(",")
    }
}

fn cmd_export(input: &Path, output: &Path) -> Result<(), CliError> {
    let data = fs::read(input)?;
    let score = import::import_score_auto(&data)?;
    let out_bytes = midi::export_score(&score)?;
    fs::write(output, &out_bytes)?;
    println!(
        "exported {} tracks ({} bytes) -> {}",
        score.tracks.len(),
        out_bytes.len(),
        output.display(),
    );
    Ok(())
}

fn cmd_classify(path: &Path) -> Result<(), CliError> {
    let data = fs::read(path)?;
    let score = import::import_score_auto(&data)?;

    println!("PPQN: {}", score.ticks_per_quarter);
    for (ti, track) in score.tracks.iter().enumerate() {
        let name = track.name.as_deref().unwrap_or("<unnamed>");
        println!(
            "Track {ti} ch={ch:02} \"{name}\" — {} bars",
            score.master_bars.len(),
            ch = track.channel,
        );
        let voice = primary_voice(track);
        for mb in &score.master_bars {
            let feat = voice.map_or_else(
                || classify::BarFeatures {
                    note_count: 0,
                    avg_velocity: 0,
                    pitch_span: 0,
                },
                |v| classify::bar_features_in_range(v, mb.tick_range),
            );
            let class: BarClass = classify::classify_bar(feat);
            println!(
                "  Bar {bi:4}  {num}/{den}  {bpm:.1} BPM  \
                 notes={notes:3}  class={class:<10}  vel={vel:3}  span={span:2}st",
                bi = mb.index,
                num = mb.time_signature.numerator,
                den = mb.time_signature.denominator,
                bpm = mb.tempo.0,
                notes = feat.note_count,
                vel = feat.avg_velocity,
                span = feat.pitch_span,
            );
        }
    }
    Ok(())
}

/// Prints each track's S14 structure metrics — a "song map" (ADR-0015):
/// pattern period, repeatability, loopability, complexity, and a coarse
/// looped / through-composed / mixed character.
fn cmd_structure(path: &Path) -> Result<(), CliError> {
    let data = fs::read(path)?;
    let score = import::import_score_auto(&data)?;

    println!("PPQN: {}", score.ticks_per_quarter);
    for (ti, track) in score.tracks.iter().enumerate() {
        let name = track.name.as_deref().unwrap_or("<unnamed>");
        match structure::measure_structure(&score, ti) {
            Ok(metrics) => print_structure(ti, name, &metrics),
            Err(err) => println!("Track {ti} \"{name}\" — cannot measure: {err:?}"),
        }
    }
    Ok(())
}

/// Prints one track's structure metrics as a readable block (S14, ADR-0015).
fn print_structure(ti: usize, name: &str, m: &structure::StructureMetrics) {
    println!("Track {ti} \"{name}\"  {bars} bars", bars = m.bar_count);
    match (
        m.detected_pattern_period_bars,
        m.detected_pattern_period_ticks,
    ) {
        (Some(bars), Some(ticks)) => println!("  pattern period : {bars} bars ({ticks}t)"),
        _ => println!("  pattern period : none (through-composed)"),
    }
    match m.detected_subbar_period_ticks {
        Some(ticks) => println!("  sub-bar period : {ticks}t"),
        None => println!("  sub-bar period : none"),
    }
    println!(
        "  repeatability  : {rep:.2}   variation {var:.2}",
        rep = m.repeatability_score,
        var = m.variation_score,
    );
    println!("  loopability    : {:.2}", m.loopability_score);
    println!("  complexity     : {:.2}", m.structural_complexity);
    println!("  character      : {}", structure_character(m));
}

/// A coarse, human-readable reading of the metrics: a span with a detected
/// period and strong repeatability is looped; one with mostly distinct bars and
/// no period is through-composed; anything between is mixed.
fn structure_character(m: &structure::StructureMetrics) -> &'static str {
    if m.detected_pattern_period_bars.is_some() && m.repeatability_score >= 0.5 {
        "looped"
    } else if m.detected_pattern_period_bars.is_none() && m.structural_complexity >= 0.7 {
        "through-composed"
    } else {
        "mixed"
    }
}

/// Detects phrase boundaries per track (S4) and prints each with its bar, tick,
/// aggregate score, and the heuristic signals that fired.
fn cmd_phrases(path: &Path) -> Result<(), CliError> {
    let data = fs::read(path)?;
    let score = import::import_score_auto(&data)?;
    // The default config's tick gaps are tuned for GP's 960 PPQN; scale them to
    // the file's own resolution so a phrase is the same musical distance
    // whatever the source encoding (2 quarter notes apart, snapped to 1/16).
    let ppqn = u32::from(score.ticks_per_quarter);
    let config = boundary::BoundaryConfig {
        min_gap: Ticks(ppqn.saturating_mul(2)),
        quantize_ticks: Ticks(ppqn.checked_div(4).unwrap_or(1).max(1)),
        ..boundary::BoundaryConfig::default()
    };

    println!("PPQN: {}", score.ticks_per_quarter);
    for (ti, track) in score.tracks.iter().enumerate() {
        let name = track.name.as_deref().unwrap_or("<unnamed>");
        let boundaries = boundary::detect_phrase_boundaries(&score, ti, &config);
        println!(
            "Track {ti} \"{name}\"  {bars} bars — {n} phrase boundaries",
            bars = score.master_bars.len(),
            n = boundaries.len(),
        );
        for b in &boundaries {
            println!(
                "  bar {bar:4}  t={tick:<7}  score={sc:.2}  [{reasons}]",
                bar = bar_at_tick(&score, b.start_tick.0),
                tick = b.start_tick.0,
                sc = b.score,
                reasons = phrase_reasons(b.reason),
            );
        }
    }
    Ok(())
}

/// The index of the master bar containing `tick`, or the last bar when the tick
/// falls at or past the end of the timeline.
fn bar_at_tick(score: &Score, tick: u32) -> usize {
    score
        .master_bars
        .iter()
        .find(|mb| tick >= mb.tick_range.start.0 && tick < mb.tick_range.end.0)
        .map_or_else(|| score.master_bars.len().saturating_sub(1), |mb| mb.index)
}

/// Renders the heuristic signals that fired at a phrase boundary as a comma
/// list (S4); `motif_boundary` is reserved for S5+ and never fires here.
fn phrase_reasons(r: boundary::BoundaryReason) -> String {
    let mut tags: Vec<&str> = Vec::new();
    if r.pause {
        tags.push("pause");
    }
    if r.cadence {
        tags.push("cadence");
    }
    if r.rhythm_reset {
        tags.push("rhythm_reset");
    }
    if r.register_jump {
        tags.push("register_jump");
    }
    if r.density_change {
        tags.push("density_change");
    }
    if r.manual_override {
        tags.push("manual");
    }
    if tags.is_empty() {
        "-".to_owned()
    } else {
        tags.join(", ")
    }
}

/// Generates a fresh riff seeded from the source's musical material — its
/// pitch palette, meter, tempo, and range — as a reranked candidate set
/// (research note §7.2/§7.3): every S6 strategy contributes `candidates`
/// seed variants, each candidate is scored on the closure + novelty axes
/// under the `generation_rerank` v1 policy, and the winner is written to a
/// MIDI file.
///
/// Without `--corpus`, the rhythm template is the input's first sounding bar
/// and novelty has nothing to measure against (all candidates read fully
/// novel). With `--corpus`, rhythm templates, novelty references, and the
/// burst/rest gesture ask come from the curated chunks.
fn cmd_generate(input: &Path, output: &Path, opts: &GenerateOpts<'_>) -> Result<(), CliError> {
    let GenerateOpts {
        seed,
        bars,
        corpus,
        candidates,
        no_gesture,
        rhythm,
        emit_rhythm_expansion,
    } = *opts;
    let data = fs::read(input)?;
    let score = import::import_score_auto(&data)?;
    let material = corpus.map(load_corpus_material).transpose()?;

    if let Some(m) = &material {
        print_corpus_summary(m, no_gesture);
    }

    // The pattern plan compiles before any pitch generation, so the artifact
    // shows the structural delta in isolation (spec §1.14).
    let plan = rhythm
        .map(|args| rhythm_pattern::compile_pattern(args, &score, bars))
        .transpose()?;
    if let (Some(plan), Some(path)) = (&plan, emit_rhythm_expansion) {
        // The artifact writes before generation: aliasing the input would
        // clobber the user's tab, aliasing the output would be silently
        // overwritten by the MIDI moments later. Canonical paths when they
        // resolve, lexical comparison otherwise (the output may not exist).
        let clashes = |other: &Path| -> bool {
            match (fs::canonicalize(path), fs::canonicalize(other)) {
                (Ok(a), Ok(b)) => a == b,
                _ => path == other,
            }
        };
        if clashes(input) || clashes(output) {
            return Err(CliError::Argument(
                "--emit-rhythm-expansion must not alias INPUT or OUTPUT".to_owned(),
            ));
        }
        fs::write(path, &plan.artifact_json)?;
    }

    // The shared compiler: the cockpit's Generate panel enters here too, so the
    // two cannot drift.
    let set = ranked_candidates(
        &score,
        material.as_ref(),
        &GenerationAsk {
            seed,
            bars,
            variants_per_strategy: candidates,
            gesture: !no_gesture,
        },
        plan.as_ref().map(|p| p.templates.as_slice()),
    )?;
    let RankedSet {
        ranked,
        base,
        source_rhythms,
        rhythm_explicit,
        gesture,
        policy,
    } = &set;

    if *rhythm_explicit {
        print_explicit_rhythm_diagnostics(source_rhythms, &base.constraints);
    } else {
        print_rhythm_diagnostics(source_rhythms, &base.constraints, gesture.is_some());
    }

    let winner = ranked
        .first()
        .ok_or_else(|| CliError::Corpus("no candidate survived scoring".to_owned()))?;

    print_ranking(ranked, policy);

    let out_bytes = midi::export_score(&winner.value.score)?;
    fs::write(output, &out_bytes)?;
    println!(
        "generated {bars} bars ({strategy:?}, seed {seed}) from a {tones}-tone scale \
         ({n} bytes) -> {out}",
        strategy = winner.value.strategy,
        tones = base.pitch_material.intervals.len(),
        n = out_bytes.len(),
        out = output.display(),
    );
    Ok(())
}

/// Prints what the corpus supplied to the pass: chunk / template counts, the
/// gesture ask (and whether `--no-gesture` overrode it), and any skipped
/// records.
fn print_corpus_summary(m: &CorpusMaterial, no_gesture: bool) {
    let gesture_note = m.gesture.map_or_else(
        || "no gesture stats".to_owned(),
        |g| {
            format!(
                "gesture burst {} / rest {}q{}",
                g.burst_notes,
                g.rest_quarters,
                if no_gesture { " (skipped)" } else { "" },
            )
        },
    );
    println!(
        "corpus: {} chunks ({} rhythm templates, {gesture_note}){}",
        m.references.len(),
        m.rhythms.len(),
        if m.skipped.is_empty() {
            String::new()
        } else {
            format!(", skipped: {}", m.skipped.join(", "))
        },
    );
}

/// Prints a deterministic rhythm-grid diagnostic for the run: how many
/// templates were loaded vs effective (after empty-removal + clamp), the bar
/// count, whether gesture carving is on, and the fingerprints of the first
/// `min(bars, effective)` grids a run of `bars` bars actually rotates through.
///
/// A small transparency seam for corpus A/B, not an analytics subsystem: with
/// no corpus, `source_rhythms` is the input's own first-bar rhythm (one
/// template, so `1 loaded / 1 effective`), so the two A/B legs are directly
/// comparable. `effective == 0` means the quarter fallback was used.
/// Prints the explicit palette's diagnostics — uncompressed by law (ADR-0029
/// §7): every template counts, silent bars included, no quarter fallback.
fn print_explicit_rhythm_diagnostics(
    palette: &[generate::RhythmTemplate],
    constraints: &generate::GenerationConstraints,
) {
    let Ok(bar_duration) =
        generate::bar_duration_ticks(constraints.time_signature, constraints.ticks_per_quarter)
    else {
        return;
    };
    let diag = generate::explicit_rhythm_diagnostics(palette, bar_duration);
    let hexes: Vec<String> = diag.fingerprints.iter().map(|h| format!("{h:x}")).collect();
    println!(
        "rhythm: explicit palette, {count} templates — per-bar strategies rotate them verbatim, RepeatVariation holds the first — over {bars} bars; grids[{count}] {fps}",
        count = diag.effective,
        bars = constraints.bar_count,
        fps = hexes.join(" "),
    );
}

fn print_rhythm_diagnostics(
    source_rhythms: &[generate::RhythmTemplate],
    constraints: &generate::GenerationConstraints,
    gesture_on: bool,
) {
    let Ok(bar_duration) =
        generate::bar_duration_ticks(constraints.time_signature, constraints.ticks_per_quarter)
    else {
        return;
    };
    let diag = generate::rhythm_diagnostics(source_rhythms, bar_duration);
    let shown = diag.effective.min(constraints.bar_count);
    let fingerprints = if diag.effective == 0 {
        " (quarter fallback)".to_owned()
    } else {
        let hexes: Vec<String> = diag
            .fingerprints
            .iter()
            .take(shown)
            .map(|h| format!("{h:x}"))
            .collect();
        format!("; grids[{shown}] {}", hexes.join(" "))
    };
    println!(
        "rhythm: {loaded} loaded / {effective} effective templates over {bars} bars; gesture {onoff}{fingerprints}",
        loaded = diag.loaded,
        effective = diag.effective,
        bars = constraints.bar_count,
        onoff = if gesture_on { "on" } else { "off" },
    );
}

/// Prints the ranked candidate list with its policy provenance (ADR-0017).
fn print_ranking(ranked: &[scoring::Scored<rerank::SetCandidate>], policy: &scoring::WeightPolicy) {
    // `ranked.len()` is the *total* candidate count (variants × strategies),
    // not the `--candidates` flag — spelled out so the summary is unambiguous.
    println!(
        "candidates: {} total ranked under {} v{}",
        ranked.len(),
        policy.id,
        policy.version,
    );
    for (rank, scored) in ranked.iter().enumerate() {
        println!(
            "  {}. {:?} variant-seed {} aggregate {:.3}",
            rank.saturating_add(1),
            scored.value.strategy,
            scored.value.seed.0,
            scored.aggregate(),
        );
    }
}

/// Options of `griff generate` beyond the input/output pair.
#[derive(Debug, Clone, Copy)]
struct GenerateOpts<'a> {
    /// Deterministic base seed.
    seed: u64,
    /// Bars to generate.
    bars: usize,
    /// Corpus directory, when one is given.
    corpus: Option<&'a Path>,
    /// Seed variants per strategy in the candidate set.
    candidates: usize,
    /// Skip gesture carving even when the corpus provides stats.
    no_gesture: bool,
    /// The compiled `--rhythm-*` ask, when a kernel was given.
    rhythm: Option<&'a rhythm_pattern::RhythmPatternArgs>,
    /// Where to write the expansion artifact, when asked.
    emit_rhythm_expansion: Option<&'a Path>,
}

/// Arranges a complementary part B (S13) for the primary track of `input` and
/// writes A plus B to a MIDI file.
fn cmd_complement(
    input: &Path,
    output: &Path,
    mode: &str,
    seed: u64,
    offset: Option<i8>,
) -> Result<(), CliError> {
    let data = fs::read(input)?;
    let score = import::import_score_auto(&data)?;
    let relation = parse_relation_mode(mode)?;
    // Part A is the first track that actually sounds: GP import keeps rest-only
    // tracks, so track 0 may be empty while a later track carries the riff.
    let track_index = score
        .tracks
        .iter()
        .position(|t| primary_voice_note_count(t) > 0)
        .ok_or(CliError::Complement(
            complement::ComplementError::PartHasNoNotes,
        ))?;
    let spec = complement::ComplementSpec {
        mode: relation,
        register_offset: offset.unwrap_or_else(|| default_offset(relation)),
    };
    let candidate =
        complement::arrange_complement(&score, track_index, spec, generate::GenerationSeed(seed))?;
    let out_bytes = midi::export_score(&candidate.score)?;
    fs::write(output, &out_bytes)?;
    println!(
        "complement ({label}, seed {seed}) — part B appended as track {b} \
         ({n} bytes) -> {out}",
        label = relation.label(),
        b = candidate.part_b_index,
        n = out_bytes.len(),
        out = output.display(),
    );
    Ok(())
}

/// The default register shift when `--offset` is omitted: an octave down for
/// the modes that reject a zero shift, otherwise none.
const fn default_offset(mode: complement::RelationMode) -> i8 {
    match mode {
        complement::RelationMode::OctaveDouble | complement::RelationMode::RegisterContrast => -12,
        _ => 0,
    }
}

/// Parses a `--mode` string into a [`complement::RelationMode`].
fn parse_relation_mode(mode: &str) -> Result<complement::RelationMode, CliError> {
    Ok(match mode {
        "rhythm_lock" => complement::RelationMode::RhythmLock,
        "register_contrast" => complement::RelationMode::RegisterContrast,
        "call_response" => complement::RelationMode::CallResponse,
        "support_layer" => complement::RelationMode::SupportLayer,
        "octave_double" => complement::RelationMode::OctaveDouble,
        "counter_melody" => complement::RelationMode::CounterMelody,
        other => {
            return Err(CliError::Argument(format!(
                "unknown complement mode '{other}' (try rhythm_lock, register_contrast, \
                 call_response, support_layer, octave_double, counter_melody)"
            )));
        }
    })
}

fn cmd_curate(path: &Path, output: Option<&Path>, ensemble: bool) -> Result<(), CliError> {
    let data = fs::read(path)?;
    let score = import::import_score_auto(&data)?;

    print_score_summary(path, &score);
    let inputs = gather_curate_inputs(ensemble)?;

    if ensemble {
        curate_ensemble(path, output, &score, &inputs)
    } else {
        curate_single(path, output, &score, &inputs)
    }
}

/// Single-chunk curation: the first note-bearing track carries the metrics.
fn curate_single(
    path: &Path,
    output: Option<&Path>,
    score: &Score,
    inputs: &CurateInputs,
) -> Result<(), CliError> {
    let measured_track = score
        .tracks
        .iter()
        .position(|t| primary_voice_note_count(t) > 0);
    let meta = build_chunk_meta(
        score,
        path,
        measured_track,
        inputs.id.clone(),
        inputs.title.clone(),
        inputs,
        None,
    );
    print_measurements(&meta);

    let out_path = output.map_or_else(|| path.with_extension("chunk.json"), PathBuf::from);
    let json = serde_json::to_string_pretty(&meta).map_err(CliError::Json)?;
    write_output(&out_path, &json)
}

/// Ensemble curation (corpus schema v4): every note-bearing track becomes a
/// linked single-part chunk, and the group record persists the measured
/// pairwise relation axes.
fn curate_ensemble(
    path: &Path,
    output: Option<&Path>,
    score: &Score,
    inputs: &CurateInputs,
) -> Result<(), CliError> {
    let tracks: Vec<usize> = score
        .tracks
        .iter()
        .enumerate()
        .filter(|(_, t)| primary_voice_note_count(t) > 0)
        .map(|(i, _)| i)
        .collect();
    if tracks.len() < 2 {
        return Err(CliError::Ensemble(
            "ensemble curation needs at least two tracks with notes in their primary voice"
                .to_owned(),
        ));
    }
    let stem = output.map_or_else(|| path.with_extension(""), Path::to_path_buf);

    let mut members = Vec::new();
    for (part, &track) in tracks.iter().enumerate() {
        let id = format!("{}_p{part}", inputs.id);
        let title = format!("{} (part {part})", inputs.title);
        let link = EnsembleRef {
            group_id: inputs.id.clone(),
            part_index: u32::try_from(part).unwrap_or(0),
        };
        let meta = build_chunk_meta(
            score,
            path,
            Some(track),
            id.clone(),
            title,
            inputs,
            Some(link),
        );
        println!("part {part} (track {track}):");
        print_measurements(&meta);
        let json = serde_json::to_string_pretty(&meta).map_err(CliError::Json)?;
        write_output(
            &PathBuf::from(format!("{}.p{part}.chunk.json", stem.display())),
            &json,
        )?;
        members.push(ChunkId(id));
    }

    let group = EnsembleGroup {
        id: inputs.id.clone(),
        members,
        relations: measure_group_relations(score, &tracks)?,
    };
    let json = serde_json::to_string_pretty(&group).map_err(CliError::Json)?;
    write_output(
        &PathBuf::from(format!("{}.group.json", stem.display())),
        &json,
    )
}

/// Phrase-split curation: one chunk per detected phrase, each a standalone
/// bar-range slice carrying its own measurements and `bar_range` provenance.
fn cmd_split(path: &Path, output: Option<&Path>) -> Result<(), CliError> {
    let data = fs::read(path)?;
    let score = import::import_score_auto(&data)?;

    print_score_summary(path, &score);
    // Split always curates single-track chunks, never an ensemble.
    let inputs = gather_curate_inputs(false)?;
    curate_phrases(path, output, &score, &inputs)
}

/// A phrase chunk paired with, for curation review, whether it near-duplicates
/// an earlier phrase (#76); `None` when it is distinct enough.
#[derive(Debug)]
struct PhraseChunk {
    meta: ChunkMeta,
    duplicate: Option<novelty::PhraseDuplicate>,
}

/// Builds one [`PhraseChunk`] per detected phrase of the first note-bearing
/// track: phrase boundaries cut the bars into segments, each segment is sliced
/// into a standalone score, measured, and stamped with its source `bar_range`
/// (the original bar indices it covers). Later phrases that near-duplicate an
/// earlier one are flagged for curator review (#76).
fn phrase_chunks(
    path: &Path,
    score: &Score,
    inputs: &CurateInputs,
) -> Result<Vec<PhraseChunk>, CliError> {
    let track = score
        .tracks
        .iter()
        .position(|t| primary_voice_note_count(t) > 0)
        .ok_or_else(|| {
            CliError::Split("split needs a track with notes in its primary voice".to_owned())
        })?;
    phrase_chunks_for_track(path, score, inputs, track, None)
}

/// Phrase-splits a specific `track` (rather than the first note-bearing one),
/// optionally linking every chunk to `ensemble`. `griff split` picks the track
/// automatically; `griff ingest` drives this per selected guitar so both parts
/// of one tab become chunks under a shared group.
fn phrase_chunks_for_track(
    path: &Path,
    score: &Score,
    inputs: &CurateInputs,
    track: usize,
    ensemble: Option<EnsembleRef>,
) -> Result<Vec<PhraseChunk>, CliError> {
    let cuts: Vec<u32> = detect_boundaries(score, track)
        .iter()
        .map(|b| b.start_tick)
        .collect();
    // Cap over-long phrases so curation never sees a 30-bar blob (#76).
    let segments = split::cap_segment_bars(
        &split::bar_segments(&score.master_bars, &cuts),
        split::MAX_PHRASE_BARS,
    );
    if segments.is_empty() {
        return Err(CliError::Split("score has no bars to split".to_owned()));
    }

    let mut chunks = chunks_for_segments(path, score, inputs, track, &segments);
    // Link every phrase to its ensemble group after the split — `ChunkMeta`
    // owns the field, so no split-path signature has to carry it.
    if let Some(link) = ensemble {
        for chunk in &mut chunks {
            chunk.meta.ensemble = Some(link.clone());
        }
    }
    Ok(chunks)
}

/// Builds one [`PhraseChunk`] per segment in which `track` sounds, renumbered
/// from 0.
///
/// `track` is the part chosen for boundary detection; every chunk is cut *and*
/// measured on that same part so its boundaries and measurements describe one
/// voice (`griff split` is single-track). A segment where `track` is silent is
/// a rest in the phrase: it is dropped rather than re-measured on a later part
/// that happens to have notes there, or written as a measurement-less chunk.
/// `extract_bars` preserves track indices, so `track` addresses the same part
/// in each slice. The stored `bar_range` is inclusive `[first, last]` — the
/// half-open end minus one — matching [`griff_core::corpus::SourceRef`].
///
/// Each kept phrase is then checked against the earlier kept ones: a later phrase
/// whose melodic line verbatim-quotes an earlier one (transposition-aware,
/// [`novelty::flag_phrase_duplicates`]) carries a `duplicate` flag for curator
/// review (#76) — parity with the web split path.
fn chunks_for_segments(
    path: &Path,
    score: &Score,
    inputs: &CurateInputs,
    track: usize,
    segments: &[Range<usize>],
) -> Vec<PhraseChunk> {
    let kept: Vec<(usize, usize, Score)> = segments
        .iter()
        .filter_map(|seg| {
            let sub = slice::extract_bars(score, seg.clone());
            // Stay on the detected track: a segment silent there is a phrase
            // rest, not a cue to measure a different part. Trivial fragments
            // (one-bar cuts, a lone note) are dropped the same way (#76).
            let notes = primary_voice_note_count(sub.tracks.get(track)?);
            if split::is_trivial_phrase(seg.end.saturating_sub(seg.start), notes) {
                return None;
            }
            Some((seg.start, seg.end, sub))
        })
        .collect();

    // Flag near-duplicate phrases (a later repeat of an earlier one) for review.
    let phrase_scores: Vec<Score> = kept.iter().map(|(_, _, sub)| sub.clone()).collect();
    let duplicates =
        novelty::flag_phrase_duplicates(&phrase_scores, track, novelty::PHRASE_DUPLICATE_SHARE);

    kept.into_iter()
        .enumerate()
        .map(|(phrase, (start, end, sub))| {
            let id = format!("{}_p{phrase}", inputs.id);
            let title = format!("{} (phrase {phrase})", inputs.title);
            let mut meta = build_chunk_meta(&sub, path, Some(track), id, title, inputs, None);
            let last = end.saturating_sub(1);
            meta.source.bar_range = Some((
                u32::try_from(start).unwrap_or(0),
                u32::try_from(last).unwrap_or(0),
            ));
            let duplicate = duplicates.get(phrase).copied().flatten();
            // Persist the link onto the record too (#76, schema v8), so a
            // downloaded chunk / manifest keeps it, not just the CLI print.
            meta.duplicate = duplicate;
            PhraseChunk { meta, duplicate }
        })
        .collect()
}

/// Assembles one source file into corpus records for bulk ingest: every
/// selected track is phrase-split, each phrase chunk is linked to a per-file
/// ensemble group (schema v4) so a reader can tell which chunks came from one
/// tab, and all carry the default community-tab rights. Returns the phrase
/// chunks and the group; the caller writes them. Relations stay empty here —
/// this slice records provenance, not measured inter-part dependencies.
fn assemble_ingest_group(
    path: &Path,
    score: &Score,
    selected: &[usize],
    group_id: &str,
    base_title: &str,
) -> Result<(Vec<ChunkMeta>, EnsembleGroup), CliError> {
    let mut records: Vec<ChunkMeta> = Vec::new();
    let mut members: Vec<ChunkId> = Vec::new();
    for (part, &track) in selected.iter().enumerate() {
        let inputs = default_ingest_inputs(score, track, group_id, base_title, part);
        let link = EnsembleRef {
            group_id: group_id.to_owned(),
            part_index: u32::try_from(part).unwrap_or(0),
        };
        for chunk in phrase_chunks_for_track(path, score, &inputs, track, Some(link))? {
            members.push(chunk.meta.id.clone());
            records.push(chunk.meta);
        }
    }
    Ok((
        records,
        EnsembleGroup {
            id: group_id.to_owned(),
            members,
            relations: Vec::new(),
        },
    ))
}

/// The non-interactive curation inputs for a bulk-ingested guitar part: a
/// part-qualified id, the real tuning label, and the default rights for a
/// scraped community tab (copyrighted composition, not redistributable). Tags
/// and reviewer are left empty for the cockpit curation pass.
fn default_ingest_inputs(
    score: &Score,
    track: usize,
    group_id: &str,
    base_title: &str,
    part: usize,
) -> CurateInputs {
    let tuning = score.tracks.get(track).map_or_else(
        || "standard_e".to_owned(),
        |t| ingest::tuning_label(&t.tuning),
    );
    let quality_flags = if score.loss.is_clean() {
        vec![QualityFlag::Clean]
    } else {
        vec![QualityFlag::Lossy]
    };
    CurateInputs {
        id: format!("{group_id}_g{part}"),
        title: format!("{base_title} (guitar {part})"),
        tuning,
        style_cohort: StyleCohort::Core,
        tags: Vec::new(),
        quality_flags,
        reviewer: None,
        rights: RightsInfo {
            rights_status: RightsStatus::CopyrightedComposition,
            acquisition: Acquisition::CommunityTabSite,
            redistributable: false,
            notes: String::new(),
        },
    }
}

/// A group id unique within one ingest run: `base`, or `base_2`, `base_3`, …
/// when `base` (or a lower suffix) is already taken. Two source files that
/// slugify to the same stem — the same song as `.gp5` and `.gpx`, or two
/// versions — must not overwrite each other's chunk records. Records the
/// chosen id in `used`.
fn unique_group_id(base: &str, used: &mut HashSet<String>) -> String {
    if used.insert(base.to_owned()) {
        return base.to_owned();
    }
    let mut n = 2_usize;
    loop {
        let candidate = format!("{base}_{n}");
        if used.insert(candidate.clone()) {
            return candidate;
        }
        n = n.saturating_add(1);
    }
}

/// A filesystem-safe, stable id from a source stem: lowercase, runs of
/// non-alphanumerics collapsed to one `_`, edges trimmed.
fn slugify(stem: &str) -> String {
    let mut slug = String::new();
    let mut gap = false;
    for ch in stem.chars() {
        if ch.is_ascii_alphanumeric() {
            if gap && !slug.is_empty() {
                slug.push('_');
            }
            slug.push(ch.to_ascii_lowercase());
            gap = false;
        } else {
            gap = true;
        }
    }
    slug
}

/// Bulk-ingests every tab file in `dir` into corpus chunk + group records,
/// then builds the manifest and prints a skip report.
fn cmd_ingest(dir: &Path, output: Option<&Path>, with_bass: bool) -> Result<(), CliError> {
    let out = output.map_or_else(|| PathBuf::from("corpus"), Path::to_path_buf);
    fs::create_dir_all(&out)?;

    let mut entries: Vec<PathBuf> = fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file())
        .collect();
    entries.sort();

    let mut ingested = 0_usize;
    let mut chunk_total = 0_usize;
    let mut skipped: Vec<(String, String)> = Vec::new();
    let mut used_ids: HashSet<String> = HashSet::new();

    for path in &entries {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("?")
            .to_owned();
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("untitled");

        let bytes = match fs::read(path) {
            Ok(b) => b,
            Err(err) => {
                skipped.push((name, format!("read error: {err}")));
                continue;
            }
        };
        let score = match import::import_score_auto(&bytes) {
            Ok(score) => score,
            Err(err) => {
                skipped.push((name, format!("import error: {err}")));
                continue;
            }
        };

        let selected = ingest::select_ingest_tracks(&score, with_bass);
        if selected.is_empty() {
            skipped.push((name, "no guitar or bass track".to_owned()));
            continue;
        }

        let group_id = unique_group_id(&slugify(stem), &mut used_ids);
        let (records, group) = assemble_ingest_group(path, &score, &selected, &group_id, stem)?;
        if records.is_empty() {
            skipped.push((name, "no phrase survived splitting".to_owned()));
            continue;
        }

        for meta in &records {
            let json = serde_json::to_string_pretty(meta).map_err(CliError::Json)?;
            write_output(&out.join(format!("{}.chunk.json", meta.id.0)), &json)?;
            chunk_total = chunk_total.saturating_add(1);
        }
        let group_json = serde_json::to_string_pretty(&group).map_err(CliError::Json)?;
        write_output(&out.join(format!("{group_id}.group.json")), &group_json)?;
        ingested = ingested.saturating_add(1);
    }

    println!(
        "ingested {ingested} file(s) into {chunk_total} phrase chunk(s) -> {}",
        out.display()
    );
    if !skipped.is_empty() {
        println!("skipped {} file(s):", skipped.len());
        for (file, reason) in &skipped {
            println!("  {file}: {reason}");
        }
    }

    // Refresh the manifest over the whole corpus directory.
    cmd_manifest(&out, None)
}

/// Splits `score` into phrase chunks and writes each to `<stem>.p<N>.chunk.json`.
fn curate_phrases(
    path: &Path,
    output: Option<&Path>,
    score: &Score,
    inputs: &CurateInputs,
) -> Result<(), CliError> {
    let chunks = phrase_chunks(path, score, inputs)?;
    let stem = output.map_or_else(|| path.with_extension(""), Path::to_path_buf);

    let mut flagged = 0_usize;
    for (phrase, chunk) in chunks.iter().enumerate() {
        let (lo, hi) = chunk.meta.source.bar_range.unwrap_or((0, 0));
        println!("phrase {phrase} (bars {lo}..{hi}):");
        if let Some(dup) = chunk.duplicate {
            flagged = flagged.saturating_add(1);
            println!(
                "  near-duplicate of phrase {} (quote {:.2}) — consider dropping",
                dup.of, dup.quote_share
            );
        }
        print_measurements(&chunk.meta);
        let json = serde_json::to_string_pretty(&chunk.meta).map_err(CliError::Json)?;
        write_output(
            &PathBuf::from(format!("{}.p{phrase}.chunk.json", stem.display())),
            &json,
        )?;
    }
    if flagged > 0 {
        println!(
            "split into {} phrase chunk(s); {flagged} near-duplicate(s) flagged for review",
            chunks.len()
        );
    } else {
        println!("split into {} phrase chunk(s)", chunks.len());
    }
    Ok(())
}

/// Measured pairwise relation axes over the group's parts, ordered by
/// `(a, b)` part indices (axes read *b relative to a*).
///
/// A failed measurement is an error, not a gap: silently omitting a relation
/// would persist an incomplete complement hyperedge (Codex P2, PR #36).
fn measure_group_relations(score: &Score, tracks: &[usize]) -> Result<Vec<PairRelation>, CliError> {
    let mut relations = Vec::new();
    for (i, &track_a) in tracks.iter().enumerate() {
        for (j, &track_b) in tracks.iter().enumerate().skip(i.saturating_add(1)) {
            let axes = complement::measure_pair_axes(score, track_a, track_b).map_err(|e| {
                CliError::Ensemble(format!(
                    "pair measurement failed for parts {i}/{j} (tracks {track_a}/{track_b}): {e:?}"
                ))
            })?;
            relations.push(PairRelation {
                parts: (u32::try_from(i).unwrap_or(0), u32::try_from(j).unwrap_or(0)),
                axes,
            });
        }
    }
    Ok(relations)
}

/// Maps an imported score's source-format tag (set by the importer) to the
/// corpus [`SourceFormat`]; an unknown or absent tag falls back to MIDI.
fn source_format(score: &Score) -> SourceFormat {
    match score.source_meta.as_ref().and_then(|m| m.format.as_deref()) {
        Some("GP3") => SourceFormat::Gp3,
        Some("GP4") => SourceFormat::Gp4,
        Some("GP5") => SourceFormat::Gp5,
        Some("GP6") => SourceFormat::Gpx,
        _ => SourceFormat::Midi,
    }
}

/// Detects phrase boundaries (S4) for `track_index`, scaling the detector's
/// tick gaps to the score's PPQN exactly as `griff phrases` does, and maps them
/// to corpus [`BoundaryEntry`] records.
fn detect_boundaries(score: &Score, track_index: usize) -> Vec<BoundaryEntry> {
    let ppqn = u32::from(score.ticks_per_quarter);
    let config = boundary::BoundaryConfig {
        min_gap: Ticks(ppqn.saturating_mul(2)),
        quantize_ticks: Ticks(ppqn.checked_div(4).unwrap_or(1).max(1)),
        ..boundary::BoundaryConfig::default()
    };
    boundary::detect_phrase_boundaries(score, track_index, &config)
        .into_iter()
        .map(|b| BoundaryEntry {
            start_tick: b.start_tick.0,
            end_tick: b.end_tick.0,
            score: b.score,
        })
        .collect()
}

/// Measures `track_index` (when present) and assembles one chunk record.
#[allow(clippy::too_many_arguments)] // a private assembly seam shared by both curate modes
fn build_chunk_meta(
    score: &Score,
    path: &Path,
    track_index: Option<usize>,
    id: String,
    title: String,
    inputs: &CurateInputs,
    ensemble: Option<EnsembleRef>,
) -> ChunkMeta {
    let (tempo_bpm, time_signature) =
        score
            .master_bars
            .first()
            .map_or((120.0, (4_u8, 4_u8)), |b| {
                (
                    b.tempo.0,
                    (b.time_signature.numerator, b.time_signature.denominator),
                )
            });
    let filename = path
        .file_name()
        .and_then(|n| n.to_str())
        .map_or_else(|| "unknown.mid".to_owned(), ToOwned::to_owned);

    let structure = track_index.and_then(|idx| structure::measure_structure(score, idx).ok());
    let gesture = track_index.and_then(|idx| gesture::measure_gesture(score, idx).ok());
    let complexity = track_index.and_then(|idx| structure::measure_complexity(score, idx).ok());
    let boundaries = track_index.map_or_else(Vec::new, |idx| detect_boundaries(score, idx));
    // Auto-derive techniques from the track's notation (ADR-0018): the tags
    // merge with the curator's choices, the free-form list fills `techniques`.
    let derived = track_index
        .map(|idx| technique::derive_techniques(score, idx))
        .unwrap_or_default();
    // Auto-derive chord-quality tags from the same notation (#75); they merge
    // additively too, like the technique tags.
    let derived_harmony = track_index
        .map(|idx| harmony::derive_harmony(score, idx))
        .unwrap_or_default();
    // Auto-derive the syncopated rhythm tag from onset placement (#75); merged
    // additively as well.
    let derived_syncopated = track_index
        .map(|idx| syncopation::derive_syncopated(score, idx))
        .unwrap_or_default();

    let now = "2026-05-20T00:00:00Z".to_owned();
    ChunkMeta {
        id: ChunkId(id),
        title,
        source: SourceRef {
            filename,
            format: source_format(score),
            bar_range: None,
        },
        tempo_bpm,
        ticks_per_quarter: score.ticks_per_quarter,
        time_signature,
        tuning: inputs.tuning.clone(),
        tags: {
            // Additive: curator choices first, then derived technique, chord-
            // quality (#75), and syncopation (#75) tags — none overrides another.
            let with_techniques = technique::merge_tags(&inputs.tags, &derived.tags);
            let with_harmony = technique::merge_tags(&with_techniques, &derived_harmony);
            technique::merge_tags(&with_harmony, &derived_syncopated)
        },
        boundaries,
        techniques: derived.names,
        quality_flags: inputs.quality_flags.clone(),
        reviewer: inputs.reviewer,
        structure,
        gesture,
        complexity,
        duplicate: None,
        style_cohort: Some(inputs.style_cohort),
        ensemble,
        rights: Some(inputs.rights.clone()),
        created_at: now.clone(),
        updated_at: now,
    }
}

/// Prints the one-line measurement summaries of a built record.
fn print_measurements(meta: &ChunkMeta) {
    if let Some(m) = &meta.structure {
        let period = m
            .detected_pattern_period_bars
            .map_or_else(|| "through-composed".to_owned(), |p| format!("{p} bar(s)"));
        println!(
            "Structure: period={period}  repeatability={rep:.2}  loopability={lp:.2}",
            rep = m.repeatability_score,
            lp = m.loopability_score,
        );
    }
    if let Some(g) = &meta.gesture {
        println!(
            "Gesture: bursts={bursts} (mean {mean:.1} notes)  rests={rests} (on-grid {grid:.2})",
            bursts = g.burst_count,
            mean = g.mean_burst_notes,
            rests = g.rest_count,
            grid = g.rest_on_grid_share,
        );
    }
}

/// Writes a serialized record and reports the path.
fn write_output(path: &Path, json: &str) -> Result<(), CliError> {
    fs::write(path, json)?;
    println!("wrote {}", path.display());
    Ok(())
}

/// Builds a [`CorpusManifest`] from a directory of curated records, prints a
/// coverage summary, and writes the manifest. Globs `*.chunk.json` into chunks
/// and `*.group.json` into groups (both sorted, so the manifest is
/// deterministic regardless of directory order).
fn cmd_manifest(dir: &Path, output: Option<&Path>) -> Result<(), CliError> {
    let mut chunk_paths: Vec<PathBuf> = Vec::new();
    let mut group_paths: Vec<PathBuf> = Vec::new();
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name.ends_with(".group.json") {
            group_paths.push(path);
        } else if name.ends_with(".chunk.json") {
            chunk_paths.push(path);
        }
    }
    chunk_paths.sort();
    group_paths.sort();

    let mut chunks = Vec::with_capacity(chunk_paths.len());
    for path in &chunk_paths {
        let json = fs::read_to_string(path)?;
        chunks.push(serde_json::from_str::<ChunkMeta>(&json).map_err(CliError::Json)?);
    }
    let mut groups = Vec::with_capacity(group_paths.len());
    for path in &group_paths {
        let json = fs::read_to_string(path)?;
        groups.push(serde_json::from_str::<EnsembleGroup>(&json).map_err(CliError::Json)?);
    }

    let manifest = CorpusManifest {
        schema_version: SCHEMA_VERSION,
        chunks,
        groups,
    };
    print_manifest_summary(&manifest);

    let out_path = output.map_or_else(|| dir.join("manifest.json"), PathBuf::from);
    let json = serde_json::to_string_pretty(&manifest).map_err(CliError::Json)?;
    write_output(&out_path, &json)
}

/// Prints corpus coverage: progress toward the S7 ~100-phrase gate, cohort mix
/// (decisions 2026-06-11 targets ~70-80% core), rights coverage, and reviews.
fn print_manifest_summary(manifest: &CorpusManifest) {
    let chunks = &manifest.chunks;
    let n = chunks.len();
    let core = chunks
        .iter()
        .filter(|c| c.style_cohort == Some(StyleCohort::Core))
        .count();
    let adjacent = chunks
        .iter()
        .filter(|c| c.style_cohort == Some(StyleCohort::Adjacent))
        .count();
    let accepted = chunks
        .iter()
        .filter(|c| c.reviewer == Some(ReviewerDecision::Accepted))
        .count();
    let with_rights = chunks.iter().filter(|c| c.rights.is_some()).count();
    let redistributable = chunks
        .iter()
        .filter(|c| c.rights.as_ref().is_some_and(|r| r.redistributable))
        .count();

    println!("Corpus manifest (schema v{})", manifest.schema_version);
    println!("Chunks : {n}  (S7 graph layer recommended at ~100)");
    println!(
        "Cohort : {core} core / {adjacent} adjacent / {} unlabeled",
        n.saturating_sub(core).saturating_sub(adjacent)
    );
    println!("Review : {accepted}/{n} accepted");
    println!("Rights : {with_rights}/{n} recorded · {redistributable} redistributable");
    if !manifest.groups.is_empty() {
        println!("Groups : {}", manifest.groups.len());
    }
}

struct CurateInputs {
    id: String,
    title: String,
    tuning: String,
    style_cohort: StyleCohort,
    tags: Vec<SwancoreTag>,
    quality_flags: Vec<QualityFlag>,
    reviewer: Option<ReviewerDecision>,
    rights: RightsInfo,
}

fn gather_curate_inputs(ensemble: bool) -> Result<CurateInputs, CliError> {
    let mut input_buf = String::new();

    let id_label = if ensemble {
        "Group ID (e.g. dgd_042)"
    } else {
        "Chunk ID (e.g. dgd_001)"
    };
    let id = prompt_line(&mut input_buf, id_label)?;
    let title = prompt_line(&mut input_buf, "Title")?;
    let tuning_raw = prompt_line(&mut input_buf, "Tuning [standard_e]")?;
    let tuning = if tuning_raw.trim().is_empty() {
        "standard_e".to_owned()
    } else {
        tuning_raw.trim().to_owned()
    };

    println!("Style cohort: 0=core  1=adjacent (decisions 2026-06-11)");
    let cohort_input = prompt_line(&mut input_buf, "Cohort [0=core]")?;
    let style_cohort = if cohort_input.trim() == "1" {
        StyleCohort::Adjacent
    } else {
        StyleCohort::Core
    };

    println!("Tags (space-separated numbers):");
    let all_tags = SwancoreTag::all_variants();
    for (i, t) in all_tags.iter().enumerate() {
        println!("  {i:2}: {t:?}");
    }
    let tag_input = prompt_line(&mut input_buf, "Tags")?;
    let tags = parse_indices(&tag_input, all_tags);

    println!("Quality flags (space-separated numbers):");
    let all_flags = [
        QualityFlag::Clean,
        QualityFlag::Lossy,
        QualityFlag::Quantized,
        QualityFlag::FlatDynamics,
    ];
    for (i, f) in all_flags.iter().enumerate() {
        println!("  {i}: {f:?}");
    }
    let flag_input = prompt_line(&mut input_buf, "Flags [0=Clean]")?;
    let quality_flags = if flag_input.trim().is_empty() {
        vec![QualityFlag::Clean]
    } else {
        parse_indices(&flag_input, &all_flags)
    };

    println!("Reviewer decision: 0=accepted  1=rejected  2=needs_review  (blank=none)");
    let rev_input = prompt_line(&mut input_buf, "Decision")?;
    let reviewer = match rev_input.trim() {
        "0" => Some(ReviewerDecision::Accepted),
        "1" => Some(ReviewerDecision::Rejected),
        "2" => Some(ReviewerDecision::NeedsReview),
        _ => None,
    };

    let rights = gather_rights(&mut input_buf)?;

    Ok(CurateInputs {
        id,
        title,
        tuning,
        style_cohort,
        tags,
        quality_flags,
        reviewer,
        rights,
    })
}

/// Prompts for the schema-v7 rights record (decisions 2026-06-12). Defaults
/// match the common case — a scraped community tab of a copyrighted modern-metal
/// composition, not redistributable — so a blank answer is the safe one.
fn gather_rights(input_buf: &mut String) -> Result<RightsInfo, CliError> {
    println!(
        "Rights status: 0=public_domain  1=cc_by  2=cc_by_sa  \
         3=copyrighted_composition  4=unknown"
    );
    let status_input = prompt_line(input_buf, "Rights [3=copyrighted_composition]")?;
    let rights_status = match status_input.trim() {
        "0" => RightsStatus::PublicDomain,
        "1" => RightsStatus::CcBy,
        "2" => RightsStatus::CcBySa,
        "4" => RightsStatus::Unknown,
        _ => RightsStatus::CopyrightedComposition,
    };

    println!(
        "Acquisition: 0=community_tab_site  1=purchased_official  2=self_transcribed  \
         3=omr_from_scan  4=artist_provided"
    );
    let acq_input = prompt_line(input_buf, "Acquisition [0=community_tab_site]")?;
    let acquisition = match acq_input.trim() {
        "1" => Acquisition::PurchasedOfficial,
        "2" => Acquisition::SelfTranscribed,
        "3" => Acquisition::OmrFromScan,
        "4" => Acquisition::ArtistProvided,
        _ => Acquisition::CommunityTabSite,
    };

    let redist_input = prompt_line(input_buf, "Redistributable? 0=no 1=yes [0=no]")?;
    let redistributable = redist_input.trim() == "1";

    let notes = prompt_line(input_buf, "Rights notes (source URL, date, publisher)")?
        .trim()
        .to_owned();

    Ok(RightsInfo {
        rights_status,
        acquisition,
        redistributable,
        notes,
    })
}

fn print_score_summary(path: &Path, score: &Score) {
    println!("File  : {}", path.display());
    println!("PPQN  : {}", score.ticks_per_quarter);
    println!("Bars  : {}", score.master_bars.len());
    println!("Tracks: {}", score.tracks.len());
    for (idx, track) in score.tracks.iter().enumerate() {
        println!(
            "  [{idx}] ch={ch:02}  notes={notes}  \"{name}\"",
            ch = track.channel,
            notes = track_note_count(track),
            name = track.name.as_deref().unwrap_or("<unnamed>"),
        );
    }
}

fn prompt_line(buf: &mut String, label: &str) -> Result<String, CliError> {
    print!("{label}: ");
    io::stdout().flush()?;
    buf.clear();
    io::stdin().read_line(buf).map_err(CliError::Io)?;
    Ok(buf.trim_end_matches('\n').trim_end_matches('\r').to_owned())
}

fn parse_indices<T: Copy>(input: &str, variants: &[T]) -> Vec<T> {
    input
        .split_whitespace()
        .filter_map(|s| s.parse::<usize>().ok())
        .filter_map(|i| variants.get(i).copied())
        .collect()
}

// ── error ─────────────────────────────────────────────────────────────────────

#[derive(Debug)]
enum CliError {
    Io(IoError),
    Import(ImportError),
    Midi(MidiError),
    Json(serde_json::Error),
    Argument(String),
    Ensemble(String),
    Split(String),
    Generate(generate::GenerationError),
    Set(rerank::SetError),
    Corpus(String),
    Complement(complement::ComplementError),
    Pattern(rhythm_pattern::PatternDiagnostic),
    /// A Swang syntax diagnostic, already rendered against its script:
    /// `error[SWG____] (<path>:<line>:<col>): <message>`.
    Swang(String),
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "I/O error: {e}"),
            Self::Import(e) => write!(f, "import error: {e}"),
            Self::Midi(e) => write!(f, "MIDI error: {e}"),
            Self::Json(e) => write!(f, "JSON error: {e}"),
            Self::Argument(msg) => write!(f, "argument error: {msg}"),
            Self::Ensemble(msg) => write!(f, "ensemble error: {msg}"),
            Self::Split(msg) => write!(f, "split error: {msg}"),
            Self::Generate(e) => write!(f, "generation error: {e:?}"),
            Self::Set(e) => write!(f, "candidate set error: {e:?}"),
            Self::Corpus(msg) => write!(f, "corpus error: {msg}"),
            Self::Complement(e) => write!(f, "complement error: {e:?}"),
            Self::Pattern(d) => write!(f, "{d}"),
            Self::Swang(rendered) => write!(f, "{rendered}"),
        }
    }
}

impl From<IoError> for CliError {
    fn from(e: IoError) -> Self {
        Self::Io(e)
    }
}

impl From<MidiError> for CliError {
    fn from(e: MidiError) -> Self {
        Self::Midi(e)
    }
}

impl From<ImportError> for CliError {
    fn from(e: ImportError) -> Self {
        Self::Import(e)
    }
}

impl From<generate::GenerationError> for CliError {
    fn from(e: generate::GenerationError) -> Self {
        Self::Generate(e)
    }
}

impl From<rerank::SetError> for CliError {
    fn from(e: rerank::SetError) -> Self {
        // Flatten the plain-generation case so it reads the same wherever it
        // surfaced from.
        match e {
            rerank::SetError::Generation(g) => Self::Generate(g),
            other => Self::Set(other),
        }
    }
}

impl From<GenerationInputError> for CliError {
    fn from(e: GenerationInputError) -> Self {
        match e {
            GenerationInputError::Generation(g) => Self::Generate(g),
            GenerationInputError::Set(s) => Self::Set(s),
            GenerationInputError::Corpus(msg) => Self::Corpus(msg),
        }
    }
}

impl From<complement::ComplementError> for CliError {
    fn from(e: complement::ComplementError) -> Self {
        Self::Complement(e)
    }
}

impl From<rhythm_pattern::PatternDiagnostic> for CliError {
    fn from(d: rhythm_pattern::PatternDiagnostic) -> Self {
        Self::Pattern(d)
    }
}

// ── tests ─────────────────────────────────────────────────────────────────────

/// Red → green for the Codex P2 finding on PR #36: ensemble part selection
/// must follow the first-voice convention every analysis module uses, and a
/// failed pair measurement must surface as an error instead of silently
/// writing an incomplete group.
#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::missing_assert_message,
    clippy::indexing_slicing
)]
mod tests {
    use griff_core::corpus::{ChunkMeta, SourceFormat};
    use griff_core::event::{NoteMarks, Pitch, Tempo, Ticks, TimeSignature, Tuning, Velocity};
    use griff_core::generate::RhythmTemplate;
    use griff_core::gesture;
    use griff_core::score::{
        AtomEvent, AtomNote, EventGroup, EventGroupKind, LossReport, MasterBar, RepeatMarker,
        Score, SourceMeta, Voice,
    };
    use griff_core::slice::TickRange;

    use super::{
        build_chunk_meta, measure_group_relations, primary_voice_note_count, source_format,
        track_note_count, CurateInputs, Track,
    };

    fn score_tagged(format: Option<&str>) -> Score {
        Score {
            ticks_per_quarter: 480,
            master_bars: Vec::new(),
            tracks: Vec::new(),
            source_meta: format.map(|f| SourceMeta {
                format: Some(f.to_owned()),
            }),
            loss: LossReport::new(),
        }
    }

    #[test]
    fn source_format_follows_the_importer_tag() {
        assert_eq!(
            source_format(&score_tagged(Some("MIDI"))),
            SourceFormat::Midi
        );
        assert_eq!(source_format(&score_tagged(Some("GP3"))), SourceFormat::Gp3);
        assert_eq!(source_format(&score_tagged(Some("GP4"))), SourceFormat::Gp4);
        assert_eq!(source_format(&score_tagged(Some("GP5"))), SourceFormat::Gp5);
        assert_eq!(source_format(&score_tagged(Some("GP6"))), SourceFormat::Gpx);
        // An unrecognised tag (e.g. GP7) and an absent tag both fall back to MIDI.
        assert_eq!(
            source_format(&score_tagged(Some("GP7"))),
            SourceFormat::Midi
        );
        assert_eq!(source_format(&score_tagged(None)), SourceFormat::Midi);
    }

    fn quarter(start: u32, pitch: u8) -> AtomEvent {
        AtomEvent::Note(AtomNote {
            absolute_start: Ticks(start),
            duration: Ticks(480),
            pitch: Pitch::new(pitch).expect("valid pitch"),
            velocity: Velocity::new(90).expect("valid velocity"),
            marks: NoteMarks::empty(),
            position: None,
        })
    }

    fn voice_of(id: u8, atoms: Vec<AtomEvent>) -> Voice {
        Voice {
            id,
            event_groups: atoms
                .into_iter()
                .map(|a| EventGroup {
                    kind: EventGroupKind::Single,
                    atoms: vec![a],
                    technique_spans: Vec::new(),
                })
                .collect(),
        }
    }

    fn track_of(voices: Vec<Voice>) -> Track {
        Track {
            name: None,
            channel: 0,
            voices,
            tuning: Tuning::standard_e(),
        }
    }

    fn one_bar_score(tracks: Vec<Track>) -> Score {
        Score {
            ticks_per_quarter: 480,
            master_bars: vec![MasterBar {
                index: 0,
                tick_range: TickRange::new(Ticks(0), Ticks(1920)).expect("ordered"),
                time_signature: TimeSignature {
                    numerator: 4,
                    denominator: 4,
                },
                tempo: Tempo::new(120.0).expect("120 BPM"),
                repeat: RepeatMarker::default(),
            }],
            tracks,
            source_meta: None,
            loss: LossReport::new(),
        }
    }

    #[test]
    fn build_chunk_meta_auto_fills_techniques_and_merges_tags() {
        use super::{build_chunk_meta, CurateInputs};
        use griff_core::corpus::{
            Acquisition, QualityFlag, RightsInfo, RightsStatus, StyleCohort, SwancoreTag,
        };
        use griff_core::event::{NoteMark, SpanTechnique, TechniqueEvidence};
        use griff_core::score::TechniqueSpan;
        use std::path::Path;

        // Voice 0: one pinch-harmonic note under a hammer-on span.
        let note = AtomEvent::Note(AtomNote {
            absolute_start: Ticks(0),
            duration: Ticks(480),
            pitch: Pitch::new(60).expect("pitch"),
            velocity: Velocity::new(90).expect("velocity"),
            marks: NoteMarks::empty().with(NoteMark::HarmonicPinch),
            position: None,
        });
        let track = track_of(vec![Voice {
            id: 0,
            event_groups: vec![EventGroup {
                kind: EventGroupKind::Single,
                atoms: vec![note],
                technique_spans: vec![TechniqueSpan {
                    technique: SpanTechnique::HammerOn,
                    tick_range: TickRange::new(Ticks(0), Ticks(480)).expect("range"),
                    evidence: TechniqueEvidence::explicit(),
                }],
            }],
        }]);
        let score = one_bar_score(vec![track]);

        let inputs = CurateInputs {
            id: "dgd_001".to_owned(),
            title: "Riff".to_owned(),
            tuning: "standard_e".to_owned(),
            style_cohort: StyleCohort::Core,
            tags: vec![SwancoreTag::Intro], // the curator picked one tag by hand
            quality_flags: vec![QualityFlag::Clean],
            reviewer: None,
            rights: RightsInfo {
                rights_status: RightsStatus::CopyrightedComposition,
                acquisition: Acquisition::CommunityTabSite,
                redistributable: false,
                notes: String::new(),
            },
        };
        let meta = build_chunk_meta(
            &score,
            Path::new("riff.gp5"),
            Some(0),
            inputs.id.clone(),
            inputs.title.clone(),
            &inputs,
            None,
        );

        // `techniques` is auto-filled from the notation…
        assert!(
            meta.techniques.contains(&"hammer_on".to_owned()),
            "{:?}",
            meta.techniques
        );
        assert!(
            meta.techniques.contains(&"pinch_harmonic".to_owned()),
            "{:?}",
            meta.techniques
        );
        // …and the curator's hand-picked tag survives alongside the derived ones.
        assert!(meta.tags.contains(&SwancoreTag::Intro), "{:?}", meta.tags);
        assert!(
            meta.tags.contains(&SwancoreTag::HammerOn),
            "{:?}",
            meta.tags
        );
        assert!(
            meta.tags.contains(&SwancoreTag::ArtificialHarmonic),
            "{:?}",
            meta.tags
        );
    }

    #[test]
    fn build_chunk_meta_auto_fills_syncopated_from_displaced_onsets() {
        use super::{build_chunk_meta, CurateInputs};
        use griff_core::corpus::{
            Acquisition, QualityFlag, RightsInfo, RightsStatus, StyleCohort, SwancoreTag,
        };
        use std::path::Path;

        // Beat 1 struck and the "and of 2" (720) anticipates beat 3 (960, unstruck):
        // 1 of 4 beats displaced = 0.25, the inclusive threshold. Guards the CLI
        // merge seam against parity drift versus the web front.
        let track = track_of(vec![voice_of(0, vec![quarter(0, 60), quarter(720, 60)])]);
        let score = one_bar_score(vec![track]);

        let inputs = CurateInputs {
            id: "dgd_001".to_owned(),
            title: "Riff".to_owned(),
            tuning: "standard_e".to_owned(),
            style_cohort: StyleCohort::Core,
            tags: Vec::new(),
            quality_flags: vec![QualityFlag::Clean],
            reviewer: None,
            rights: RightsInfo {
                rights_status: RightsStatus::CopyrightedComposition,
                acquisition: Acquisition::CommunityTabSite,
                redistributable: false,
                notes: String::new(),
            },
        };
        let meta = build_chunk_meta(
            &score,
            Path::new("riff.gp5"),
            Some(0),
            inputs.id.clone(),
            inputs.title.clone(),
            &inputs,
            None,
        );

        assert!(
            meta.tags.contains(&SwancoreTag::Syncopated),
            "syncopated tag should be auto-derived: {:?}",
            meta.tags
        );
    }

    #[test]
    fn primary_voice_note_count_ignores_secondary_voices() {
        // Notes only in voice 1: every analysis module reads voice 0, so the
        // curate selection predicate must agree and skip this track.
        let track = track_of(vec![
            voice_of(0, Vec::new()),
            voice_of(1, vec![quarter(0, 60), quarter(480, 62)]),
        ]);
        assert_eq!(track_note_count(&track), 2, "all-voice count sees them");
        assert_eq!(
            primary_voice_note_count(&track),
            0,
            "the measurement convention does not"
        );

        let measurable = track_of(vec![voice_of(0, vec![quarter(0, 60)])]);
        assert_eq!(primary_voice_note_count(&measurable), 1);
    }

    /// One 4/4 bar with explicit bounds (no arithmetic in the fixture).
    fn mbar(index: usize, start: u32, end: u32) -> MasterBar {
        MasterBar {
            index,
            tick_range: TickRange::new(Ticks(start), Ticks(end)).expect("ordered"),
            time_signature: TimeSignature {
                numerator: 4,
                denominator: 4,
            },
            tempo: Tempo::new(120.0).expect("bpm"),
            repeat: RepeatMarker::default(),
        }
    }

    /// Four contiguous 4/4 bars spanning ticks 0..7680.
    fn split_master_bars() -> Vec<MasterBar> {
        vec![
            mbar(0, 0, 1920),
            mbar(1, 1920, 3840),
            mbar(2, 3840, 5760),
            mbar(3, 5760, 7680),
        ]
    }

    /// Single-track curation inputs with id `dgd`.
    fn split_inputs() -> CurateInputs {
        use griff_core::corpus::{Acquisition, QualityFlag, RightsInfo, RightsStatus, StyleCohort};
        CurateInputs {
            id: "dgd".to_owned(),
            title: "Riff".to_owned(),
            tuning: "standard_e".to_owned(),
            style_cohort: StyleCohort::Core,
            tags: Vec::new(),
            quality_flags: vec![QualityFlag::Clean],
            reviewer: None,
            rights: RightsInfo {
                rights_status: RightsStatus::CopyrightedComposition,
                acquisition: Acquisition::CommunityTabSite,
                redistributable: false,
                notes: String::new(),
            },
        }
    }

    #[test]
    fn phrase_chunks_tile_the_bars_with_inclusive_bar_range_and_ids() {
        use super::phrase_chunks;
        use std::path::Path;

        // Four bars, a note on each downbeat.
        let voice = voice_of(
            0,
            vec![
                quarter(0, 60),
                quarter(1920, 62),
                quarter(3840, 64),
                quarter(5760, 65),
            ],
        );
        let score = Score {
            ticks_per_quarter: 480,
            master_bars: split_master_bars(),
            tracks: vec![track_of(vec![voice])],
            source_meta: None,
            loss: LossReport::new(),
        };

        let chunks = phrase_chunks(Path::new("riff.gp5"), &score, &split_inputs()).expect("splits");
        assert!(!chunks.is_empty(), "at least one phrase chunk");

        // Whatever the detector decides, the chunks tile the four bars with
        // inclusive `[first, last]` ranges, each id suffixed by its phrase index.
        let mut next = 0_u32;
        for (i, chunk) in chunks.iter().enumerate() {
            let (lo, hi) = chunk.meta.source.bar_range.expect("bar_range set");
            assert_eq!(lo, next, "first bar follows the previous chunk's last + 1");
            assert!(hi >= lo, "inclusive last bar is at least the first");
            assert_eq!(chunk.meta.id.0, format!("dgd_p{i}"));
            next = hi.saturating_add(1);
        }
        assert_eq!(next, 4, "inclusive ranges cover all four bars");
    }

    #[test]
    fn chunks_for_segments_skips_silent_and_uses_inclusive_bar_range() {
        use super::chunks_for_segments;
        use std::path::Path;

        // Notes only in bars 0–1; bars 2–3 are silent.
        let voice = voice_of(0, vec![quarter(0, 60), quarter(1920, 62)]);
        let score = Score {
            ticks_per_quarter: 480,
            master_bars: split_master_bars(),
            tracks: vec![track_of(vec![voice])],
            source_meta: None,
            loss: LossReport::new(),
        };

        // A sounding [0,2) segment and a silent [2,4) one.
        let chunks = chunks_for_segments(
            Path::new("riff.gp5"),
            &score,
            &split_inputs(),
            0,
            &[0..2, 2..4],
        );
        assert_eq!(chunks.len(), 1, "the silent [2,4) segment is dropped");
        assert_eq!(
            chunks[0].meta.source.bar_range,
            Some((0, 1)),
            "inclusive last bar is end-1, not the half-open end"
        );
        assert_eq!(chunks[0].meta.id.0, "dgd_p0", "kept chunks renumber from 0");
    }

    #[test]
    fn chunks_for_segments_stays_on_the_detected_track() {
        use super::chunks_for_segments;
        use std::path::Path;

        // Track 0 — the boundary-detection track — sounds only in bars 0–1; a
        // second track sounds only in bars 2–3. `griff split` cuts single-track
        // chunks from the detected track, so the [2,4) segment, silent on that
        // track, is a rest in this phrase and must be dropped rather than
        // re-measured on the later track that happens to have notes there.
        let detected = track_of(vec![voice_of(0, vec![quarter(0, 60), quarter(1920, 62)])]);
        let other = track_of(vec![voice_of(
            0,
            vec![quarter(3840, 48), quarter(5760, 50)],
        )]);
        let score = Score {
            ticks_per_quarter: 480,
            master_bars: split_master_bars(),
            tracks: vec![detected, other],
            source_meta: None,
            loss: LossReport::new(),
        };

        let chunks = chunks_for_segments(
            Path::new("riff.gp5"),
            &score,
            &split_inputs(),
            0,
            &[0..2, 2..4],
        );
        assert_eq!(
            chunks.len(),
            1,
            "the [2,4) segment is silent on the detected track and is dropped, \
             even though a later track has notes there"
        );
        assert_eq!(
            chunks[0].meta.source.bar_range,
            Some((0, 1)),
            "the kept chunk is the detected track's sounding bars 0–1"
        );
    }

    /// A 480-PPQN 4/4 score of `phrases`, each phrase eight quarter notes spanning
    /// two bars (phrase `i` covers bars `2i`, `2i+1`).
    fn phrase_score(phrases: &[&[u8]]) -> Score {
        let bar_count = phrases.len().saturating_mul(2);
        let master_bars = (0..bar_count)
            .map(|i| {
                let start = u32::try_from(i).unwrap_or(0).saturating_mul(1920);
                MasterBar {
                    index: i,
                    tick_range: TickRange::new(Ticks(start), Ticks(start.saturating_add(1920)))
                        .expect("ordered"),
                    time_signature: TimeSignature {
                        numerator: 4,
                        denominator: 4,
                    },
                    tempo: Tempo::new(120.0).expect("120 BPM"),
                    repeat: RepeatMarker::default(),
                }
            })
            .collect();
        let mut atoms = Vec::new();
        for (pi, phrase) in phrases.iter().enumerate() {
            let phrase_start = u32::try_from(pi).unwrap_or(0).saturating_mul(3840);
            for (qi, &pitch) in phrase.iter().enumerate() {
                let onset =
                    phrase_start.saturating_add(u32::try_from(qi).unwrap_or(0).saturating_mul(480));
                atoms.push(quarter(onset, pitch));
            }
        }
        Score {
            ticks_per_quarter: 480,
            master_bars,
            tracks: vec![track_of(vec![voice_of(0, atoms)])],
            source_meta: None,
            loss: LossReport::new(),
        }
    }

    #[test]
    fn chunks_for_segments_flags_near_duplicate_phrases() {
        use super::chunks_for_segments;
        use std::path::Path;

        // Phrase 0 (bars 0–1) and phrase 2 (bars 4–5) are identical; phrase 1
        // (bars 2–3) is a different contour. `measure_novelty` is transposition-
        // aware, so phrase 1 must differ in *intervals*, not just pitch level.
        let stepwise: &[u8] = &[60, 62, 64, 65, 67, 65, 64, 62];
        let arpeggio: &[u8] = &[60, 64, 67, 72, 71, 67, 64, 60];
        let score = phrase_score(&[stepwise, arpeggio, stepwise]);

        let chunks = chunks_for_segments(
            Path::new("riff.gp5"),
            &score,
            &split_inputs(),
            0,
            &[0..2, 2..4, 4..6],
        );

        assert_eq!(chunks.len(), 3, "three non-trivial phrases");
        assert!(
            chunks[0].duplicate.is_none(),
            "the first occurrence is canonical"
        );
        assert!(
            chunks[1].duplicate.is_none(),
            "a distinct contour is not flagged"
        );
        let dup = chunks[2]
            .duplicate
            .expect("phrase 2 near-duplicates phrase 0");
        assert_eq!(dup.of, 0);
        assert!(dup.quote_share >= 0.8, "share {}", dup.quote_share);
        // The link is also mirrored onto the persisted record (#76, schema v8),
        // so a downloaded chunk / manifest keeps it — not just the CLI print.
        assert_eq!(
            chunks[2].meta.duplicate, chunks[2].duplicate,
            "the duplicate link is persisted on the ChunkMeta"
        );
        assert!(chunks[0].meta.duplicate.is_none());
    }

    #[test]
    fn group_relations_measure_all_pairs() {
        let score = one_bar_score(vec![
            track_of(vec![voice_of(0, vec![quarter(0, 60), quarter(480, 62)])]),
            track_of(vec![voice_of(0, vec![quarter(0, 48), quarter(480, 50)])]),
        ]);
        let relations = measure_group_relations(&score, &[0, 1]).expect("both parts measurable");
        assert_eq!(relations.len(), 1);
        assert_eq!(relations[0].parts, (0, 1));
    }

    #[test]
    fn colliding_stems_get_distinct_group_ids() {
        use super::unique_group_id;
        use std::collections::HashSet;
        let mut used = HashSet::new();
        // Two source files with the same stem (e.g. `Shark Dad.gp5` and
        // `Shark Dad.gpx`) must not overwrite each other's chunks.
        assert_eq!(unique_group_id("shark_dad", &mut used), "shark_dad");
        assert_eq!(unique_group_id("shark_dad", &mut used), "shark_dad_2");
        assert_eq!(unique_group_id("shark_dad", &mut used), "shark_dad_3");
        assert_eq!(unique_group_id("other", &mut used), "other");
    }

    #[test]
    fn slugify_makes_a_stable_id_from_a_messy_stem() {
        use super::slugify;
        assert_eq!(
            slugify("Dance Gavin Dance - Care (ver 2 by X)"),
            "dance_gavin_dance_care_ver_2_by_x"
        );
        assert_eq!(slugify("A Lot Like Birds"), "a_lot_like_birds");
        assert_eq!(slugify("--edge--"), "edge");
    }

    #[test]
    fn ingest_assembles_phrase_chunks_linked_as_one_group() {
        use super::assemble_ingest_group;
        use griff_core::corpus::{Acquisition, RightsStatus};
        use std::path::Path;

        let gtr1 = Track {
            name: Some("Guitar 1".to_owned()),
            channel: 0,
            voices: vec![voice_of(
                0,
                vec![
                    note(0, 480, 52),
                    note(1920, 480, 55),
                    note(3840, 480, 52),
                    note(5760, 480, 57),
                ],
            )],
            tuning: Tuning::standard_e(),
        };
        let drop_d = Tuning::new(
            [64_u8, 59, 55, 50, 45, 38]
                .iter()
                .map(|&m| Pitch::new(m).expect("valid pitch"))
                .collect(),
        );
        let gtr2 = Track {
            name: Some("Guitar 2".to_owned()),
            channel: 0,
            voices: vec![voice_of(
                0,
                vec![
                    note(0, 480, 50),
                    note(1920, 480, 53),
                    note(3840, 480, 50),
                    note(5760, 480, 55),
                ],
            )],
            tuning: drop_d,
        };
        let score = bars_score(4, vec![gtr1, gtr2]);

        let (chunks, group) = assemble_ingest_group(
            Path::new("Some Band - Song.gp"),
            &score,
            &[0, 1],
            "some_band_song",
            "Some Band - Song",
        )
        .expect("assemble succeeds");

        assert!(
            chunks.len() >= 2,
            "each of the two guitars yields at least one phrase chunk"
        );
        assert_eq!(group.id, "some_band_song");
        assert_eq!(group.members.len(), chunks.len());
        assert!(
            group.relations.is_empty(),
            "a provenance group records no measured relations in this slice"
        );

        for chunk in &chunks {
            let link = chunk
                .ensemble
                .as_ref()
                .expect("each chunk links to the group");
            assert_eq!(link.group_id, "some_band_song");
            assert!(link.part_index == 0 || link.part_index == 1);
            let rights = chunk.rights.as_ref().expect("each chunk carries rights");
            assert_eq!(rights.rights_status, RightsStatus::CopyrightedComposition);
            assert_eq!(rights.acquisition, Acquisition::CommunityTabSite);
            assert!(!rights.redistributable);
        }

        let part0 = chunks
            .iter()
            .find(|c| c.ensemble.as_ref().is_some_and(|e| e.part_index == 0))
            .expect("a part-0 chunk");
        let part1 = chunks
            .iter()
            .find(|c| c.ensemble.as_ref().is_some_and(|e| e.part_index == 1))
            .expect("a part-1 chunk");
        assert_eq!(part0.tuning, "standard_e");
        assert_eq!(part1.tuning, "drop_d");
    }

    #[test]
    fn group_relations_propagate_measure_errors() {
        // Track 1 has no notes in its primary voice: the pair measurement
        // fails, and the failure must surface instead of silently writing an
        // incomplete group.
        let score = one_bar_score(vec![
            track_of(vec![voice_of(0, vec![quarter(0, 60)])]),
            track_of(vec![
                voice_of(0, Vec::new()),
                voice_of(1, vec![quarter(0, 48)]),
            ]),
        ]);
        let err = measure_group_relations(&score, &[0, 1]).expect_err("must propagate");
        assert!(
            format!("{err}").contains("part"),
            "the curator sees which measurement failed: {err}"
        );
    }

    // ── corpus-fed generation (research note §7.2/§7.3 wiring) ────────────────
    //
    // TDD red phase: `bar_rhythms`, `gesture_control_from_chunks`, and
    // `load_corpus_material` do not exist yet, so these tests fail to compile
    // until the green step. They specify how `griff generate --corpus <dir>`
    // turns curated chunk records + their source tabs into generator inputs:
    // per-bar rhythm templates, novelty reference scores, and an aggregated
    // gesture ask.

    /// A score of `bar_count` 4/4 bars (1920 ticks each) over `tracks`.
    fn bars_score(bar_count: usize, tracks: Vec<Track>) -> Score {
        let master_bars = (0..bar_count)
            .map(|i| {
                let start = u32::try_from(i).expect("small index").saturating_mul(1920);
                MasterBar {
                    index: i,
                    tick_range: TickRange::new(Ticks(start), Ticks(start.saturating_add(1920)))
                        .expect("ordered"),
                    time_signature: TimeSignature {
                        numerator: 4,
                        denominator: 4,
                    },
                    tempo: Tempo::new(120.0).expect("120 BPM"),
                    repeat: RepeatMarker::default(),
                }
            })
            .collect();
        Score {
            ticks_per_quarter: 480,
            master_bars,
            tracks,
            source_meta: None,
            loss: LossReport::new(),
        }
    }

    /// A note atom of arbitrary duration.
    fn note(start: u32, dur: u32, pitch: u8) -> AtomEvent {
        AtomEvent::Note(AtomNote {
            absolute_start: Ticks(start),
            duration: Ticks(dur),
            pitch: Pitch::new(pitch).expect("valid pitch"),
            velocity: Velocity::new(90).expect("valid velocity"),
            marks: NoteMarks::empty(),
            position: None,
        })
    }

    #[test]
    fn bar_rhythms_extracts_per_bar_templates_and_skips_silent_bars() {
        use griff_cli::generation_input::bar_rhythms;
        use griff_core::generate::TemplateNote;

        // Bar 0: four quarters; bar 1: silent; bar 2: two *syncopated*
        // eighths (off the downbeat, a gap between them). The extracted
        // template must keep the in-bar offsets — rests and syncopation are
        // exactly what the corpus should teach the grid (2026-07-11
        // playtest: back-to-back extraction flattened them away).
        let track = track_of(vec![voice_of(
            0,
            vec![
                note(0, 480, 40),
                note(480, 480, 43),
                note(960, 480, 45),
                note(1440, 480, 47),
                note(4080, 240, 50),
                note(4800, 240, 47),
            ],
        )]);
        let score = bars_score(3, vec![track]);

        assert_eq!(
            bar_rhythms(&score, 0),
            vec![
                RhythmTemplate::from_durations(&[Ticks(480); 4]),
                RhythmTemplate {
                    notes: vec![
                        TemplateNote {
                            offset: Ticks(240),
                            duration: Ticks(240),
                        },
                        TemplateNote {
                            offset: Ticks(960),
                            duration: Ticks(240),
                        },
                    ],
                },
            ],
            "templates keep in-bar offsets; silent bars skipped"
        );
    }

    #[test]
    fn bar_rhythms_dedups_identical_templates() {
        use griff_cli::generation_input::bar_rhythms;

        // Two identical quarter-note bars: the corpus should not drown the
        // template rotation in copies of one rhythm.
        let track = track_of(vec![voice_of(
            0,
            vec![
                note(0, 480, 40),
                note(480, 480, 43),
                note(960, 480, 45),
                note(1440, 480, 47),
                note(1920, 480, 40),
                note(2400, 480, 43),
                note(2880, 480, 45),
                note(3360, 480, 47),
            ],
        )]);
        let score = bars_score(2, vec![track]);

        assert_eq!(
            bar_rhythms(&score, 0),
            vec![RhythmTemplate::from_durations(&[Ticks(480); 4])]
        );
    }

    /// Curate inputs with the community-tab rights defaults.
    fn corpus_test_inputs(id: &str) -> CurateInputs {
        use griff_core::corpus::{Acquisition, QualityFlag, RightsInfo, RightsStatus, StyleCohort};
        CurateInputs {
            id: id.to_owned(),
            title: format!("Chunk {id}"),
            tuning: "standard_e".to_owned(),
            style_cohort: StyleCohort::Core,
            tags: Vec::new(),
            quality_flags: vec![QualityFlag::Clean],
            reviewer: None,
            rights: RightsInfo {
                rights_status: RightsStatus::CopyrightedComposition,
                acquisition: Acquisition::CommunityTabSite,
                redistributable: false,
                notes: String::new(),
            },
        }
    }

    /// Gesture stats whose only meaningful fields here are the two the
    /// control derives from; the rest are plausible fillers.
    fn gesture_stats(mean_burst_notes: f64, mean_rest_quarters: f64) -> gesture::GestureStats {
        gesture::GestureStats {
            note_count: 12,
            burst_count: 3,
            mean_burst_notes,
            max_burst_notes: 6,
            rest_count: if mean_rest_quarters == 0.0 { 0 } else { 2 },
            mean_rest_quarters,
            rest_on_grid_share: 1.0,
            modal_landing_share: 0.5,
            mean_final_lengthening: 0.5,
        }
    }

    /// A chunk record built through the real builder, with its measured
    /// gesture replaced by `stats` (or cleared).
    fn chunk_with_gesture(id: &str, stats: Option<gesture::GestureStats>) -> ChunkMeta {
        use std::path::Path;
        let track = track_of(vec![voice_of(0, vec![quarter(0, 60), quarter(480, 62)])]);
        let score = one_bar_score(vec![track]);
        let inputs = corpus_test_inputs(id);
        let mut meta = build_chunk_meta(
            &score,
            Path::new(&format!("{id}.mid")),
            Some(0),
            inputs.id.clone(),
            inputs.title.clone(),
            &inputs,
            None,
        );
        meta.gesture = stats;
        meta
    }

    #[test]
    fn gesture_control_from_chunks_averages_per_chunk_controls() {
        use griff_cli::generation_input::gesture_control_from_chunks;

        // Per-chunk controls (4, 1.0q) and (2, 3.0q) average to (3, 2.0q);
        // a stats-less chunk is skipped, not treated as zero.
        let chunks = vec![
            chunk_with_gesture("a", Some(gesture_stats(4.0, 1.0))),
            chunk_with_gesture("b", None),
            chunk_with_gesture("c", Some(gesture_stats(2.0, 3.0))),
        ];
        let control = gesture_control_from_chunks(&chunks).expect("stats present");
        assert_eq!(control.burst_notes, 3);
        assert!((control.rest_quarters - 2.0).abs() < 1e-9);
    }

    #[test]
    fn gesture_control_from_chunks_ignores_restless_chunks() {
        use griff_cli::generation_input::gesture_control_from_chunks;

        // A wall-to-wall riff's stats describe one giant burst (mean burst =
        // the whole chunk); letting it vote inflates the ask past ever
        // carving (2026-07-11 playtest: burst 69 over a 32-note request
        // carved nothing). Only chunks that actually rest vote.
        let chunks = vec![
            chunk_with_gesture("wall", Some(gesture_stats(120.0, 0.0))),
            chunk_with_gesture("a", Some(gesture_stats(4.0, 1.0))),
            chunk_with_gesture("b", Some(gesture_stats(2.0, 3.0))),
        ];
        let control = gesture_control_from_chunks(&chunks).expect("resting chunks vote");
        assert_eq!(control.burst_notes, 3, "the restless chunk does not vote");
        assert!((control.rest_quarters - 2.0).abs() < 1e-9);
    }

    #[test]
    fn gesture_control_from_chunks_takes_the_median_against_outliers() {
        use griff_cli::generation_input::gesture_control_from_chunks;

        // One long-burst outlier must not drag the ask out of carving range:
        // the aggregate is the per-axis median, not the mean.
        let chunks = vec![
            chunk_with_gesture("a", Some(gesture_stats(2.0, 1.0))),
            chunk_with_gesture("b", Some(gesture_stats(3.0, 1.5))),
            chunk_with_gesture("c", Some(gesture_stats(100.0, 4.0))),
        ];
        let control = gesture_control_from_chunks(&chunks).expect("stats present");
        assert_eq!(control.burst_notes, 3);
        assert!((control.rest_quarters - 1.5).abs() < 1e-9);
    }

    #[test]
    fn gesture_control_from_chunks_is_none_when_no_chunk_rests() {
        use griff_cli::generation_input::gesture_control_from_chunks;
        let chunks = vec![chunk_with_gesture("wall", Some(gesture_stats(120.0, 0.0)))];
        assert!(
            gesture_control_from_chunks(&chunks).is_none(),
            "an all-wall-to-wall corpus asks for no gesture instead of a degenerate one"
        );
    }

    #[test]
    fn gesture_control_from_chunks_is_none_without_stats() {
        use griff_cli::generation_input::gesture_control_from_chunks;
        let chunks = vec![chunk_with_gesture("a", None)];
        assert!(gesture_control_from_chunks(&chunks).is_none());
    }

    #[test]
    fn load_corpus_material_reads_chunks_slices_ranges_and_skips_missing_sources() {
        use std::{env, fs, process};

        use griff_cli::generation_input::load_corpus_material;
        use griff_core::midi;

        let dir = env::temp_dir().join(format!("griff_corpus_material_{}", process::id()));
        fs::create_dir_all(&dir).expect("create corpus dir");

        // Source tab: two bars, quarters then eighths.
        let track = track_of(vec![voice_of(
            0,
            vec![
                note(0, 480, 40),
                note(480, 480, 43),
                note(960, 480, 45),
                note(1440, 480, 47),
                note(1920, 240, 50),
                note(2160, 240, 47),
                note(2400, 240, 45),
                note(2640, 240, 43),
                note(2880, 240, 40),
                note(3120, 240, 43),
                note(3360, 240, 45),
                note(3600, 240, 47),
            ],
        )]);
        let source = bars_score(2, vec![track]);
        fs::write(
            dir.join("a.mid"),
            midi::export_score(&source).expect("export source"),
        )
        .expect("write source");

        // Chunk a: covers only bar 0 of its source. Chunk b: source missing.
        let inputs = corpus_test_inputs("a");
        let mut meta_a = build_chunk_meta(
            &source,
            &dir.join("a.mid"),
            Some(0),
            inputs.id.clone(),
            inputs.title.clone(),
            &inputs,
            None,
        );
        meta_a.source.bar_range = Some((0, 0));
        fs::write(
            dir.join("a.chunk.json"),
            serde_json::to_string(&meta_a).expect("serialize a"),
        )
        .expect("write a.chunk.json");

        let mut meta_b = chunk_with_gesture("b", None);
        meta_b.source.filename = "missing.mid".to_owned();
        fs::write(
            dir.join("b.chunk.json"),
            serde_json::to_string(&meta_b).expect("serialize b"),
        )
        .expect("write b.chunk.json");

        // A group record must be ignored, not parsed as a chunk.
        fs::write(dir.join("g.group.json"), "{}").expect("write group");

        let material = load_corpus_material(&dir).expect("corpus loads");

        assert_eq!(
            material.references.len(),
            1,
            "one chunk with a readable source"
        );
        assert_eq!(
            material.references[0].master_bars.len(),
            1,
            "bar_range (0, 0) slices the source to one bar"
        );
        assert_eq!(
            material.rhythms,
            vec![RhythmTemplate::from_durations(&[Ticks(480); 4])],
            "rhythm templates come from the sliced bar range only"
        );
        assert_eq!(
            material.skipped,
            vec!["b.chunk.json".to_owned()],
            "records whose source cannot be read are skipped, by record name"
        );

        fs::remove_dir_all(&dir).ok();
    }
}
