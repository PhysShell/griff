//! General Guitar Pro chord-event representation census (Lab only).

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use griff_constraint_lab::chord_event::{
    analyze_regimes, chord_event_census, rotate_anchors_within_song, technique_string_controls,
    AssignmentCount, ChordAssignment, ChordCensusEvent, ChordCensusStatus, ChordEventIdentity,
    ExactPreferredSet, HandAnchor, HumanSetMetrics, ObservedChordVoicing,
};
use griff_constraint_lab::fingering::song_key;
use griff_core::fretboard::STANDARD_MAX_FRET;
use griff_core::gp::import_gp_score;
use griff_core::ingest::select_ingest_tracks;
use serde::Serialize;

type DynError = Box<dyn std::error::Error>;
type LoadedCorpus = (Vec<ChordCensusEvent>, CorpusFacts);

#[derive(Serialize)]
struct CorpusFacts {
    files: usize,
    import_failures: usize,
    guitar_tracks: usize,
    fingerprint: String,
}

#[derive(Default, Serialize)]
struct StatusCounts {
    complete_explicit: u64,
    incomplete_position: u64,
    pitch_mismatch: u64,
    duplicate_explicit_string: u64,
    beyond_max_fret: u64,
    other_unsupported: u64,
}

impl StatusCounts {
    fn add(&mut self, status: ChordCensusStatus) {
        let slot = match status {
            ChordCensusStatus::CompleteExplicit => &mut self.complete_explicit,
            ChordCensusStatus::IncompletePosition => &mut self.incomplete_position,
            ChordCensusStatus::PitchMismatch => &mut self.pitch_mismatch,
            ChordCensusStatus::DuplicateExplicitString => &mut self.duplicate_explicit_string,
            ChordCensusStatus::BeyondMaxFret => &mut self.beyond_max_fret,
            ChordCensusStatus::OtherUnsupported => &mut self.other_unsupported,
        };
        *slot = slot.saturating_add(1);
    }
}

#[derive(Serialize)]
struct CensusRecord<'a> {
    source: &'a str,
    song_key: &'a str,
    track: usize,
    voice: u8,
    onset: u32,
    atom_count: usize,
    status: &'static str,
}

#[derive(Serialize)]
struct CountRecord {
    value: u64,
    saturated: bool,
    ln: f64,
}

impl From<AssignmentCount> for CountRecord {
    fn from(value: AssignmentCount) -> Self {
        Self {
            value: value.value,
            saturated: value.saturated,
            ln: value.ln,
        }
    }
}

#[derive(Serialize)]
struct HumanRecord {
    exact_membership: bool,
    floor: f64,
    uniform: f64,
    chosen: f64,
    ceiling: f64,
}

impl From<&HumanSetMetrics> for HumanRecord {
    fn from(value: &HumanSetMetrics) -> Self {
        Self {
            exact_membership: value.exact_membership,
            floor: value.floor,
            uniform: value.uniform,
            chosen: value.chosen,
            ceiling: value.ceiling,
        }
    }
}

#[derive(Serialize)]
struct PreferredRecord {
    optimum: i64,
    optimum_count: CountRecord,
    chosen: Vec<PositionRecord>,
    human: Option<HumanRecord>,
}

fn preferred_record(value: &ExactPreferredSet) -> PreferredRecord {
    PreferredRecord {
        optimum: value.optimum,
        optimum_count: value.optimum_count.into(),
        chosen: value
            .assignments
            .first()
            .map_or_else(Vec::new, assignment_record),
        human: value.human.as_ref().map(HumanRecord::from),
    }
}

#[derive(Serialize)]
struct PositionRecord {
    atom_id: usize,
    string: u8,
    fret: u8,
}

fn assignment_record(assignment: &ChordAssignment) -> Vec<PositionRecord> {
    assignment
        .positions
        .iter()
        .map(|position| PositionRecord {
            atom_id: position.atom_id,
            string: position.position.string,
            fret: position.position.fret,
        })
        .collect()
}

#[derive(Serialize)]
struct TechniqueControlRecord {
    target_atom_id: usize,
    observed_string: u8,
    strings: Vec<TechniqueStringRecord>,
}

#[derive(Serialize)]
struct TechniqueStringRecord {
    string: u8,
    admissible_count: CountRecord,
    b0: Option<PreferredRecord>,
    anchor: Option<PreferredRecord>,
}

