//! Candidate-set presentation (S8): the ranked set a frontend browses.
//!
//! Generation itself is **not** implemented here — it is
//! [`griff_core::generation_input::ranked_candidates`], the same entry point
//! `griff generate` uses, so a cockpit set and a CLI set are the same set. This
//! module only turns that result into rows a table can paint: rank, strategy,
//! variant seed, the aggregate and the six rerank axes behind it, and a stable
//! id to hang provenance (and, later, S9 feedback) on.
//!
//! Pure and wasm-safe: the caller hands in an already-imported source score and
//! already-parsed corpus material, and owns its own I/O.

use griff_core::candidate_chain::{
    intact_s6_cost, plan_candidate_chain, ChainError, PlannedCandidateChain,
};
use griff_core::generation_input::{
    ranked_candidates, CorpusMaterial, GenerationAsk, GenerationInputError, RankedSet,
};
use griff_core::rerank::RERANK_AXIS_LABELS;
use griff_core::score::{AtomEvent, Score};

/// One candidate, as a table row.
#[derive(Debug, Clone, PartialEq)]
pub struct CandidateRow {
    /// 1-based rank in the reranked set (1 is the winner `griff generate`
    /// would have written).
    pub rank: usize,
    /// Stable within a set and reproducible across runs of the same ask:
    /// `<strategy>#<variant-seed hex>`. Rerunning the same ask reproduces this
    /// candidate exactly.
    pub id: String,
    /// The strategy that produced it.
    pub strategy: String,
    /// The derived variant seed it ran under — its reproduction key.
    pub variant_seed: u64,
    /// Weighted aggregate over the six rerank axes.
    pub aggregate: f64,
    /// Each rerank axis and its value, in [`RERANK_AXIS_LABELS`] order.
    pub axes: Vec<(&'static str, f64)>,
    /// Notes in the candidate's first track — a coarse density read.
    pub note_count: usize,
    /// Lowest and highest sounding pitch, `None` when silent.
    pub pitch_range: Option<(u8, u8)>,
}

/// What the pass was given — shown above the table so a curator can see whether
/// the corpus actually contributed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetSummary {
    /// Rhythm templates the pass rotated.
    pub templates: usize,
    /// Novelty reference chunks.
    pub references: usize,
    /// The gesture ask, when the pass carved one.
    pub gesture: Option<(usize, String)>,
    /// Distinct scale tones the source supplied (12 means the source's tracks
    /// union to the full chromatic — see S15).
    pub scale_tones: usize,
    /// Corpus records that could not be loaded (never silently dropped).
    pub skipped: Vec<String>,
}

/// A generated, reranked, browsable set.
#[derive(Debug, Clone)]
pub struct CandidateSet {
    /// Rows in rank order.
    pub rows: Vec<CandidateRow>,
    /// The candidate scores, parallel to [`Self::rows`] — index `i` is the
    /// score for `rows[i]`.
    pub scores: Vec<Score>,
    /// What the pass was given.
    pub summary: SetSummary,
}

impl CandidateSet {
    /// Presents an already-generated [`RankedSet`] as browsable rows.
    ///
    /// The one mapping every frontend uses — `generate_set` for the corpus
    /// path, the Swang evaluator's result for the authoring path — so a
    /// candidate table is the same table wherever the set came from. The
    /// `material` supplies only the reference count and skipped-record list,
    /// which live outside the ranked set.
    #[must_use]
    pub fn from_ranked(set: &RankedSet, material: Option<&CorpusMaterial>) -> Self {
        let rows = set
            .ranked
            .iter()
            .enumerate()
            .map(|(i, scored)| {
                let variant_seed = scored.value.seed.0;
                let strategy = format!("{:?}", scored.value.strategy);
                CandidateRow {
                    rank: i.saturating_add(1),
                    id: format!("{strategy}#{variant_seed:016x}"),
                    strategy,
                    variant_seed,
                    aggregate: scored.aggregate(),
                    axes: RERANK_AXIS_LABELS
                        .iter()
                        .map(|&label| (label, scored.axes.get(label).unwrap_or(0.0)))
                        .collect(),
                    note_count: note_count(&scored.value.score),
                    pitch_range: pitch_range(&scored.value.score),
                }
            })
            .collect();
        let scores = set.ranked.iter().map(|s| s.value.score.clone()).collect();

        Self {
            rows,
            scores,
            summary: SetSummary {
                templates: set.source_rhythms.len(),
                references: material.map_or(0, |m| m.references.len()),
                gesture: set
                    .gesture
                    .map(|g| (g.burst_notes, format!("{:.1}q", g.rest_quarters))),
                scale_tones: set.base.pitch_material.intervals.len(),
                skipped: material.map_or_else(Vec::new, |m| m.skipped.clone()),
            },
        }
    }
}

