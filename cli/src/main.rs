use std::{
    fmt, fs,
    io::{self, Error as IoError, Write as IoWrite},
    path::{Path, PathBuf},
    process::ExitCode,
};

use clap::{Parser, Subcommand};
use griff_core::{
    classify::{self, BarClass},
    corpus::{
        ChunkId, ChunkMeta, QualityFlag, ReviewerDecision, SourceFormat, SourceRef, SwancoreTag,
    },
    event::{NoteMarks, NotePosition, TechniqueSource},
    gesture,
    midi::{self, MidiError},
    score::{AtomEvent, Score, Track, Voice},
    slice::TickRange,
    structure,
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
    /// Parse a MIDI file and print a one-line summary per track.
    Import {
        /// Path to the `.mid` file.
        #[arg(value_name = "FILE")]
        path: PathBuf,
    },

    /// Print a detailed bar-by-bar inspection of a MIDI file.
    Inspect {
        /// Path to the `.mid` file.
        #[arg(value_name = "FILE")]
        path: PathBuf,
    },

    /// Import a MIDI file and write it back out (roundtrip check).
    Export {
        /// Input `.mid` file.
        #[arg(value_name = "INPUT")]
        input: PathBuf,
        /// Output `.mid` file.
        #[arg(value_name = "OUTPUT")]
        output: PathBuf,
    },

    /// Classify each bar of a MIDI file as Riff, Solo, Breakdown, Clean, or Unknown.
    Classify {
        /// Path to the `.mid` file.
        #[arg(value_name = "FILE")]
        path: PathBuf,
    },

    /// Interactively curate a MIDI file into a corpus `ChunkMeta` JSON record.
    Curate {
        /// Path to the `.mid` file to curate.
        #[arg(value_name = "FILE")]
        path: PathBuf,
        /// Output path for the `ChunkMeta` JSON (default: `<file>.chunk.json`).
        #[arg(short, long, value_name = "OUTPUT")]
        output: Option<PathBuf>,
    },
}

fn run() -> Result<(), CliError> {
    let cli = Cli::parse();
    match cli.command {
        Command::Import { path } => cmd_import(&path),
        Command::Inspect { path } => cmd_inspect(&path),
        Command::Export { input, output } => cmd_export(&input, &output),
        Command::Classify { path } => cmd_classify(&path),
        Command::Curate { path, output } => cmd_curate(&path, output.as_deref()),
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
    let score = midi::import_score(&data)?;

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

fn cmd_inspect(path: &Path) -> Result<(), CliError> {
    let data = fs::read(path)?;
    let score = midi::import_score(&data)?;

    println!("PPQN: {}", score.ticks_per_quarter);
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
        for mb in &score.master_bars {
            let notes = voice.map_or(0, |v| note_count_in_range(v, mb.tick_range));
            println!(
                "  Bar {bi:4}  {num}/{den}  {bpm:.1} BPM  {notes} notes",
                bi = mb.index,
                num = mb.time_signature.numerator,
                den = mb.time_signature.denominator,
                bpm = mb.tempo.0,
            );
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
    let score = midi::import_score(&data)?;
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
    let score = midi::import_score(&data)?;

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

fn cmd_curate(path: &Path, output: Option<&Path>) -> Result<(), CliError> {
    let data = fs::read(path)?;
    let score = midi::import_score(&data)?;

    print_score_summary(path, &score);

    let inputs = gather_curate_inputs()?;

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

    // S14 Phase 3 / schema v3: measure the structure and gesture stats of the
    // first note-bearing track and persist them with the record.
    let measured_track = score.tracks.iter().position(|t| track_note_count(t) > 0);
    let structure = measured_track.and_then(|idx| structure::measure_structure(&score, idx).ok());
    if let Some(m) = &structure {
        let period = m
            .detected_pattern_period_bars
            .map_or_else(|| "through-composed".to_owned(), |p| format!("{p} bar(s)"));
        println!(
            "Structure: period={period}  repeatability={rep:.2}  loopability={lp:.2}",
            rep = m.repeatability_score,
            lp = m.loopability_score,
        );
    }
    let gesture = measured_track.and_then(|idx| gesture::measure_gesture(&score, idx).ok());
    if let Some(g) = &gesture {
        println!(
            "Gesture: bursts={bursts} (mean {mean:.1} notes)  rests={rests} (on-grid {grid:.2})",
            bursts = g.burst_count,
            mean = g.mean_burst_notes,
            rests = g.rest_count,
            grid = g.rest_on_grid_share,
        );
    }

    let now = "2026-05-20T00:00:00Z".to_owned();
    let meta = ChunkMeta {
        id: ChunkId(inputs.id),
        title: inputs.title,
        source: SourceRef {
            filename,
            format: SourceFormat::Midi,
            bar_range: None,
        },
        tempo_bpm,
        ticks_per_quarter: score.ticks_per_quarter,
        time_signature,
        tuning: inputs.tuning,
        tags: inputs.tags,
        boundaries: Vec::new(),
        techniques: Vec::new(),
        quality_flags: inputs.quality_flags,
        reviewer: inputs.reviewer,
        structure,
        gesture,
        created_at: now.clone(),
        updated_at: now,
    };

    let out_path = output.map_or_else(|| path.with_extension("chunk.json"), PathBuf::from);
    let json = serde_json::to_string_pretty(&meta).map_err(CliError::Json)?;
    fs::write(&out_path, json)?;
    println!("wrote {}", out_path.display());
    Ok(())
}

struct CurateInputs {
    id: String,
    title: String,
    tuning: String,
    tags: Vec<SwancoreTag>,
    quality_flags: Vec<QualityFlag>,
    reviewer: Option<ReviewerDecision>,
}

fn gather_curate_inputs() -> Result<CurateInputs, CliError> {
    let mut input_buf = String::new();

    let id = prompt_line(&mut input_buf, "Chunk ID (e.g. dgd_001)")?;
    let title = prompt_line(&mut input_buf, "Title")?;
    let tuning_raw = prompt_line(&mut input_buf, "Tuning [standard_e]")?;
    let tuning = if tuning_raw.trim().is_empty() {
        "standard_e".to_owned()
    } else {
        tuning_raw.trim().to_owned()
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
    Midi(MidiError),
    Json(serde_json::Error),
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "I/O error: {e}"),
            Self::Midi(e) => write!(f, "MIDI error: {e}"),
            Self::Json(e) => write!(f, "JSON error: {e}"),
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
