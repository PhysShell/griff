use std::{
    fmt, fs,
    io::{self, Error as IoError, Write as IoWrite},
    ops::Range,
    path::{Path, PathBuf},
    process::ExitCode,
};

use clap::{Parser, Subcommand};
use griff_core::{
    boundary,
    classify::{self, BarClass},
    complement,
    corpus::{
        Acquisition, BoundaryEntry, ChunkId, ChunkMeta, CorpusManifest, EnsembleGroup, EnsembleRef,
        PairRelation, QualityFlag, ReviewerDecision, RightsInfo, RightsStatus, SourceFormat,
        SourceRef, StyleCohort, SwancoreTag, SCHEMA_VERSION,
    },
    event::{NoteMarks, NotePosition, Pitch, TechniqueSource, Ticks},
    generate, gesture,
    import::{self, ImportError},
    midi::{self, MidiError},
    score::{AtomEvent, Score, Track, Voice},
    slice::{self, TickRange},
    split, structure, technique, unfold,
};

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

    /// Generate a fresh riff (S6) seeded from a tab's scale, rhythm, meter, and
    /// pitch range, and write it to a MIDI file.
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
        } => cmd_generate(&input, &output, seed, bars),
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
        Command::Manifest { dir, output } => cmd_manifest(&dir, output.as_deref()),
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

