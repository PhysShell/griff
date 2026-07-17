//! The cockpit's Generate panel state (S8): ask for a candidate set, browse it,
//! keep what survives.
//!
//! Generation is **not** implemented here. The panel calls
//! [`griff_ui_core::generate::generate_set`], which calls
//! `griff_core::generation_input::ranked_candidates` — the same entry point
//! `griff generate` uses — so a set browsed here is the set the CLI would have
//! written. What lives here is the panel's plain state, the corpus *directory*
//! I/O the native app owns (the web app reads the same records out of OPFS), and
//! the provenance a kept candidate is stamped with.

#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;

#[cfg(not(target_arch = "wasm32"))]
use griff_core::corpus::ChunkMeta;
#[cfg(not(target_arch = "wasm32"))]
use griff_core::generation_input::CorpusMaterial;
use griff_core::generation_input::GenerationAsk;
use griff_ui_core::generate::{CandidateRow, CandidateSet, GlobalChainOutcome};
use griff_ui_core::history::{CorpusContribution, GenerationRunId};

/// A tab the panel can seed a generation from: its display name and the bytes
/// the shared importer parses. Bytes, not a path, so the same state works in the
/// browser.
#[derive(Debug, Clone)]
pub struct SourceTab {
    /// Display name (the file's name).
    pub name: String,
    /// The raw MIDI / Guitar Pro bytes.
    pub bytes: Vec<u8>,
}

/// The **immutable** request/input identity of one Generate run, captured the
/// moment the set was produced.
///
/// A candidate's provenance reads its request fields from here, never from live
/// panel state, so changing a knob (or attaching a corpus) after generation
/// cannot rewrite an already-made candidate's origin.
#[derive(Debug, Clone)]
pub struct GenerateRunContext {
    /// The run this set belongs to.
    pub run: GenerationRunId,
    /// The seed score's identity (a source-tab name, or the displayed title).
    pub source: Option<String>,
    /// The ask seed.
    pub seed: u64,
    /// Bars generated.
    pub bars: usize,
    /// Seed variants per strategy.
    pub variants_per_strategy: usize,
    /// What the corpus actually contributed to this pass.
    pub corpus: CorpusContribution,
}

/// A produced Generate set bound to the immutable context that made it — so the
/// set and its provenance identity cannot drift apart.
///
/// The S7 global chain is bound in here for the same reason. It is planned once,
/// when the set is produced, from the very `RankedSet` the set was presented
/// from; afterwards it is a snapshot like any other. Nothing — auditioning,
/// playback, export, favourite/reject, opening a history entry — re-plans it,
/// because there is no longer a `RankedSet` to plan from.
#[derive(Debug)]
pub struct ActiveGenerateRun {
    /// The immutable run context.
    pub context: GenerateRunContext,
    /// The reranked candidate set.
    pub set: CandidateSet,
    /// The global chain this run's set yielded — planned, or typed-refused.
    pub chain: GlobalChainOutcome,
}

/// The Generate panel's state.
#[derive(Debug, Default)]
pub struct GeneratePanel {
    /// Whether the panel window is shown (the `g` key toggles it).
    pub open: bool,
    /// Seed tabs to choose from — the corpus's source tabs on native, empty in
    /// the browser (which seeds from the displayed score).
    pub sources: Vec<SourceTab>,
    /// Index into [`Self::sources`]; `None` seeds from the displayed score.
    pub source: Option<usize>,
    /// Deterministic seed.
    pub seed: u64,
    /// Bars to generate.
    pub bars: usize,
    /// Seed variants per strategy (the set holds this × 5 strategies).
    pub variants: usize,
    /// Carve the corpus's burst/rest gesture.
    pub gesture: bool,
    /// The last produced run: its set bound to the immutable context that made
    /// it. `None` until the first generation.
    pub active: Option<ActiveGenerateRun>,
    /// Index into the set's rows — the candidate the roll is showing.
    pub selected: Option<usize>,
    /// Outcome of the last generate / keep, shown in the panel.
    pub status: Option<String>,
}

impl GeneratePanel {
    /// A panel with the CLI's defaults (`griff generate`: seed 0, 8 bars,
    /// 2 variants per strategy, gesture on).
    #[must_use]
    pub const fn new() -> Self {
        Self {
            open: false,
            sources: Vec::new(),
            source: None,
            seed: 0,
            bars: 8,
            variants: 2,
            gesture: true,
            active: None,
            selected: None,
            status: None,
        }
    }

    /// The current run's candidate set, if any.
    #[must_use]
    pub fn set(&self) -> Option<&CandidateSet> {
        self.active.as_ref().map(|a| &a.set)
    }

    /// The current run's immutable context, if any.
    #[must_use]
    pub fn context(&self) -> Option<&GenerateRunContext> {
        self.active.as_ref().map(|a| &a.context)
    }

    /// The ask the knobs currently describe.
    #[must_use]
    pub const fn ask(&self) -> GenerationAsk {
        GenerationAsk {
            seed: self.seed,
            bars: self.bars,
            variants_per_strategy: self.variants,
            gesture: self.gesture,
        }
    }

    /// The selected seed tab, when one is picked.
    #[must_use]
    pub fn source_tab(&self) -> Option<&SourceTab> {
        self.source.and_then(|i| self.sources.get(i))
    }
}

/// A corpus read off a directory: the material a generation pass consumes, plus
/// the source tabs it was built from (the panel's seed-tab pick-list).
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug)]
pub struct LoadedCorpus {
    /// The rhythm/novelty/gesture material.
    pub material: CorpusMaterial,
    /// The distinct source tabs the records point at, by first-seen order.
    pub sources: Vec<SourceTab>,
}