#[derive(Serialize)]
struct EventRecord {
    source: String,
    song_key: String,
    gp_family: &'static str,
    track: usize,
    voice: u8,
    onset: u32,
    atom_count: usize,
    status: &'static str,
    has_anchor: bool,
    incoming_relations: usize,
    r0_count: CountRecord,
    r0_b0: Option<PreferredRecord>,
    r0_human: Option<HumanRecord>,
    r1_anchor: Option<PreferredRecord>,
    r2_count: CountRecord,
    r2_b0: Option<PreferredRecord>,
    r2_human: Option<HumanRecord>,
    r2_conflict: bool,
    r3_anchor: Option<PreferredRecord>,
    technique_controls: Vec<TechniqueControlRecord>,
    candidate_product: u64,
    elapsed_micros: u128,
}

#[derive(Clone)]
struct Effect {
    song: String,
    anchor_uniform: Option<f64>,
    anchor_membership: Option<i8>,
    combined_uniform: Option<f64>,
    combined_membership: Option<i8>,
    technique_fractional_reduction: Option<f64>,
    true_minus_rotated_uniform: Option<f64>,
    true_minus_rotated_membership: Option<i8>,
}

#[derive(Default, Serialize)]
struct Coverage {
    anchor: u64,
    technique: u64,
    both: u64,
}

#[derive(Serialize)]
struct RuntimeSummary {
    total_millis: u128,
    max_candidate_product: u64,
    max_admissible_assignments: u64,
    worst_event: Option<String>,
    worst_event_micros: u128,
}

#[derive(Serialize)]
struct Summary {
    schema: &'static str,
    corpus: CorpusFacts,
    statuses: StatusCounts,
    coverage: Coverage,
    complete_by_chord_size: BTreeMap<String, u64>,
    complete_by_gp_family: BTreeMap<String, u64>,
    r0_human_membership: u64,
    r0_b0_human_membership: u64,
    r1_human_membership: u64,
    r2_human_feasible: u64,
    r2_human_membership: u64,
    r3_human_membership: u64,
    technique_conflicts: u64,
    runtime: RuntimeSummary,
}

#[derive(Serialize)]
struct SongSummary {
    song_key: String,
    cases: usize,
    anchor_cases: usize,
    anchor_uniform_delta: f64,
    anchor_membership_delta: i64,
    combined_cases: usize,
    combined_uniform_delta: f64,
    combined_membership_delta: i64,
    technique_cases: usize,
    mean_technique_fractional_reduction: f64,
    control_cases: usize,
    true_minus_rotated_uniform: f64,
    true_minus_rotated_membership: i64,
}