/// Note atoms in a track's primary (first) voice — the voice every analysis
/// module measures (`analyze_part`, structure, gesture, …). Curate selects
/// measurable tracks with this predicate so selection and measurement agree.
fn primary_voice_note_count(track: &Track) -> usize {
    track.voices.first().map_or(0, |v| {
        v.event_groups
            .iter()
            .flat_map(|g| &g.atoms)
            .filter(|a| matches!(a, AtomEvent::Note(_)))
            .count()
    })
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

/// Generates a fresh riff (S6) seeded from the source's musical material — its
/// pitch palette, the rhythm of its first sounding bar, and its meter, tempo,
/// and range — then writes the result to a MIDI file.
fn cmd_generate(input: &Path, output: &Path, seed: u64, bars: usize) -> Result<(), CliError> {
    let data = fs::read(input)?;
    let score = import::import_score_auto(&data)?;
    let request = generation_request_from_score(&score, seed, bars)?;
    let candidate = generate::generate(&request)?;
    let out_bytes = midi::export_score(&candidate.score)?;
    fs::write(output, &out_bytes)?;
    println!(
        "generated {bars} bars ({strategy:?}, seed {seed}) from a {tones}-tone scale \
         ({n} bytes) -> {out}",
        strategy = candidate.strategy,
        tones = request.pitch_material.intervals.len(),
        n = out_bytes.len(),
        out = output.display(),
    );
    Ok(())
}

/// Builds a tab-seeded [`generate::RuleGenerationRequest`]: the scale is the
/// source's distinct pitch classes, the rhythm template its first sounding bar,
/// and meter / tempo / range its transport.
fn generation_request_from_score(
    score: &Score,
    seed: u64,
    bars: usize,
) -> Result<generate::RuleGenerationRequest, CliError> {
    if bars == 0 {
        return Err(CliError::Generate(generate::GenerationError::BarCountZero));
    }
    let pitches = all_pitches(score);
    let (lo, hi) = pitch_range(&pitches)?;
    let first_bar = score.master_bars.first().ok_or(CliError::Generate(
        generate::GenerationError::InvalidConstraints,
    ))?;
    let constraints = generate::GenerationConstraints {
        bar_count: bars,
        time_signature: first_bar.time_signature,
        tempo: first_bar.tempo,
        ticks_per_quarter: Ticks(u32::from(score.ticks_per_quarter)),
        pitch_lo: lo,
        pitch_hi: hi,
    };
    Ok(generate::RuleGenerationRequest {
        seed: generate::GenerationSeed(seed),
        pitch_material: pitch_material_from(lo, &pitches),
        constraints,
        source_rhythms: vec![first_bar_rhythm(score)],
        strategy: generate::GenerationStrategy::RhythmCopyPitchSubstitute,
    })
}

/// Every note pitch across all tracks and voices, in track/voice order.
fn all_pitches(score: &Score) -> Vec<u8> {
    score
        .tracks
        .iter()
        .flat_map(|t| &t.voices)
        .flat_map(|v| &v.event_groups)
        .flat_map(|g| &g.atoms)
        .filter_map(|a| match a {
            AtomEvent::Note(n) => Some(n.pitch.0),
            AtomEvent::Rest(_) => None,
        })
        .collect()
}

/// The lowest and highest pitch present; errors (no pitch material) when the
/// source is silent.
fn pitch_range(pitches: &[u8]) -> Result<(Pitch, Pitch), CliError> {
    let lo = pitches.iter().min().copied().ok_or(CliError::Generate(
        generate::GenerationError::EmptyPitchMaterial,
    ))?;
    let hi = pitches.iter().max().copied().unwrap_or(lo);
    Ok((Pitch(lo), Pitch(hi)))
}

/// A scale rooted at `lo` whose intervals are the distinct semitone classes the
/// source uses, so the generated riff stays in the tab's pitch palette.
fn pitch_material_from(lo: Pitch, pitches: &[u8]) -> generate::PitchMaterial {
    let mut intervals: Vec<u8> = pitches
        .iter()
        .map(|&p| p.saturating_sub(lo.0).checked_rem(12).unwrap_or(0))
        .collect();
    intervals.sort_unstable();
    intervals.dedup();
    if intervals.is_empty() {
        intervals.push(0);
    }
    generate::PitchMaterial {
        root: lo,
        intervals,
    }
}

/// The note durations of the first *sounding* bar — the earliest master bar
/// holding any note across all tracks and voices — in onset order, as the
/// rhythm template the generator copies. Falls back to four quarter notes only
/// when the source is entirely silent.
fn first_bar_rhythm(score: &Score) -> Vec<Ticks> {
    for bar in &score.master_bars {
        let mut notes: Vec<(u32, Ticks)> = score
            .tracks
            .iter()
            .flat_map(|t| &t.voices)
            .flat_map(|v| &v.event_groups)
            .flat_map(|g| &g.atoms)
            .filter_map(|a| match a {
                AtomEvent::Note(n)
                    if n.absolute_start.0 >= bar.tick_range.start.0
                        && n.absolute_start.0 < bar.tick_range.end.0 =>
                {
                    Some((n.absolute_start.0, n.duration))
                }
                _ => None,
            })
            .collect();
        if !notes.is_empty() {
            notes.sort_by_key(|&(onset, _)| onset);
            return notes.into_iter().map(|(_, dur)| dur).collect();
        }
    }
    let quarter = Ticks(u32::from(score.ticks_per_quarter));
    vec![quarter; 4]
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

/// Builds one [`ChunkMeta`] per detected phrase of the first note-bearing track:
/// phrase boundaries cut the bars into segments, each segment is sliced into a
/// standalone score, measured, and stamped with its source `bar_range` (the
/// original bar indices it covers).
fn phrase_chunks(
    path: &Path,
    score: &Score,
    inputs: &CurateInputs,
) -> Result<Vec<ChunkMeta>, CliError> {
    let track = score
        .tracks
        .iter()
        .position(|t| primary_voice_note_count(t) > 0)
        .ok_or_else(|| {
            CliError::Split("split needs a track with notes in its primary voice".to_owned())
        })?;
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

    Ok(chunks_for_segments(path, score, inputs, track, &segments))
}

/// Builds one [`ChunkMeta`] per segment in which `track` sounds, renumbered
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
fn chunks_for_segments(
    path: &Path,
    score: &Score,
    inputs: &CurateInputs,
    track: usize,
    segments: &[Range<usize>],
) -> Vec<ChunkMeta> {
    segments
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
        .enumerate()
        .map(|(phrase, (start, end, sub))| {
            let id = format!("{}_p{phrase}", inputs.id);
            let title = format!("{} (phrase {phrase})", inputs.title);
            let mut meta = build_chunk_meta(&sub, path, Some(track), id, title, inputs, None);
            let last = end.saturating_sub(1);
            meta.source.bar_range =
                Some((u32::try_from(start).unwrap_or(0), u32::try_from(last).unwrap_or(0)));
            meta
        })
        .collect()
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

    for (phrase, meta) in chunks.iter().enumerate() {
        let (lo, hi) = meta.source.bar_range.unwrap_or((0, 0));
        println!("phrase {phrase} (bars {lo}..{hi}):");
        print_measurements(meta);
        let json = serde_json::to_string_pretty(meta).map_err(CliError::Json)?;
        write_output(
            &PathBuf::from(format!("{}.p{phrase}.chunk.json", stem.display())),
            &json,
        )?;
    }
    println!("split into {} phrase chunk(s)", chunks.len());
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
        tags: technique::merge_tags(&inputs.tags, &derived.tags),
        boundaries,
        techniques: derived.names,
        quality_flags: inputs.quality_flags.clone(),
        reviewer: inputs.reviewer,
        structure,
        gesture,
        complexity,
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
    Complement(complement::ComplementError),
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
            Self::Complement(e) => write!(f, "complement error: {e:?}"),
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

impl From<complement::ComplementError> for CliError {
    fn from(e: complement::ComplementError) -> Self {
        Self::Complement(e)
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
    use griff_core::corpus::SourceFormat;
    use griff_core::event::{NoteMarks, Pitch, Tempo, Ticks, TimeSignature, Tuning, Velocity};
    use griff_core::score::{
        AtomEvent, AtomNote, EventGroup, EventGroupKind, LossReport, MasterBar, RepeatMarker,
        Score, SourceMeta, Voice,
    };
    use griff_core::slice::TickRange;

    use super::{
        measure_group_relations, primary_voice_note_count, source_format, track_note_count, Track,
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
        assert!(meta.techniques.contains(&"hammer_on".to_owned()), "{:?}", meta.techniques);
        assert!(
            meta.techniques.contains(&"pinch_harmonic".to_owned()),
            "{:?}",
            meta.techniques
        );
        // …and the curator's hand-picked tag survives alongside the derived ones.
        assert!(meta.tags.contains(&SwancoreTag::Intro), "{:?}", meta.tags);
        assert!(meta.tags.contains(&SwancoreTag::HammerOn), "{:?}", meta.tags);
        assert!(meta.tags.contains(&SwancoreTag::ArtificialHarmonic), "{:?}", meta.tags);
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
    fn split_inputs() -> super::CurateInputs {
        use griff_core::corpus::{Acquisition, QualityFlag, RightsInfo, RightsStatus, StyleCohort};
        super::CurateInputs {
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
            vec![quarter(0, 60), quarter(1920, 62), quarter(3840, 64), quarter(5760, 65)],
        );
        let score = Score {
            ticks_per_quarter: 480,
            master_bars: split_master_bars(),
            tracks: vec![track_of(vec![voice])],
            source_meta: None,
            loss: LossReport::new(),
        };

        let chunks =
            phrase_chunks(Path::new("riff.gp5"), &score, &split_inputs()).expect("splits");
        assert!(!chunks.is_empty(), "at least one phrase chunk");

        // Whatever the detector decides, the chunks tile the four bars with
        // inclusive `[first, last]` ranges, each id suffixed by its phrase index.
        let mut next = 0_u32;
        for (i, chunk) in chunks.iter().enumerate() {
            let (lo, hi) = chunk.source.bar_range.expect("bar_range set");
            assert_eq!(lo, next, "first bar follows the previous chunk's last + 1");
            assert!(hi >= lo, "inclusive last bar is at least the first");
            assert_eq!(chunk.id.0, format!("dgd_p{i}"));
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
        let chunks =
            chunks_for_segments(Path::new("riff.gp5"), &score, &split_inputs(), 0, &[0..2, 2..4]);
        assert_eq!(chunks.len(), 1, "the silent [2,4) segment is dropped");
        assert_eq!(
            chunks[0].source.bar_range,
            Some((0, 1)),
            "inclusive last bar is end-1, not the half-open end"
        );
        assert_eq!(chunks[0].id.0, "dgd_p0", "kept chunks renumber from 0");
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
        let other = track_of(vec![voice_of(0, vec![quarter(3840, 48), quarter(5760, 50)])]);
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
            chunks[0].source.bar_range,
            Some((0, 1)),
            "the kept chunk is the detected track's sounding bars 0–1"
        );
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
}
