//! General Guitar Pro chord-event representation census (Lab only).

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use griff_constraint_lab::chord_event::{
    analyze_regimes, causal_anchor_controls, chord_event_census, technique_string_controls,
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
    observed_non_target_rank: Option<usize>,
    observed_minus_mean_alternatives: Option<f64>,
    observed_minus_median_alternatives: Option<f64>,
    observed_minus_best_alternative: Option<f64>,
    observed_b0_minus_mean_alternatives: Option<f64>,
}

#[derive(Serialize)]
struct TechniqueStringRecord {
    string: u8,
    admissible_count: CountRecord,
    b0: Option<PreferredRecord>,
    anchor: Option<PreferredRecord>,
    whole_chord_membership: bool,
    non_target_uniform: Option<f64>,
    b0_non_target_uniform: Option<f64>,
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
    technique_non_target_uniform: Option<f64>,
    technique_observed_minus_alternatives: Vec<f64>,
    technique_observed_b0_minus_alternatives: Vec<f64>,
    true_minus_previous_uniform: Option<f64>,
    true_minus_previous_membership: Option<i8>,
    true_minus_lag_two_uniform: Option<f64>,
    true_minus_lag_two_membership: Option<i8>,
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

#[derive(Default, Serialize)]
struct EffectSummary {
    cases: usize,
    improved: usize,
    unchanged: usize,
    worsened: usize,
    uniform_delta_sum: f64,
    uniform_delta_mean: f64,
    membership_delta: i64,
    songs_with_improvement: usize,
    songs_with_worsening: usize,
}

#[derive(Default, Serialize)]
struct AmbiguitySummary {
    r0_median: u64,
    r0_b0_median: u64,
    r1_median: u64,
    r2_median: u64,
    r2_b0_median: u64,
    r3_median: u64,
    technique_fractional_reduction_mean: f64,
    technique_fractional_reduction_median: f64,
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
    technique_human_feasible: u64,
    technique_string_conditions: u64,
    technique_string_conditions_feasible: u64,
    anchor_effect: EffectSummary,
    technique_non_target_effect: EffectSummary,
    combined_effect: EffectSummary,
    anchor_previous_control_effect: EffectSummary,
    anchor_lag_two_control_effect: EffectSummary,
    technique_observed_control_effect: EffectSummary,
    technique_observed_b0_control_effect: EffectSummary,
    ambiguity: AmbiguitySummary,
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
    previous_control_cases: usize,
    true_minus_previous_uniform: f64,
    true_minus_previous_membership: i64,
    lag_two_control_cases: usize,
    true_minus_lag_two_uniform: f64,
    true_minus_lag_two_membership: i64,
    technique_observed_control_cases: usize,
    technique_observed_minus_alternatives: f64,
}

#[derive(Serialize)]
struct LooRecord {
    omitted_song: String,
    anchor_uniform_delta: f64,
    anchor_membership_delta: i64,
    combined_uniform_delta: f64,
    combined_membership_delta: i64,
    true_minus_previous_uniform: f64,
    true_minus_previous_membership: i64,
    true_minus_lag_two_uniform: f64,
    true_minus_lag_two_membership: i64,
    technique_observed_minus_alternatives: f64,
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
    let previous: BTreeMap<ChordEventIdentity, HandAnchor> =
        causal_anchor_controls(&anchors, 1).into_iter().collect();
    let lag_two: BTreeMap<ChordEventIdentity, HandAnchor> =
        causal_anchor_controls(&anchors, 2).into_iter().collect();
    let mut writer = BufWriter::new(File::create(out.join("chord-event-results.jsonl"))?);
    let mut census_writer = BufWriter::new(File::create(out.join("chord-event-census.jsonl"))?);
    let mut statuses = StatusCounts::default();
    let mut coverage = Coverage::default();
    let mut effects = Vec::new();
    let mut size_counts = BTreeMap::new();
    let mut family_counts = BTreeMap::new();
    let mut membership = [0_u64; 6];
    let mut conflicts = 0_u64;
    let mut technique_human_feasible = 0_u64;
    let mut technique_string_conditions = 0_u64;
    let mut technique_string_conditions_feasible = 0_u64;
    let mut ambiguity = [
        Vec::<u64>::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
    ];
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
        technique_human_feasible += u64::from(
            has_technique
                && analysis
                    .r2
                    .human
                    .as_ref()
                    .is_some_and(|human| human.exact_membership),
        );
        ambiguity[0].push(analysis.r0.admissible_count.value);
        ambiguity[1].push(
            analysis
                .r0
                .b0
                .as_ref()
                .map_or(0, |set| set.optimum_count.value),
        );
        ambiguity[2].push(
            analysis
                .r1
                .as_ref()
                .map_or(0, |set| set.optimum_count.value),
        );
        ambiguity[3].push(analysis.r2.admissible_count.value);
        ambiguity[4].push(
            analysis
                .r2
                .b0
                .as_ref()
                .map_or(0, |set| set.optimum_count.value),
        );
        ambiguity[5].push(
            analysis
                .r3
                .as_ref()
                .map_or(0, |set| set.optimum_count.value),
        );
        let previous_analysis = previous.get(&event.identity).and_then(|anchor| {
            analyze_regimes(&problem.with_anchor(Some(*anchor)), Some(observed)).ok()
        });
        let lag_two_analysis = lag_two.get(&event.identity).and_then(|anchor| {
            analyze_regimes(&problem.with_anchor(Some(*anchor)), Some(observed)).ok()
        });
        let technique_controls = technique_controls(problem, observed)?;
        effects.push(effect(
            event,
            &analysis,
            previous_analysis.as_ref(),
            lag_two_analysis.as_ref(),
            &technique_controls,
        ));
        for control in &technique_controls {
            technique_string_conditions =
                technique_string_conditions.saturating_add(control.strings.len() as u64);
            technique_string_conditions_feasible = technique_string_conditions_feasible
                .saturating_add(
                    control
                        .strings
                        .iter()
                        .filter(|condition| condition.admissible_count.value > 0)
                        .count() as u64,
                );
        }
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
    let technique_reductions: Vec<f64> = effects
        .iter()
        .filter_map(|effect| effect.technique_fractional_reduction)
        .collect();
    let summary = Summary {
        schema: "griff.constraint-lab-chord-event-representation.v2",
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
        technique_human_feasible,
        technique_string_conditions,
        technique_string_conditions_feasible,
        anchor_effect: summarize_effect(&effects, |effect| {
            effect.anchor_uniform.zip(effect.anchor_membership)
        }),
        technique_non_target_effect: summarize_effect(&effects, |effect| {
            effect.technique_non_target_uniform.map(|delta| (delta, 0))
        }),
        combined_effect: summarize_effect(&effects, |effect| {
            effect.combined_uniform.zip(effect.combined_membership)
        }),
        anchor_previous_control_effect: summarize_effect(&effects, |effect| {
            effect
                .true_minus_previous_uniform
                .zip(effect.true_minus_previous_membership)
        }),
        anchor_lag_two_control_effect: summarize_effect(&effects, |effect| {
            effect
                .true_minus_lag_two_uniform
                .zip(effect.true_minus_lag_two_membership)
        }),
        technique_observed_control_effect: summarize_values(&effects, |effect| {
            &effect.technique_observed_minus_alternatives
        }),
        technique_observed_b0_control_effect: summarize_values(&effects, |effect| {
            &effect.technique_observed_b0_minus_alternatives
        }),
        ambiguity: AmbiguitySummary {
            r0_median: median_u64(&mut ambiguity[0]),
            r0_b0_median: median_u64(&mut ambiguity[1]),
            r1_median: median_u64(&mut ambiguity[2]),
            r2_median: median_u64(&mut ambiguity[3]),
            r2_b0_median: median_u64(&mut ambiguity[4]),
            r3_median: median_u64(&mut ambiguity[5]),
            technique_fractional_reduction_mean: mean(&technique_reductions),
            technique_fractional_reduction_median: median_f64(technique_reductions),
        },
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
                    row.previous_control_cases,
                    row.true_minus_previous_uniform,
                    row.true_minus_previous_membership,
                    row.lag_two_control_cases,
                    row.true_minus_lag_two_uniform,
                    row.true_minus_lag_two_membership,
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
                    row.technique_observed_control_cases,
                    row.technique_observed_minus_alternatives,
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
    let excluded: BTreeSet<usize> = targets.keys().copied().collect();
    targets
        .into_iter()
        .map(|(target_atom_id, observed_string)| {
            let strings: Vec<_> =
                technique_string_controls(problem, target_atom_id, Some(observed))?
                    .into_iter()
                    .map(|entry| {
                        let whole_chord_membership = entry
                            .assignments
                            .iter()
                            .any(|assignment| assignment_matches(assignment, observed));
                        let non_target_score =
                            non_target_uniform(&entry.assignments, observed, &excluded);
                        let b0_non_target_uniform = entry.b0.as_ref().and_then(|set| {
                            non_target_uniform(&set.assignments, observed, &excluded)
                        });
                        TechniqueStringRecord {
                            string: entry.string,
                            admissible_count: entry.admissible_count.into(),
                            b0: entry.b0.as_ref().map(preferred_record),
                            anchor: entry.anchor.as_ref().map(preferred_record),
                            whole_chord_membership,
                            non_target_uniform: non_target_score,
                            b0_non_target_uniform,
                        }
                    })
                    .collect();
            let observed_value = strings
                .iter()
                .find(|row| row.string == observed_string)
                .and_then(|row| row.non_target_uniform);
            let alternatives: Vec<f64> = strings
                .iter()
                .filter(|row| row.string != observed_string)
                .filter_map(|row| row.non_target_uniform)
                .collect();
            let observed_b0 = strings
                .iter()
                .find(|row| row.string == observed_string)
                .and_then(|row| row.b0_non_target_uniform);
            let b0_alternatives: Vec<f64> = strings
                .iter()
                .filter(|row| row.string != observed_string)
                .filter_map(|row| row.b0_non_target_uniform)
                .collect();
            let observed_non_target_rank = observed_value.map(|value| {
                1 + strings
                    .iter()
                    .filter_map(|row| row.non_target_uniform)
                    .filter(|alternative| *alternative > value + 1e-12)
                    .count()
            });
            Ok(TechniqueControlRecord {
                target_atom_id,
                observed_string,
                strings,
                observed_non_target_rank,
                observed_minus_mean_alternatives: observed_value
                    .filter(|_| !alternatives.is_empty())
                    .map(|value| value - mean(&alternatives)),
                observed_minus_median_alternatives: observed_value
                    .filter(|_| !alternatives.is_empty())
                    .map(|value| value - median_f64(alternatives.clone())),
                observed_minus_best_alternative: observed_value
                    .zip(alternatives.iter().copied().max_by(f64::total_cmp))
                    .map(|(value, best)| value - best),
                observed_b0_minus_mean_alternatives: observed_b0
                    .filter(|_| !b0_alternatives.is_empty())
                    .map(|value| value - mean(&b0_alternatives)),
            })
        })
        .collect()
}

fn assignment_matches(assignment: &ChordAssignment, observed: &ObservedChordVoicing) -> bool {
    observed
        .positions()
        .iter()
        .all(|expected| assignment.position(expected.atom_id) == Some(expected.position))
}

#[allow(clippy::cast_precision_loss)] // descriptive ratio; exact counts retained
fn effect(
    event: &ChordCensusEvent,
    analysis: &griff_constraint_lab::chord_event::ChordRegimeAnalysis,
    previous: Option<&griff_constraint_lab::chord_event::ChordRegimeAnalysis>,
    lag_two: Option<&griff_constraint_lab::chord_event::ChordRegimeAnalysis>,
    technique_controls: &[TechniqueControlRecord],
) -> Effect {
    let has_technique = event
        .problem
        .as_ref()
        .is_some_and(|problem| !problem.incoming_techniques().is_empty());
    let r0 = analysis.r0.b0.as_ref().and_then(|set| set.human.as_ref());
    let r1 = analysis.r1.as_ref().and_then(|set| set.human.as_ref());
    let r2 = analysis.r2.b0.as_ref().and_then(|set| set.human.as_ref());
    let r3 = analysis.r3.as_ref().and_then(|set| set.human.as_ref());
    let previous_r1 = previous
        .and_then(|value| value.r1.as_ref())
        .and_then(|set| set.human.as_ref());
    let lag_two_r1 = lag_two
        .and_then(|value| value.r1.as_ref())
        .and_then(|set| set.human.as_ref());
    let count0 = analysis.r0.admissible_count.value as f64;
    let count2 = analysis.r2.admissible_count.value as f64;
    let observed = event.observed.as_ref();
    let targets: BTreeSet<usize> = event
        .problem
        .as_ref()
        .into_iter()
        .flat_map(griff_constraint_lab::chord_event::ChordEventProblem::incoming_techniques)
        .map(|incoming| incoming.target_atom_id)
        .collect();
    let non_target =
        observed.and_then(|observed| {
            non_target_uniform(&analysis.r0.assignments, observed, &targets).zip(
                non_target_uniform(&analysis.r2.assignments, observed, &targets),
            )
        });
    Effect {
        song: event.identity.song_key.clone(),
        anchor_uniform: r0
            .zip(r1)
            .map(|(base, context)| context.uniform - base.uniform),
        anchor_membership: r0.zip(r1).map(|(base, context)| {
            i8::from(context.exact_membership) - i8::from(base.exact_membership)
        }),
        combined_uniform: has_technique
            .then(|| {
                r2.zip(r3)
                    .map(|(base, context)| context.uniform - base.uniform)
            })
            .flatten(),
        combined_membership: has_technique
            .then(|| {
                r2.zip(r3).map(|(base, context)| {
                    i8::from(context.exact_membership) - i8::from(base.exact_membership)
                })
            })
            .flatten(),
        technique_fractional_reduction: (has_technique && count0 > 0.0)
            .then(|| (count0 - count2) / count0),
        technique_non_target_uniform: has_technique
            .then(|| non_target.map(|(r0, r2)| r2 - r0))
            .flatten(),
        technique_observed_minus_alternatives: technique_controls
            .iter()
            .filter_map(|control| control.observed_minus_mean_alternatives)
            .collect(),
        technique_observed_b0_minus_alternatives: technique_controls
            .iter()
            .filter_map(|control| control.observed_b0_minus_mean_alternatives)
            .collect(),
        true_minus_previous_uniform: r1
            .zip(previous_r1)
            .map(|(true_anchor, wrong)| true_anchor.uniform - wrong.uniform),
        true_minus_previous_membership: r1.zip(previous_r1).map(|(true_anchor, wrong)| {
            i8::from(true_anchor.exact_membership) - i8::from(wrong.exact_membership)
        }),
        true_minus_lag_two_uniform: r1
            .zip(lag_two_r1)
            .map(|(true_anchor, wrong)| true_anchor.uniform - wrong.uniform),
        true_minus_lag_two_membership: r1.zip(lag_two_r1).map(|(true_anchor, wrong)| {
            i8::from(true_anchor.exact_membership) - i8::from(wrong.exact_membership)
        }),
    }
}

#[allow(clippy::cast_precision_loss)] // descriptive uniform agreement
fn non_target_uniform(
    assignments: &[ChordAssignment],
    observed: &ObservedChordVoicing,
    excluded: &BTreeSet<usize>,
) -> Option<f64> {
    let positions: Vec<_> = observed
        .positions()
        .iter()
        .filter(|position| !excluded.contains(&position.atom_id))
        .collect();
    if positions.is_empty() || assignments.is_empty() {
        return None;
    }
    let matches: usize = assignments
        .iter()
        .map(|assignment| {
            positions
                .iter()
                .filter(|expected| assignment.position(expected.atom_id) == Some(expected.position))
                .count()
        })
        .sum();
    Some(matches as f64 / (assignments.len() * positions.len()) as f64)
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
    let previous_control: Vec<_> = rows
        .iter()
        .filter_map(|row| {
            row.true_minus_previous_uniform
                .zip(row.true_minus_previous_membership)
        })
        .collect();
    let lag_two_control: Vec<_> = rows
        .iter()
        .filter_map(|row| {
            row.true_minus_lag_two_uniform
                .zip(row.true_minus_lag_two_membership)
        })
        .collect();
    let technique_control: Vec<_> = rows
        .iter()
        .flat_map(|row| row.technique_observed_minus_alternatives.iter().copied())
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
        previous_control_cases: previous_control.len(),
        true_minus_previous_uniform: previous_control.iter().map(|row| row.0).sum(),
        true_minus_previous_membership: previous_control.iter().map(|row| i64::from(row.1)).sum(),
        lag_two_control_cases: lag_two_control.len(),
        true_minus_lag_two_uniform: lag_two_control.iter().map(|row| row.0).sum(),
        true_minus_lag_two_membership: lag_two_control.iter().map(|row| i64::from(row.1)).sum(),
        technique_observed_control_cases: technique_control.len(),
        technique_observed_minus_alternatives: technique_control.iter().sum(),
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
                true_minus_previous_uniform: row.true_minus_previous_uniform,
                true_minus_previous_membership: row.true_minus_previous_membership,
                true_minus_lag_two_uniform: row.true_minus_lag_two_uniform,
                true_minus_lag_two_membership: row.true_minus_lag_two_membership,
                technique_observed_minus_alternatives: row.technique_observed_minus_alternatives,
            }
        })
        .collect()
}