#[derive(Serialize)]
struct LooRecord {
    omitted_song: String,
    anchor_uniform_delta: f64,
    anchor_membership_delta: i64,
    combined_uniform_delta: f64,
    combined_membership_delta: i64,
    true_minus_rotated_uniform: f64,
    true_minus_rotated_membership: i64,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), DynError> {
    let (tabs, out, legacy_summary) = parse_args()?;
    fs::create_dir_all(&out)?;
    verify_legacy(&legacy_summary, &out)?;
    let started = Instant::now();
    let (events, corpus) = load_events(&tabs)?;
    let anchors: Vec<_> = events
        .iter()
        .filter_map(|event| {
            event.problem.as_ref().and_then(|problem| {
                problem
                    .preceding_hand()
                    .map(|anchor| (event.identity.clone(), anchor))
            })
        })
        .collect();
    let rotated: BTreeMap<ChordEventIdentity, HandAnchor> =
        rotate_anchors_within_song(&anchors).into_iter().collect();
    let mut writer = BufWriter::new(File::create(out.join("chord-event-results.jsonl"))?);
    let mut census_writer = BufWriter::new(File::create(out.join("chord-event-census.jsonl"))?);
    let mut statuses = StatusCounts::default();
    let mut coverage = Coverage::default();
    let mut effects = Vec::new();
    let mut size_counts = BTreeMap::new();
    let mut family_counts = BTreeMap::new();
    let mut membership = [0_u64; 6];
    let mut conflicts = 0_u64;
    let mut max_product = 0_u64;
    let mut max_assignments = 0_u64;
    let mut worst = None;
    let mut worst_time = Duration::ZERO;
    for event in &events {
        statuses.add(event.status);
        serde_json::to_writer(
            &mut census_writer,
            &CensusRecord {
                source: &event.identity.source,
                song_key: &event.identity.song_key,
                track: event.identity.track,
                voice: event.identity.voice,
                onset: event.identity.onset,
                atom_count: event.atom_count,
                status: status_name(event.status),
            },
        )?;
        census_writer.write_all(b"\n")?;
        if event.status != ChordCensusStatus::CompleteExplicit {
            continue;
        }
        let problem = event
            .problem
            .as_ref()
            .ok_or("complete event has no problem")?;
        let observed = event
            .observed
            .as_ref()
            .ok_or("complete event has no observation")?;
        let event_started = Instant::now();
        let analysis = analyze_regimes(problem, Some(observed))?;
        let elapsed = event_started.elapsed();
        if elapsed > worst_time {
            worst_time = elapsed;
            worst = Some(identity_text(&event.identity));
        }
        max_assignments = max_assignments.max(analysis.r0.admissible_count.value);
        let product = candidate_product(problem);
        max_product = max_product.max(product);
        let has_anchor = problem.preceding_hand().is_some();
        let has_technique = !problem.incoming_techniques().is_empty();
        coverage.anchor = coverage.anchor.saturating_add(u64::from(has_anchor));
        coverage.technique = coverage.technique.saturating_add(u64::from(has_technique));
        coverage.both = coverage
            .both
            .saturating_add(u64::from(has_anchor && has_technique));
        *size_counts
            .entry(if event.atom_count >= 5 {
                "5+".into()
            } else {
                event.atom_count.to_string()
            })
            .or_insert(0_u64) += 1;
        let family = gp_family(&event.identity.source).to_owned();
        *family_counts.entry(family).or_insert(0_u64) += 1;
        membership[0] += member(analysis.r0.human.as_ref());
        membership[1] += analysis
            .r0
            .b0
            .as_ref()
            .and_then(|set| set.human.as_ref())
            .map_or(0, |human| u64::from(human.exact_membership));
        membership[2] += analysis
            .r1
            .as_ref()
            .and_then(|set| set.human.as_ref())
            .map_or(0, |human| u64::from(human.exact_membership));
        membership[3] += member(analysis.r2.human.as_ref());
        membership[4] += member(analysis.r2.human.as_ref());
        membership[5] += analysis
            .r3
            .as_ref()
            .and_then(|set| set.human.as_ref())
            .map_or(0, |human| u64::from(human.exact_membership));
        conflicts += u64::from(analysis.r2.conflict.is_some());
        let rotated_analysis = rotated.get(&event.identity).and_then(|anchor| {
            analyze_regimes(&problem.with_anchor(Some(*anchor)), Some(observed)).ok()
        });
        effects.push(effect(event, &analysis, rotated_analysis.as_ref()));
        let technique_controls = technique_controls(problem, observed)?;
        let record = EventRecord {
            source: event.identity.source.clone(),
            song_key: event.identity.song_key.clone(),
            gp_family: gp_family(&event.identity.source),
            track: event.identity.track,
            voice: event.identity.voice,
            onset: event.identity.onset,
            atom_count: event.atom_count,
            status: "complete_explicit",
            has_anchor,
            incoming_relations: problem.incoming_techniques().len(),
            r0_count: analysis.r0.admissible_count.into(),
            r0_b0: analysis.r0.b0.as_ref().map(preferred_record),
            r0_human: analysis.r0.human.as_ref().map(HumanRecord::from),
            r1_anchor: analysis.r1.as_ref().map(preferred_record),
            r2_count: analysis.r2.admissible_count.into(),
            r2_b0: analysis.r2.b0.as_ref().map(preferred_record),
            r2_human: analysis.r2.human.as_ref().map(HumanRecord::from),
            r2_conflict: analysis.r2.conflict.is_some(),
            r3_anchor: analysis.r3.as_ref().map(preferred_record),
            technique_controls,
            candidate_product: product,
            elapsed_micros: elapsed.as_micros(),
        };
        serde_json::to_writer(&mut writer, &record)?;
        writer.write_all(b"\n")?;
    }
    writer.flush()?;
    census_writer.flush()?;
    let runtime = RuntimeSummary {
        total_millis: started.elapsed().as_millis(),
        max_candidate_product: max_product,
        max_admissible_assignments: max_assignments,
        worst_event: worst,
        worst_event_micros: worst_time.as_micros(),
    };
    let summary = Summary {
        schema: "griff.constraint-lab-chord-event-representation.v1",
        corpus,
        statuses,
        coverage,
        complete_by_chord_size: size_counts,
        complete_by_gp_family: family_counts,
        r0_human_membership: membership[0],
        r0_b0_human_membership: membership[1],
        r1_human_membership: membership[2],
        r2_human_feasible: membership[3],
        r2_human_membership: membership[4],
        r3_human_membership: membership[5],
        technique_conflicts: conflicts,
        runtime,
    };
    write_json(&out.join("chord-event-census.json"), &summary)?;
    let song = song_summaries(&effects);
    write_json(&out.join("chord-event-song-summary.json"), &song)?;
    write_json(&out.join("chord-event-loo.json"), &loo(&effects))?;
    write_json(
        &out.join("chord-event-anchor-control.json"),
        &song
            .iter()
            .map(|row| {
                (
                    &row.song_key,
                    row.control_cases,
                    row.true_minus_rotated_uniform,
                    row.true_minus_rotated_membership,
                )
            })
            .collect::<Vec<_>>(),
    )?;
    write_json(
        &out.join("chord-event-technique-control.json"),
        &song
            .iter()
            .map(|row| {
                (
                    &row.song_key,
                    row.technique_cases,
                    row.mean_technique_fractional_reduction,
                )
            })
            .collect::<Vec<_>>(),
    )?;
    println!("{}", serde_json::to_string_pretty(&summary)?);
    Ok(())
}