/// Reads a corpus *directory* — the native app's I/O half.
///
/// Mirrors what the CLI's `load_corpus_material` does and what the web app does
/// over OPFS. Every musical decision (slicing, rhythm extraction, gesture
/// aggregation) is core's.
///
/// Records are visited in sorted order, so the rhythm-template palette is
/// deterministic. A record whose source is missing, unreadable, unimportable, or
/// silent is reported in `material.skipped`, never silently dropped.
///
/// # Errors
/// A message when `dir` cannot be read.
#[cfg(not(target_arch = "wasm32"))]
pub fn load_corpus_dir(dir: &Path) -> Result<LoadedCorpus, String> {
    use std::fs;

    use griff_core::generation_input::{corpus_material, prepare_chunk};
    use griff_core::import::import_score_auto;

    let entries =
        fs::read_dir(dir).map_err(|e| format!("cannot read corpus dir {}: {e}", dir.display()))?;
    let mut names: Vec<String> = entries
        .filter_map(Result::ok)
        .filter_map(|e| e.file_name().to_str().map(ToOwned::to_owned))
        .filter(|n| n.ends_with(".chunk.json"))
        .collect();
    names.sort_unstable();

    let mut loaded = Vec::new();
    let mut skipped = Vec::new();
    let mut sources: Vec<SourceTab> = Vec::new();

    for name in names {
        let Some((meta, bytes)) = read_record(dir, &name) else {
            skipped.push(name);
            continue;
        };
        let Ok(source) = import_score_auto(&bytes) else {
            skipped.push(name);
            continue;
        };
        let filename = meta.source.filename.clone();
        let Some(chunk) = prepare_chunk(meta, &source) else {
            skipped.push(name);
            continue;
        };
        if !sources.iter().any(|s| s.name == filename) {
            sources.push(SourceTab {
                name: filename,
                bytes,
            });
        }
        loaded.push(chunk);
    }

    Ok(LoadedCorpus {
        material: corpus_material(loaded, skipped),
        sources,
    })
}

/// Reads one record and the bytes of the tab it names. `None` when either is
/// missing or unparseable.
#[cfg(not(target_arch = "wasm32"))]
fn read_record(dir: &Path, record: &str) -> Option<(ChunkMeta, Vec<u8>)> {
    use std::fs;

    let meta: ChunkMeta = serde_json::from_str(&fs::read_to_string(dir.join(record)).ok()?).ok()?;
    let bytes = fs::read(dir.join(&meta.source.filename)).ok()?;
    Some((meta, bytes))
}

/// The provenance stamped next to a kept candidate: everything needed to
/// reproduce it exactly with `griff generate` (or another cockpit run).
#[derive(Debug, serde::Serialize)]
pub struct KeptProvenance<'a> {
    /// The tab the pass was seeded from.
    pub source: &'a str,
    /// Whether a corpus supplied templates / references / gesture.
    pub corpus: bool,
    /// The ask.
    pub seed: u64,
    /// Bars generated.
    pub bars: usize,
    /// Seed variants per strategy.
    pub variants_per_strategy: usize,
    /// Whether the gesture ask was carved.
    pub gesture: bool,
    /// The candidate's strategy.
    pub strategy: &'a str,
    /// The derived variant seed — the candidate's reproduction key within the
    /// set.
    pub variant_seed: u64,
    /// Its 1-based rank in the reranked set (1 is what `griff generate` writes).
    pub rank: usize,
    /// Its weighted aggregate.
    pub aggregate: f64,
    /// Each rerank axis and its value.
    pub axes: Vec<(&'static str, f64)>,
}

/// The one conversion from a captured run and an immutable row to the Keep
/// sidecar.
///
/// The history provenance and the sidecar are two renderings of the same run,
/// never two hand-written interpretations of it.
///
/// Every request/input field comes from `context` (captured when the set was
/// produced); only the candidate's own result comes from `row`. `corpus` here
/// means the corpus **actually contributed**, not that one was attached.
#[must_use]
pub fn kept_provenance<'a>(
    context: &'a GenerateRunContext,
    row: &'a CandidateRow,
) -> KeptProvenance<'a> {
    KeptProvenance {
        source: context.source.as_deref().unwrap_or("displayed score"),
        corpus: !context.corpus.is_seed_only(),
        seed: context.seed,
        bars: context.bars,
        variants_per_strategy: context.variants_per_strategy,
        gesture: context.corpus.gesture,
        strategy: &row.strategy,
        variant_seed: row.variant_seed,
        rank: row.rank,
        aggregate: row.aggregate,
        axes: row.axes.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_panel_carries_the_cli_generate_defaults() {
        let panel = GeneratePanel::new();
        let ask = panel.ask();
        assert_eq!(ask.seed, 0, "griff generate --seed default");
        assert_eq!(ask.bars, 8, "griff generate --bars default");
        assert_eq!(
            ask.variants_per_strategy, 2,
            "griff generate --candidates default",
        );
        assert!(ask.gesture, "gesture is on unless --no-gesture");
        assert!(panel.set().is_none(), "nothing generated yet");
    }

    #[test]
    fn no_source_pick_means_seed_from_the_displayed_score() {
        let mut panel = GeneratePanel::new();
        assert!(panel.source_tab().is_none(), "no corpus tabs loaded");
        panel.sources.push(SourceTab {
            name: "riff.gp5".to_owned(),
            bytes: vec![1, 2, 3],
        });
        panel.source = Some(0);
        assert_eq!(
            panel.source_tab().map(|s| s.name.as_str()),
            Some("riff.gp5"),
        );
        panel.source = Some(9);
        assert!(
            panel.source_tab().is_none(),
            "an out-of-range pick falls back to the displayed score, it does not panic",
        );
    }
}
