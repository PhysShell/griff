use std::{
    fmt, fs,
    io::Error as IoError,
    path::{Path, PathBuf},
    process::ExitCode,
};

use clap::{Parser, Subcommand};
use griff_core::{
    classify,
    event::Event,
    midi::{self, MidiError},
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
}

fn run() -> Result<(), CliError> {
    let cli = Cli::parse();
    match cli.command {
        Command::Import { path } => cmd_import(&path),
        Command::Inspect { path } => cmd_inspect(&path),
        Command::Export { input, output } => cmd_export(&input, &output),
        Command::Classify { path } => cmd_classify(&path),
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

fn cmd_import(path: &Path) -> Result<(), CliError> {
    let data = fs::read(path)?;
    let song = midi::import(&data)?;
    let summary = midi::summarise(&song);

    println!("PPQN: {}", summary.ppqn);
    println!("Tracks: {}", summary.tracks.len());
    for t in &summary.tracks {
        let name = t.name.as_deref().unwrap_or("<unnamed>");
        println!(
            "  [{idx}] ch={ch:02}  bars={bars:4}  notes={notes:5}  \"{name}\"",
            idx = t.index,
            ch = t.channel,
            bars = t.bar_count,
            notes = t.note_count,
        );
    }
    Ok(())
}

fn cmd_inspect(path: &Path) -> Result<(), CliError> {
    let data = fs::read(path)?;
    let song = midi::import(&data)?;

    println!("PPQN: {}", song.ppqn.0);
    for (ti, track) in song.tracks.iter().enumerate() {
        let name = track.name.as_deref().unwrap_or("<unnamed>");
        println!("Track {ti} ch={ch} \"{name}\":", ch = track.channel);
        for (bi, bar) in track.phrase.bars.iter().enumerate() {
            let note_count = bar
                .events
                .iter()
                .filter(|e| matches!(e, Event::Note(_)))
                .count();
            println!(
                "  Bar {bi:4}  {num}/{den}  {bpm:.1} BPM  {notes} notes",
                num = bar.time_signature.numerator,
                den = bar.time_signature.denominator,
                bpm = bar.tempo.0,
                notes = note_count,
            );
        }
    }
    Ok(())
}

fn cmd_export(input: &Path, output: &Path) -> Result<(), CliError> {
    let data = fs::read(input)?;
    let song = midi::import(&data)?;
    let out_bytes = midi::export(&song)?;
    fs::write(output, &out_bytes)?;
    println!(
        "exported {} tracks ({} bytes) -> {}",
        song.tracks.len(),
        out_bytes.len(),
        output.display(),
    );
    Ok(())
}

fn cmd_classify(path: &Path) -> Result<(), CliError> {
    let data = fs::read(path)?;
    let song = midi::import(&data)?;

    println!("PPQN: {}", song.ppqn.0);
    for (ti, track) in song.tracks.iter().enumerate() {
        let name = track.name.as_deref().unwrap_or("<unnamed>");
        println!(
            "Track {ti} ch={ch:02} \"{name}\" — {} bars",
            track.phrase.bars.len(),
            ch = track.channel,
        );
        for (bi, bar) in track.phrase.bars.iter().enumerate() {
            let feat = classify::bar_features(bar);
            let class = classify::classify_bar(feat);
            println!(
                "  Bar {bi:4}  {num}/{den}  {bpm:.1} BPM  \
                 notes={notes:3}  class={class:<10}  vel={vel:3}  span={span:2}st",
                num = bar.time_signature.numerator,
                den = bar.time_signature.denominator,
                bpm = bar.tempo.0,
                notes = feat.note_count,
                vel = feat.avg_velocity,
                span = feat.pitch_span,
            );
        }
    }
    Ok(())
}

// ── error ─────────────────────────────────────────────────────────────────────

#[derive(Debug)]
enum CliError {
    Io(IoError),
    Midi(MidiError),
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "I/O error: {e}"),
            Self::Midi(e) => write!(f, "MIDI error: {e}"),
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
