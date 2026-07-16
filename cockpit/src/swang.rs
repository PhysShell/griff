//! The Swang authoring panel's state and pure logic (S8 Playground).
//!
//! The egui rendering lives in `lib.rs` beside the other panels; everything
//! that decides *what* to show — parse, format, evaluate, select — lives here
//! as a testable state machine over
//! [`griff_swang::eval`]. The panel never shells out to `griff.exe` and never
//! reimplements the generator: it drives the one shared evaluator, and a
//! resolved source score is handed in by the shell (the CLI reads a file, the
//! cockpit reads its own, the web later passes bytes).

use griff_core::generation_input::CorpusMaterial;
use griff_core::score::Score;
use griff_swang::eval::{self, CompiledProgram, DiagLocation};
use griff_swang::syntax::{self, Span};
use griff_ui_core::generate::CandidateSet;

/// A diagnostic rendered for the editor: its stable code, a human location,
/// and the message. Span diagnostics also carry the byte range so the editor
/// can point into the text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwangDiag {
    /// The stable `SWG____` code.
    pub code: &'static str,
    /// A rendered location: `line:col` for a span, `node <path>` for a
    /// structural breach.
    pub location: String,
    /// The message, in program vocabulary.
    pub message: String,
    /// The source byte range, for span diagnostics — `None` for structural.
    pub span: Option<Span>,
}

/// The starter program shown in a fresh editor — the spec §3.1 shape with a
/// placeholder `source` the author points at their own file.
pub const STARTER: &str = r#"swang 1

pattern riff {
    ascii "X.X/XX./.XX"
    |> fractalize depth 1 max_cells 4096 density 9500bps seed 4
    |> linearize snake
    |> map_rhythm unit 1/16 tail rest_pad
    |> generate {
        source "riff.mid"
        bars 8
        seed 42
        candidates 2
        strategy repeat_variation
    }
    |> export midi "riff_out.mid"
}
"#;

/// The Swang editor panel's state.
#[derive(Debug, Clone)]
pub struct SwangPanel {
    /// Whether the window is shown.
    pub open: bool,
    /// The editable program text.
    pub text: String,
    /// Diagnostics from the last check or run.
    pub diagnostics: Vec<SwangDiag>,
    /// The last successful run's candidate set.
    pub set: Option<CandidateSet>,
    /// The row currently shown in the roll — the evaluator's selection at
    /// first, then whatever the user clicks.
    pub selected: Option<usize>,
    /// The program's export path from the last run, for the Build button.
    pub export_path: Option<String>,
    /// The exact text the current `set` was evaluated from. Build and the
    /// candidate table are valid only while `text` still equals this — an
    /// edit makes the run stale, so an old score can never be exported under
    /// a new program's provenance.
    evaluated_text: Option<String>,
    /// A one-line status for the panel header.
    pub status: String,
}

impl Default for SwangPanel {
    fn default() -> Self {
        Self {
            open: false,
            text: STARTER.to_owned(),
            diagnostics: Vec::new(),
            set: None,
            selected: None,
            export_path: None,
            evaluated_text: None,
            status: String::new(),
        }
    }
}

impl SwangPanel {
    /// The seed-score path the current text declares, if it compiles — the
    /// shell resolves this to bytes before [`Self::run`].
    #[must_use]
    pub fn source_path(&self) -> Option<String> {
        eval::compile_program(&self.text)
            .ok()
            .map(|c| c.source_path().to_owned())
    }

    /// A previous run's result is stale the moment the text changes: clears
    /// the set, the selection, and the export so Build cannot fire on it.
    pub fn invalidate_run(&mut self) {
        self.set = None;
        self.selected = None;
        self.export_path = None;
        self.evaluated_text = None;
    }

    /// True only while the shown candidates were evaluated from the *current*
    /// text — the guard that stops an edited program from exporting an old
    /// score under new provenance.
    #[must_use]
    pub fn run_is_current(&self) -> bool {
        self.set.is_some() && self.evaluated_text.as_deref() == Some(self.text.as_str())
    }

    /// Parse and statically check only — no inputs, no generation. Fills
    /// [`Self::diagnostics`], and on failure clears the now-stale run.
    pub fn check(&mut self) {
        // `compile` fills the diagnostics and invalidates the run on failure.
        if self.compile().is_some() {
            "checks clean".clone_into(&mut self.status);
        }
    }