#[allow(clippy::cast_precision_loss)] // descriptive mean over exact case values
fn summarize_effect(
    effects: &[Effect],
    select: impl Fn(&Effect) -> Option<(f64, i8)>,
) -> EffectSummary {
    let rows: Vec<_> = effects
        .iter()
        .filter_map(|effect| select(effect).map(|value| (&effect.song, value)))
        .collect();
    let mut songs: BTreeMap<&str, f64> = BTreeMap::new();
    for (song, (delta, _)) in &rows {
        *songs.entry(song).or_default() += *delta;
    }
    let epsilon = 1e-12;
    let sum: f64 = rows.iter().map(|(_, row)| row.0).sum();
    EffectSummary {
        cases: rows.len(),
        improved: rows.iter().filter(|(_, row)| row.0 > epsilon).count(),
        unchanged: rows
            .iter()
            .filter(|(_, row)| row.0.abs() <= epsilon)
            .count(),
        worsened: rows.iter().filter(|(_, row)| row.0 < -epsilon).count(),
        uniform_delta_sum: sum,
        uniform_delta_mean: if rows.is_empty() {
            0.0
        } else {
            sum / rows.len() as f64
        },
        membership_delta: rows.iter().map(|(_, row)| i64::from(row.1)).sum(),
        songs_with_improvement: songs.values().filter(|delta| **delta > epsilon).count(),
        songs_with_worsening: songs.values().filter(|delta| **delta < -epsilon).count(),
    }
}

