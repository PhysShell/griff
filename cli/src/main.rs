use std::{
    fmt, fs,
    io::{self, Error as IoError, Write as IoWrite},
    path::{Path, PathBuf},
    process::ExitCode,
};

use clap::{Parser, Subcommand};
use griff_core::{
    boundary,
    classify::{self, BarClass},
    complement,
    corpus::{
        ChunkId, ChunkMeta, EnsembleGroup, EnsembleRef, PairRelation, QualityFlag,
        ReviewerDecision, SourceFormat, SourceRef, StyleCohort, SwancoreTag,
    },
    event::{NoteMarks, NotePosition, TechniqueSource, Ticks},
    gesture,
    import::{self, ImportError},
    midi::{self, MidiError},
    score::{AtomEvent, Score, Track, Voice},
    slice::TickRange,
    structure, unfold,
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

    /// Detect phrase boundaries per track (S4): where one musical phrase ends
    /// and the next begins, with the heuristic signals that fired.
    Phrases {
        /// Path to the MIDI (`.mid`) or Guitar Pro (`.gp3`/`.gp4`/`.gp5`/`.gpx`) file.
        #[arg(value_name = "FILE")]
        path: PathBuf,
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
}

fn run() -> Result<(), CliError> {
    let cli = Cli::parse();
    match cli.command {
        Command::Import { path } => cmd_import(&path),
        Command::Inspect { path, unfold } => cmd_inspect(&path, unfold),
        Command::Export { input, output } => cmd_export(&input, &output),
        Command::Classify { path } => cmd_classify(&path),
        Command::Phrases { path } => cmd_phrases(&path),
        Command::Curate {
            path,
            output,
            ensemble,
        } => cmd_curate(&path, output.as_deref(), ensemble),
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
        tags: inputs.tags.clone(),
        boundaries: Vec::new(),
        techniques: Vec::new(),
        quality_flags: inputs.quality_flags.clone(),
        reviewer: inputs.reviewer,
        structure,
        gesture,
        complexity,
        style_cohort: Some(inputs.style_cohort),
        ensemble,
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

struct CurateInputs {
    id: String,
    title: String,
    tuning: String,
    style_cohort: StyleCohort,
    tags: Vec<SwancoreTag>,
    quality_flags: Vec<QualityFlag>,
    reviewer: Option<ReviewerDecision>,
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

    Ok(CurateInputs {
        id,
        title,
        tuning,
        style_cohort,
        tags,
        quality_flags,
        reviewer,
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
    Ensemble(String),
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "I/O error: {e}"),
            Self::Import(e) => write!(f, "import error: {e}"),
            Self::Midi(e) => write!(f, "MIDI error: {e}"),
            Self::Json(e) => write!(f, "JSON error: {e}"),
            Self::Ensemble(msg) => write!(f, "ensemble error: {msg}"),
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