fn load_events(tabs: &Path) -> Result<LoadedCorpus, DynError> {
    let mut paths: Vec<PathBuf> = fs::read_dir(tabs)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .collect();
    paths.sort();
    let mut events = Vec::new();
    let mut failures = 0;
    let mut tracks = 0;
    let mut hashes = Vec::new();
    for path in &paths {
        let bytes = fs::read(path)?;
        hashes.extend_from_slice(&fnv1a64(&bytes).to_le_bytes());
        let name = path
            .file_name()
            .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
        let key = song_key(&name);
        let Ok(score) = import_gp_score(&bytes) else {
            failures += 1;
            continue;
        };
        for track in select_ingest_tracks(&score, false) {
            tracks += 1;
            events.extend(chord_event_census(
                &score,
                &name,
                &key,
                track,
                STANDARD_MAX_FRET,
            )?);
        }
    }
    events.sort_by(|left, right| left.identity.cmp(&right.identity));
    let facts = CorpusFacts {
        files: paths.len(),
        import_failures: failures,
        guitar_tracks: tracks,
        fingerprint: format!("{:016x}", fnv1a64(&hashes)),
    };
    Ok((events, facts))
}

fn technique_controls(
    problem: &griff_constraint_lab::chord_event::ChordEventProblem,
    observed: &ObservedChordVoicing,
) -> Result<Vec<TechniqueControlRecord>, DynError> {
    let mut targets: BTreeMap<usize, u8> = BTreeMap::new();
    for incoming in problem.incoming_techniques() {
        targets.insert(incoming.target_atom_id, incoming.origin_position.string);
    }
    targets
        .into_iter()
        .map(|(target_atom_id, observed_string)| {
            let strings = technique_string_controls(problem, target_atom_id, Some(observed))?
                .into_iter()
                .map(|entry| TechniqueStringRecord {
                    string: entry.string,
                    admissible_count: entry.admissible_count.into(),
                    b0: entry.b0.as_ref().map(preferred_record),
                    anchor: entry.anchor.as_ref().map(preferred_record),
                })
                .collect();
            Ok(TechniqueControlRecord {
                target_atom_id,
                observed_string,
                strings,
            })
        })
        .collect()
}