#[allow(clippy::cast_precision_loss)] // descriptive mean over exact control values
fn summarize_values<'a>(
    effects: &'a [Effect],
    select: impl Fn(&'a Effect) -> &'a [f64],
) -> EffectSummary {
    let expanded: Vec<Effect> = effects
        .iter()
        .flat_map(|effect| {
            select(effect).iter().map(|delta| Effect {
                song: effect.song.clone(),
                anchor_uniform: Some(*delta),
                anchor_membership: Some(0),
                combined_uniform: None,
                combined_membership: None,
                technique_fractional_reduction: None,
                technique_non_target_uniform: None,
                technique_observed_minus_alternatives: Vec::new(),
                technique_observed_b0_minus_alternatives: Vec::new(),
                true_minus_previous_uniform: None,
                true_minus_previous_membership: None,
                true_minus_lag_two_uniform: None,
                true_minus_lag_two_membership: None,
            })
        })
        .collect();
    summarize_effect(&expanded, |effect| {
        effect.anchor_uniform.zip(effect.anchor_membership)
    })
}

fn median_u64(values: &mut [u64]) -> u64 {
    values.sort_unstable();
    values
        .get(values.len().saturating_sub(1) / 2)
        .copied()
        .unwrap_or(0)
}

fn median_f64(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    values
        .get(values.len().saturating_sub(1) / 2)
        .copied()
        .unwrap_or(0.0)
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
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |acc, byte| {
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