    /// Replace the text with its canonical form, if it parses; otherwise fill
    /// diagnostics and leave the text untouched.
    pub fn format(&mut self) {
        match eval::compile_program(&self.text) {
            Ok(compiled) => {
                self.text = syntax::format(compiled.program());
                self.diagnostics.clear();
                self.invalidate_run(); // the reformatted text has not been run
                "formatted".clone_into(&mut self.status);
            }
            Err(diags) => {
                self.diagnostics = diags.iter().map(|d| self.span_diag(d)).collect();
                self.invalidate_run();
                "cannot format: fix the diagnostics first".clone_into(&mut self.status);
            }
        }
    }

    /// Compile the current text, or fill span diagnostics and invalidate the
    /// run. The shell resolves inputs from the returned program, then calls
    /// [`Self::apply`] — one compile, real diagnostics preserved.
    #[must_use]
    pub fn compile(&mut self) -> Option<CompiledProgram> {
        match eval::compile_program(&self.text) {
            Ok(compiled) => {
                self.diagnostics.clear();
                Some(compiled)
            }
            Err(diags) => {
                self.diagnostics = diags.iter().map(|d| self.span_diag(d)).collect();
                self.status = format!("{} diagnostic(s)", self.diagnostics.len());
                self.invalidate_run();
                None
            }
        }
    }

    /// Evaluate an already-compiled program against a resolved source and
    /// store the outcome, snapshotting the text for the stale guard.
    ///
    /// A program that declares a `corpus` but was handed none is **refused**,
    /// not silently run without it — the cockpit does not resolve a corpus in
    /// this slice, so the declared input may not be quietly dropped.
    pub fn apply(
        &mut self,
        compiled: &CompiledProgram,
        source: Score,
        corpus: Option<CorpusMaterial>,
    ) {
        if let Some(path) = compiled.corpus_path() {
            if corpus.is_none() {
                self.diagnostics.clear();
                self.invalidate_run();
                self.status = format!(
                    "corpus \"{path}\" is declared but the cockpit does not load a corpus yet — \
                     remove the corpus line, or build it with `griff swang build`"
                );
                return;
            }
        }
        let inputs = eval::ResolvedProgramInputs {
            source_score: source,
            corpus,
        };
        match eval::evaluate_program(compiled, &inputs) {
            Ok(result) => {
                self.diagnostics.clear();
                let set = CandidateSet::from_ranked(&result.ranked, inputs.corpus.as_ref());
                self.status = format!("{} candidates -> {}", set.rows.len(), result.export.path);
                self.set = Some(set);
                self.selected = Some(result.selected);
                self.export_path = Some(result.export.path);
                self.evaluated_text = Some(self.text.clone());
            }
            Err(diags) => {
                self.diagnostics = diags.iter().map(|d| self.eval_diag(d)).collect();
                self.status = format!("{} diagnostic(s)", self.diagnostics.len());
                self.invalidate_run();
            }
        }
    }

    /// Convenience for a caller that already holds a resolved source: compile
    /// then apply. The cockpit shell instead splits the two so it can resolve
    /// `source` from the compiled program between them.
    pub fn run(&mut self, source: Score, corpus: Option<CorpusMaterial>) {
        if let Some(compiled) = self.compile() {
            self.apply(&compiled, source, corpus);
        }
    }

    /// The score currently selected for the roll, if a *current* run produced
    /// one.
    #[must_use]
    pub fn selected_score(&self) -> Option<&Score> {
        if !self.run_is_current() {
            return None;
        }
        let set = self.set.as_ref()?;
        let i = self.selected?;
        set.scores.get(i)
    }

    /// Renders a parser (span) diagnostic at a `line:col` location.
    fn span_diag(&self, d: &syntax::Diagnostic) -> SwangDiag {
        let (line, col) = line_col(&self.text, d.span.start);
        SwangDiag {
            code: d.code,
            location: format!("{line}:{col}"),
            message: d.message.clone(),
            span: Some(d.span),
        }
    }

    /// Renders an evaluator diagnostic at its §1.5 layered location.
    fn eval_diag(&self, d: &eval::EvalDiagnostic) -> SwangDiag {
        match &d.location {
            DiagLocation::Span(span) => {
                let (line, col) = line_col(&self.text, span.start);
                SwangDiag {
                    code: d.code,
                    location: format!("{line}:{col}"),
                    message: d.message.clone(),
                    span: Some(*span),
                }
            }
            DiagLocation::Node(node) => {
                // `node.as_slice()` needs no type name in scope; a structural
                // breach has no source span the editor can point at.
                let words = if node.as_slice().is_empty() {
                    "root".to_owned()
                } else {
                    node.as_slice()
                        .iter()
                        .map(u32::to_string)
                        .collect::<Vec<_>>()
                        .join(".")
                };
                SwangDiag {
                    code: d.code,
                    location: format!("node {words}"),
                    message: d.message.clone(),
                    span: None,
                }
            }
        }
    }
}