/// Generates and reranks a candidate set, then presents it as rows.
///
/// A thin adapter over [`ranked_candidates`] — it adds no musical decision of
/// its own.
///
/// # Errors
/// Whatever the shared compiler rejects: a source that cannot seed a request,
/// or a candidate-set builder rejection (e.g. a zero variant count).
pub fn generate_set(
    source: &Score,
    material: Option<&CorpusMaterial>,
    ask: &GenerationAsk,
) -> Result<CandidateSet, GenerationInputError> {
    let set = ranked_candidates(source, material, ask, None)?;
    Ok(CandidateSet::from_ranked(&set, material))
}

/// The S7 global candidate chain planned from a Generate run's ranked set.
///
/// Holds the core's own trace verbatim — [`PlannedCandidateChain`] carries the
/// assembled score, the per-bar steps, the per-boundary transitions, the total
/// and the chain policy's provenance — beside the S6 baseline the comparison is
/// against. Nothing here is recomputed or re-explained: the UI displays what
/// the core returned.
#[derive(Debug, Clone)]
pub struct PlannedGlobalChain {
    /// The core's plan: assembled score, steps, transitions, total, provenance.
    pub plan: PlannedCandidateChain,
    /// Ranked candidate 0 kept intact, weighed under the *same* chain policy —
    /// the number the chain's total is compared against.
    pub baseline_cost: f64,
}

/// What a run's global chain came to: a plan, or a typed refusal.
///
/// Refusal is a first-class outcome, not an absence. The chain can be refused
/// for reasons that are entirely about the set (a candidate disagreeing about
/// the master timeline, material crossing a bar line), and when it is, the
/// error is kept — a chain that could not be planned must never be reported as
/// an empty chain, and the intact S6 winner is unaffected either way.
#[derive(Debug, Clone)]
pub enum GlobalChainOutcome {
    /// The chain was planned; audition it.
    Planned(Box<PlannedGlobalChain>),
    /// The chain was refused, and this is why. Kept typed.
    Refused(ChainError),
}

/// What one Generate run produced.
///
/// Both results — the intact S6 winner (row/score 0 of `set`) and the S7 global
/// chain — come from **one** [`RankedSet`], planned once, here. The set is
/// consumed by this function and never seen again, which is what makes
/// re-planning later impossible rather than merely discouraged.
#[derive(Debug, Clone)]
pub struct GeneratedRun {
    /// The reranked set as browsable rows plus its per-candidate scores.
    pub set: CandidateSet,
    /// The global chain planned from the same ranked set.
    pub chain: GlobalChainOutcome,
}

/// Generates and reranks a candidate set, then plans the global chain from that
/// same set.
///
/// One pass, one `RankedSet`, both results. `ranked_candidates` is called
/// exactly once and the chain is planned from its live result — the alternative
/// (keeping the set around to plan from later) is what lets a chain drift away
/// from the candidates it claims to be made of.
///
/// # Errors
/// Whatever the shared compiler rejects: a source that cannot seed a request,
/// or a candidate-set builder rejection (e.g. a zero variant count). A refused
/// *chain* is not an error here — the run succeeded, and the refusal is carried
/// in [`GeneratedRun::chain`].
pub fn generate_run(
    source: &Score,
    material: Option<&CorpusMaterial>,
    ask: &GenerationAsk,
) -> Result<GeneratedRun, GenerationInputError> {
    let set = ranked_candidates(source, material, ask, None)?;
    let chain = plan_global_chain(&set);
    Ok(GeneratedRun {
        set: CandidateSet::from_ranked(&set, material),
        chain,
    })
}

/// Plans the global chain and its baseline from one ranked set.
///
/// Both sides of the comparison are measured here, from this one set, under the
/// one policy the core owns. They refuse together — each validates the set's
/// chain-compatibility — so a refusal is reported once, typed, rather than as a
/// half-available comparison.
fn plan_global_chain(set: &RankedSet) -> GlobalChainOutcome {
    match (plan_candidate_chain(set), intact_s6_cost(set)) {
        (Ok(plan), Ok(baseline_cost)) => {
            GlobalChainOutcome::Planned(Box::new(PlannedGlobalChain {
                plan,
                baseline_cost,
            }))
        }
        (Err(error), _) | (Ok(_), Err(error)) => GlobalChainOutcome::Refused(error),
    }
}

/// Notes across a score's first track.
fn note_count(score: &Score) -> usize {
    notes(score).count()
}