#[allow(clippy::cast_precision_loss)] // descriptive ratio; exact counts retained
fn effect(
    event: &ChordCensusEvent,
    analysis: &griff_constraint_lab::chord_event::ChordRegimeAnalysis,
    rotated: Option<&griff_constraint_lab::chord_event::ChordRegimeAnalysis>,
) -> Effect {
    let r0 = analysis.r0.b0.as_ref().and_then(|set| set.human.as_ref());
    let r1 = analysis.r1.as_ref().and_then(|set| set.human.as_ref());
    let r2 = analysis.r2.b0.as_ref().and_then(|set| set.human.as_ref());
    let r3 = analysis.r3.as_ref().and_then(|set| set.human.as_ref());
    let rotated_r1 = rotated
        .and_then(|value| value.r1.as_ref())
        .and_then(|set| set.human.as_ref());
    let count0 = analysis.r0.admissible_count.value as f64;
    let count2 = analysis.r2.admissible_count.value as f64;
    Effect {
        song: event.identity.song_key.clone(),
        anchor_uniform: r0
            .zip(r1)
            .map(|(base, context)| context.uniform - base.uniform),
        anchor_membership: r0.zip(r1).map(|(base, context)| {
            i8::from(context.exact_membership) - i8::from(base.exact_membership)
        }),
        combined_uniform: r2
            .zip(r3)
            .map(|(base, context)| context.uniform - base.uniform),
        combined_membership: r2.zip(r3).map(|(base, context)| {
            i8::from(context.exact_membership) - i8::from(base.exact_membership)
        }),
        technique_fractional_reduction: (!event
            .problem
            .as_ref()
            .is_some_and(|problem| problem.incoming_techniques().is_empty())
            && count0 > 0.0)
            .then(|| (count0 - count2) / count0),
        true_minus_rotated_uniform: r1
            .zip(rotated_r1)
            .map(|(true_anchor, wrong)| true_anchor.uniform - wrong.uniform),
        true_minus_rotated_membership: r1.zip(rotated_r1).map(|(true_anchor, wrong)| {
            i8::from(true_anchor.exact_membership) - i8::from(wrong.exact_membership)
        }),
    }
}

fn song_summaries(effects: &[Effect]) -> Vec<SongSummary> {
    let songs: BTreeSet<_> = effects.iter().map(|effect| effect.song.clone()).collect();
    songs
        .into_iter()
        .map(|song| summarize_song(&song, effects.iter().filter(|effect| effect.song == song)))
        .collect()
}

fn summarize_song<'a>(song: &str, rows: impl Iterator<Item = &'a Effect>) -> SongSummary {
    let rows: Vec<_> = rows.collect();
    let anchor: Vec<_> = rows
        .iter()
        .filter_map(|row| row.anchor_uniform.zip(row.anchor_membership))
        .collect();
    let combined: Vec<_> = rows
        .iter()
        .filter_map(|row| row.combined_uniform.zip(row.combined_membership))
        .collect();
    let technique: Vec<_> = rows
        .iter()
        .filter_map(|row| row.technique_fractional_reduction)
        .collect();
    let control: Vec<_> = rows
        .iter()
        .filter_map(|row| {
            row.true_minus_rotated_uniform
                .zip(row.true_minus_rotated_membership)
        })
        .collect();
    SongSummary {
        song_key: song.to_owned(),
        cases: rows.len(),
        anchor_cases: anchor.len(),
        anchor_uniform_delta: anchor.iter().map(|row| row.0).sum(),
        anchor_membership_delta: anchor.iter().map(|row| i64::from(row.1)).sum(),
        combined_cases: combined.len(),
        combined_uniform_delta: combined.iter().map(|row| row.0).sum(),
        combined_membership_delta: combined.iter().map(|row| i64::from(row.1)).sum(),
        technique_cases: technique.len(),
        mean_technique_fractional_reduction: mean(&technique),
        control_cases: control.len(),
        true_minus_rotated_uniform: control.iter().map(|row| row.0).sum(),
        true_minus_rotated_membership: control.iter().map(|row| i64::from(row.1)).sum(),
    }
}

fn loo(effects: &[Effect]) -> Vec<LooRecord> {
    let songs: BTreeSet<_> = effects.iter().map(|effect| effect.song.clone()).collect();
    songs
        .into_iter()
        .map(|omitted_song| {
            let row = summarize_song(
                "all",
                effects.iter().filter(|effect| effect.song != omitted_song),
            );
            LooRecord {
                omitted_song,
                anchor_uniform_delta: row.anchor_uniform_delta,
                anchor_membership_delta: row.anchor_membership_delta,
                combined_uniform_delta: row.combined_uniform_delta,
                combined_membership_delta: row.combined_membership_delta,
                true_minus_rotated_uniform: row.true_minus_rotated_uniform,
                true_minus_rotated_membership: row.true_minus_rotated_membership,
            }
        })
        .collect()
}