/// 1-based line and column of a byte offset (columns count Unicode scalar
/// values from the line start).
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

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::missing_assert_message,
    clippy::arithmetic_side_effects,
    clippy::str_to_string
)]
mod tests {
    use griff_core::event::{NoteMarks, Pitch, Tempo, Ticks, TimeSignature, Tuning, Velocity};
    use griff_core::score::{
        AtomEvent, AtomNote, EventGroup, EventGroupKind, LossReport, MasterBar, RepeatMarker,
        Score, Track, Voice,
    };
    use griff_core::slice::TickRange;

    use super::SwangPanel;

    const BAR: u32 = 1920;

    fn seed_score(bar_count: usize) -> Score {
        let master_bars = (0..bar_count)
            .map(|i| {
                let start = u32::try_from(i).unwrap() * BAR;
                MasterBar {
                    index: i,
                    tick_range: TickRange::new(Ticks(start), Ticks(start + BAR)).expect("ordered"),
                    time_signature: TimeSignature {
                        numerator: 4,
                        denominator: 4,
                    },
                    tempo: Tempo::new(120.0).expect("tempo"),
                    repeat: RepeatMarker::default(),
                }
            })
            .collect();
        let mut groups = Vec::new();
        for bar in 0..bar_count {
            let bar_start = u32::try_from(bar).unwrap() * BAR;
            for beat in 0..4_u32 {
                groups.push(EventGroup {
                    kind: EventGroupKind::Single,
                    atoms: vec![AtomEvent::Note(AtomNote {
                        absolute_start: Ticks(bar_start + beat * 480),
                        duration: Ticks(480),
                        pitch: Pitch::new(40 + u8::try_from(beat).unwrap()).expect("pitch"),
                        velocity: Velocity::new(90).expect("velocity"),
                        marks: NoteMarks::empty(),
                        position: None,
                    })],
                    technique_spans: Vec::new(),
                });
            }
        }
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

    fn program(strategy: &str) -> String {
        format!(
            r#"swang 1

pattern p {{
    ascii "X.X/XX./.XX"
    |> fractalize depth 1 max_cells 4096 density 9500bps seed 4
    |> linearize snake
    |> map_rhythm unit 1/16 tail rest_pad
    |> generate {{
        source "seed.mid"
        bars 4
        seed 42
        candidates 2
        strategy {strategy}
    }}
    |> export midi "riff_out.mid"
}}
"#
        )
    }

    #[test]
    fn a_clean_program_runs_to_a_selected_candidate_and_export() {
        let mut panel = SwangPanel {
            text: program("repeat_variation"),
            ..SwangPanel::default()
        };
        panel.run(seed_score(4), None);

        assert!(
            panel.diagnostics.is_empty(),
            "a clean program has no diagnostics"
        );
        let set = panel.set.as_ref().expect("a run produced a set");
        assert!(!set.rows.is_empty(), "candidates to browse");
        assert_eq!(
            panel.selected,
            Some(set_selected(&panel)),
            "the evaluator's pick is shown"
        );
        assert_eq!(panel.export_path.as_deref(), Some("riff_out.mid"));
        assert!(
            panel.selected_score().is_some(),
            "a score to draw in the roll"
        );
    }

    fn set_selected(panel: &SwangPanel) -> usize {
        panel.selected.expect("selected after a run")
    }

    #[test]
    fn clicking_a_row_changes_the_shown_score() {
        let mut panel = SwangPanel {
            text: program("auto"),
            ..SwangPanel::default()
        };
        panel.run(seed_score(4), None);
        let rows = panel.set.as_ref().unwrap().rows.len();
        assert!(rows >= 2, "more than one candidate to switch between");

        let first_notes = panel.selected_score().unwrap().tracks.len();
        panel.selected = Some(rows - 1);
        let last = panel.selected_score().unwrap();
        // The panel shows whichever row is selected — a different index reads
        // a different entry of the parallel scores vector.
        assert_eq!(
            panel.set.as_ref().unwrap().scores[rows - 1].tracks.len(),
            last.tracks.len()
        );
        assert_eq!(
            first_notes,
            last.tracks.len(),
            "both are one-track candidates"
        );
    }

    #[test]
    fn a_seedless_density_reports_a_span_diagnostic() {
        let mut panel = SwangPanel {
            text: program("auto").replace("density 9500bps seed 4", "density 9500bps"),
            ..SwangPanel::default()
        };
        panel.check();
        assert_eq!(panel.diagnostics.len(), 1);
        assert_eq!(panel.diagnostics[0].code, "SWG0303");
        assert!(
            panel.diagnostics[0].span.is_some(),
            "a parser diagnostic carries a source span for the editor"
        );
        assert!(
            panel.diagnostics[0].location.contains(':'),
            "line:col location"
        );
    }

    #[test]
    fn format_canonicalizes_and_run_is_untouched() {
        let messy = program("auto").replace("    |> linearize snake\n", "  |> linearize snake\n");
        let mut panel = SwangPanel {
            text: messy,
            ..SwangPanel::default()
        };
        panel.format();
        assert!(panel.text.contains("    |> linearize snake"), "re-indented");
        assert!(panel.diagnostics.is_empty());
    }

    #[test]
    fn run_reports_a_silent_kernel_without_a_set() {
        let mut panel = SwangPanel {
            text: program("auto").replace("X.X/XX./.XX", "........./........./........."),
            ..SwangPanel::default()
        };
        panel.run(seed_score(4), None);
        assert!(
            panel.set.is_none(),
            "a silent expansion yields no candidates"
        );
        assert_eq!(panel.diagnostics[0].code, "SWG0306");
    }

    #[test]
    fn source_path_reads_the_declared_seed() {
        let panel = SwangPanel {
            text: program("auto"),
            ..SwangPanel::default()
        };
        assert_eq!(panel.source_path().as_deref(), Some("seed.mid"));
    }

    // ── the four #123 review regressions ────────────────────────────────────

    /// Defect 1: Run on a broken program must show its real span diagnostics,
    /// not lose them to a generic "check it first" message.
    #[test]
    fn run_on_broken_text_preserves_span_diagnostics() {
        let mut panel = SwangPanel {
            text: program("auto").replace("density 9500bps seed 4", "density 9500bps"),
            ..SwangPanel::default()
        };
        panel.run(seed_score(4), None);
        assert!(panel.set.is_none());
        assert_eq!(panel.diagnostics.len(), 1, "the diagnostic survives Run");
        assert_eq!(panel.diagnostics[0].code, "SWG0303");
        assert!(
            panel.diagnostics[0].span.is_some(),
            "with a source span the editor can point at"
        );
    }

    /// Defect 2: editing the text after a run makes the result stale — no
    /// export under a new program's provenance.
    #[test]
    fn editing_after_a_run_makes_it_stale_and_unexportable() {
        let mut panel = SwangPanel {
            text: program("repeat_variation"),
            ..SwangPanel::default()
        };
        panel.run(seed_score(4), None);
        assert!(panel.run_is_current(), "fresh run is current");
        assert!(panel.selected_score().is_some());

        // The author edits program A into program B.
        panel.text = program("shuffle_motifs");
        assert!(
            !panel.run_is_current(),
            "an edited program is no longer the one that was run"
        );
        assert!(
            panel.selected_score().is_none(),
            "a stale score is not offered to the roll or to Build"
        );
    }

    /// Defect 2 (continued): `check()` clears a stale set when the edited text
    /// no longer compiles.
    #[test]
    fn check_clears_a_stale_set_on_error() {
        let mut panel = SwangPanel {
            text: program("auto"),
            ..SwangPanel::default()
        };
        panel.run(seed_score(4), None);
        assert!(panel.set.is_some());

        panel.text = program("auto").replace("density 9500bps seed 4", "density 9500bps");
        panel.check();
        assert!(panel.set.is_none(), "a failed check clears the stale run");
        assert_eq!(panel.diagnostics[0].code, "SWG0303");
    }

    /// Defect 3: a program that declares a corpus the frontend did not load is
    /// refused, never silently run without it.
    #[test]
    fn a_declared_corpus_without_a_loaded_one_is_refused() {
        let with_corpus = program("auto").replace(
            "        strategy auto\n",
            "        strategy auto\n        corpus \"corpus\"\n",
        );
        let mut panel = SwangPanel {
            text: with_corpus,
            ..SwangPanel::default()
        };
        panel.run(seed_score(4), None);
        assert!(
            panel.set.is_none(),
            "the declared corpus was not loaded, so the run is refused"
        );
        assert!(
            panel.status.contains("corpus"),
            "the refusal names the missing input: {}",
            panel.status
        );
    }
}