/// Lowest and highest sounding pitch of a score's first track.
fn pitch_range(score: &Score) -> Option<(u8, u8)> {
    let lo = notes(score).min()?;
    let hi = notes(score).max()?;
    Some((lo, hi))
}

/// Every note pitch in the score's first track, in event order.
fn notes(score: &Score) -> impl Iterator<Item = u8> + '_ {
    score
        .tracks
        .first()
        .into_iter()
        .flat_map(|t| &t.voices)
        .flat_map(|v| &v.event_groups)
        .flat_map(|g| &g.atoms)
        .filter_map(|a| match a {
            AtomEvent::Note(n) => Some(n.pitch.0),
            AtomEvent::Rest(_) => None,
        })
}

#[cfg(test)]
mod tests {
    // Fixture arithmetic runs over a fixed `0..4`, so it cannot overflow.
    #![allow(
        clippy::arithmetic_side_effects,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic
    )]

    use super::*;
    use griff_core::event::{NoteMarks, Pitch, Tempo, Ticks, TimeSignature, Tuning, Velocity};
    use griff_core::score::{
        AtomNote, EventGroup, EventGroupKind, LossReport, MasterBar, RepeatMarker, Track, Voice,
    };
    use griff_core::slice::TickRange;

    const PPQN: u16 = 480;
    const BAR: u32 = 1920;

    /// One bar of four ascending quarter notes — enough pitch material and
    /// rhythm to seed a request.
    fn source() -> Score {
        let event_groups = (0..4u32)
            .map(|i| EventGroup {
                kind: EventGroupKind::Single,
                atoms: vec![AtomEvent::Note(AtomNote {
                    absolute_start: Ticks(i * u32::from(PPQN)),
                    duration: Ticks(u32::from(PPQN)),
                    pitch: Pitch::new(60 + u8::try_from(i).expect("0..4")).expect("valid pitch"),
                    velocity: Velocity::new(96).expect("valid velocity"),
                    marks: NoteMarks::empty(),
                    position: None,
                })],
                technique_spans: Vec::new(),
            })
            .collect();
        Score {
            ticks_per_quarter: PPQN,
            master_bars: vec![MasterBar {
                index: 0,
                tick_range: TickRange::new(Ticks(0), Ticks(BAR)).expect("ordered"),
                time_signature: TimeSignature::new(4, 4).expect("4/4"),
                tempo: Tempo::new(120.0).expect("120 BPM"),
                repeat: RepeatMarker::default(),
            }],
            tracks: vec![Track {
                name: Some("guitar".to_owned()),
                channel: 0,
                voices: vec![Voice {
                    id: 0,
                    event_groups,
                }],
                tuning: Tuning::standard_e(),
            }],
            source_meta: None,
            loss: LossReport::new(),
        }
    }

    fn ask() -> GenerationAsk {
        GenerationAsk {
            seed: 7,
            bars: 4,
            variants_per_strategy: 2,
            gesture: false,
        }
    }

    // ── Global Chain Audition: one run, two results ──────────────────────────

    #[test]
    fn one_generate_run_yields_both_the_intact_winner_and_the_global_chain() {
        // The whole premise of the comparison: both sides come from the same
        // pass. If the chain were planned from a second `ranked_candidates`
        // call, the two would be answers to two different questions that merely
        // look like a before and after.
        let run = generate_run(&source(), None, &ask()).expect("a four-note bar seeds a request");
        assert!(!run.set.rows.is_empty(), "the ranked set is presented");
        let GlobalChainOutcome::Planned(chain) = &run.chain else {
            panic!("this fixture's set is chain-compatible: {:?}", run.chain);
        };
        assert_eq!(
            chain.plan.steps.len(),
            ask().bars,
            "one selected bar per asked bar",
        );
        assert_eq!(
            chain.plan.transitions.len(),
            ask().bars - 1,
            "one transition per bar line",
        );
        assert_eq!(
            chain.plan.score.master_bars.len(),
            ask().bars,
            "the assembled score spans the asked bars",
        );
    }

    #[test]
    fn every_chain_step_names_a_candidate_of_this_runs_set() {
        // The chain is made of *these* candidates. Each step's supplier must be
        // an ordinal into the set the same run produced, carrying that
        // candidate's own rank — not a rank invented by the chain.
        let run = generate_run(&source(), None, &ask()).expect("seeds");
        let GlobalChainOutcome::Planned(chain) = &run.chain else {
            panic!("chain-compatible fixture");
        };
        for step in &chain.plan.steps {
            let row = run
                .set
                .rows
                .get(step.state.candidate)
                .expect("the supplier is a candidate of this run's set");
            assert_eq!(
                row.rank, step.state.rank,
                "the step carries the supplying candidate's own rank",
            );
            assert_eq!(
                row.variant_seed, step.state.variant_seed.0,
                "and its own derived variant seed",
            );
        }
    }

    #[test]
    fn the_baseline_is_the_intact_winner_measured_by_the_core() {
        // Not recomputed here, and not a different metric: the number the core
        // returns for ranked candidate 0 kept whole, under the chain policy.
        let set = ranked_candidates(&source(), None, &ask(), None).expect("seeds");
        let expected = intact_s6_cost(&set).expect("compatible");
        let run = generate_run(&source(), None, &ask()).expect("seeds");
        let GlobalChainOutcome::Planned(chain) = &run.chain else {
            panic!("chain-compatible fixture");
        };
        assert_eq!(
            chain.baseline_cost.to_bits(),
            expected.to_bits(),
            "the baseline is the core's, bit for bit",
        );
    }

    #[test]
    fn the_planned_chain_carries_the_chain_policys_own_provenance() {
        // The UI shows what the core decided, under the policy the core used.
        let run = generate_run(&source(), None, &ask()).expect("seeds");
        let GlobalChainOutcome::Planned(chain) = &run.chain else {
            panic!("chain-compatible fixture");
        };
        assert_eq!(chain.plan.provenance.policy_id, "candidate_chain");
        assert_eq!(chain.plan.provenance.policy_version, 1);
        assert_eq!(
            chain.plan.provenance.seed, None,
            "selection is deterministic by construction — no seed takes part",
        );
    }

    #[test]
    fn a_run_is_reproducible_under_a_fixed_ask() {
        // Same ask, same run — the chain included. Nothing in the planning path
        // reaches for a clock, an RNG, or a hash-map order.
        let a = generate_run(&source(), None, &ask()).expect("seeds");
        let b = generate_run(&source(), None, &ask()).expect("seeds");
        let (GlobalChainOutcome::Planned(a), GlobalChainOutcome::Planned(b)) = (&a.chain, &b.chain)
        else {
            panic!("chain-compatible fixture");
        };
        assert_eq!(
            a.plan.total_cost.to_bits(),
            b.plan.total_cost.to_bits(),
            "the same total, bit for bit",
        );
        assert_eq!(
            a.plan
                .steps
                .iter()
                .map(|s| s.state.candidate)
                .collect::<Vec<_>>(),
            b.plan
                .steps
                .iter()
                .map(|s| s.state.candidate)
                .collect::<Vec<_>>(),
            "and the same suppliers",
        );
    }

    #[test]
    fn rows_are_rank_ordered_and_parallel_to_scores() {
        let set = generate_set(&source(), None, &ask()).expect("a four-note bar seeds a request");
        assert_eq!(set.rows.len(), set.scores.len(), "rows index into scores");
        assert!(!set.rows.is_empty(), "five strategies x 2 variants");
        for (i, row) in set.rows.iter().enumerate() {
            assert_eq!(row.rank, i + 1, "rank is 1-based position");
        }
        for pair in set.rows.windows(2) {
            assert!(
                pair[0].aggregate >= pair[1].aggregate,
                "aggregate descends: {} then {}",
                pair[0].aggregate,
                pair[1].aggregate,
            );
        }
    }

    #[test]
    fn the_same_ask_reproduces_the_same_set() {
        let (a, b) = (
            generate_set(&source(), None, &ask()).expect("seeds"),
            generate_set(&source(), None, &ask()).expect("seeds"),
        );
        assert_eq!(a.rows, b.rows, "generation is deterministic under a seed");
    }

    #[test]
    fn a_candidate_id_carries_its_reproduction_key() {
        let set = generate_set(&source(), None, &ask()).expect("seeds");
        let row = &set.rows[0];
        assert!(
            row.id.starts_with(&row.strategy) && row.id.contains('#'),
            "id names the strategy: {}",
            row.id,
        );
        assert!(
            row.id.ends_with(&format!("{:016x}", row.variant_seed)),
            "id carries the variant seed that reproduces it: {}",
            row.id,
        );
    }

    #[test]
    fn every_rerank_axis_is_surfaced() {
        let set = generate_set(&source(), None, &ask()).expect("seeds");
        let labels: Vec<&str> = set.rows[0].axes.iter().map(|&(l, _)| l).collect();
        assert_eq!(
            labels,
            RERANK_AXIS_LABELS.to_vec(),
            "all six axes, in canonical order",
        );
    }

    #[test]
    fn a_zero_variant_ask_is_rejected_not_silently_empty() {
        let ask = GenerationAsk {
            variants_per_strategy: 0,
            ..ask()
        };
        assert!(
            generate_set(&source(), None, &ask).is_err(),
            "an empty set is an error, never an empty table",
        );
    }
}