fn candidate_product(problem: &griff_constraint_lab::chord_event::ChordEventProblem) -> u64 {
    problem
        .atoms()
        .iter()
        .map(|atom| {
            problem
                .tuning()
                .candidates(atom.pitch, problem.max_fret())
                .len() as u64
        })
        .fold(1, u64::saturating_mul)
}

fn member(metrics: Option<&HumanSetMetrics>) -> u64 {
    metrics.map_or(0, |metrics| u64::from(metrics.exact_membership))
}

fn identity_text(identity: &ChordEventIdentity) -> String {
    format!(
        "{}:t{}.v{}.at{}",
        identity.source, identity.track, identity.voice, identity.onset
    )
}

fn gp_family(source: &str) -> &'static str {
    match Path::new(source)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("gp" | "gpx") => "gp6-7",
        _ => "gp3-5",
    }
}

fn status_name(status: ChordCensusStatus) -> &'static str {
    match status {
        ChordCensusStatus::CompleteExplicit => "complete_explicit",
        ChordCensusStatus::IncompletePosition => "incomplete_position",
        ChordCensusStatus::PitchMismatch => "pitch_mismatch",
        ChordCensusStatus::DuplicateExplicitString => "duplicate_explicit_string",
        ChordCensusStatus::BeyondMaxFret => "beyond_max_fret",
        ChordCensusStatus::OtherUnsupported => "other_unsupported",
    }
}

#[allow(clippy::cast_precision_loss)] // descriptive mean over exact case values
fn mean(values: &[f64]) -> f64 {
    if values.is_empty() {
        0.0
    } else {
        values.iter().sum::<f64>() / values.len() as f64
    }
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf_9ce4_8422_2325, |acc, byte| {
        (acc ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<(), DynError> {
    let mut writer = BufWriter::new(File::create(path)?);
    serde_json::to_writer_pretty(&mut writer, value)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

fn verify_legacy(path: &Path, out: &Path) -> Result<(), DynError> {
    let value: serde_json::Value = serde_json::from_slice(&fs::read(path)?)?;
    let number = |pointer: &str| value.pointer(pointer).and_then(serde_json::Value::as_u64);
    let view_rank_one = |name: &str| {
        value
            .get("views")?
            .as_array()?
            .iter()
            .find(|view| view.get("view").and_then(serde_json::Value::as_str) == Some(name))?
            .get("rank_one")?
            .as_u64()
    };
    let checks = [
        ("population", number("/population"), 38),
        (
            "legal conditions",
            number("/baseline_legal_conditions"),
            176,
        ),
        (
            "feasible conditions",
            number("/baseline_feasible_conditions"),
            174,
        ),
        ("B0 rank 1", number("/baseline_rank_counts/1"), 11),
        ("B0 rank 2", number("/baseline_rank_counts/2"), 24),
        ("B0 rank 3", number("/baseline_rank_counts/3"), 3),
        ("O rank 1", view_rank_one("O"), 36),
        ("A rank 1", view_rank_one("A"), 30),
    ];
    for (name, actual, expected) in checks {
        if actual != Some(expected) {
            return Err(format!(
                "legacy #209/#210 drift: {name} = {actual:?}, expected {expected}"
            )
            .into());
        }
    }
    let legacy = serde_json::json!({
        "schema": "griff.constraint-lab-chord-event-legacy-209-210.v1",
        "population": 38,
        "legal_conditions": 176,
        "feasible_conditions": 174,
        "b0_rank_counts": {"1": 11, "2": 24, "3": 3},
        "origin_rank_one": 36,
        "anchor_rank_one": 30,
        "source_summary": path.to_string_lossy(),
    });
    write_json(&out.join("chord-event-legacy-209-210.json"), &legacy)
}

fn parse_args() -> Result<(PathBuf, PathBuf, PathBuf), DynError> {
    let mut args = std::env::args().skip(1);
    let mut tabs = None;
    let mut out = None;
    let mut legacy_summary = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--tabs" => tabs = args.next().map(PathBuf::from),
            "--out" => out = args.next().map(PathBuf::from),
            "--legacy-summary" => legacy_summary = args.next().map(PathBuf::from),
            _ => return Err(format!("unknown argument {arg}").into()),
        }
    }
    Ok((
        tabs.ok_or("missing --tabs")?,
        out.ok_or("missing --out")?,
        legacy_summary.ok_or("missing --legacy-summary")?,
    ))
}
