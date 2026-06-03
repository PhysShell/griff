//! Engine-derived analysis that backs the interactive preview.
//!
//! Named sections (from [`griff_core::classify`]) and structure metrics (from
//! [`griff_core::structure`]). Pure and headless-testable.

use griff_core::classify::{bar_features_across_voices, classify_bar, BarClass};
use griff_core::score::{AtomEvent, Score, Voice};
use griff_core::structure::{measure_structure, StructureMetrics};

/// A run of consecutive bars sharing one classification — a *named section*
/// such as `Riff`, `Breakdown`, or `Solo`.
///
/// Bar range is half-open `[bar_start, bar_end)`; tick range is half-open
/// `[tick_start, tick_end)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Section {
    /// The classification shared by every bar in the run.
    pub class: BarClass,
    /// First bar index (inclusive).
    pub bar_start: usize,
    /// One past the last bar index (exclusive).
    pub bar_end: usize,
    /// Absolute onset tick of the section.
    pub tick_start: u32,
    /// Absolute end tick of the section (exclusive).
    pub tick_end: u32,
}

impl Section {
    /// Number of bars covered by the section.
    #[must_use]
    pub const fn bar_count(&self) -> usize {
        self.bar_end.saturating_sub(self.bar_start)
    }
}

/// The analysis surfaced by the preview UI: which track drives sectioning, the
/// named sections themselves, and that track's structure metrics (if any).
#[derive(Debug, Clone)]
pub struct Analysis {
    /// Index of the track used for sectioning and metrics (the busiest track).
    pub focus_track: usize,
    /// Named sections in playback order.
    pub sections: Vec<Section>,
    /// Structure metrics for the focus track; `None` for an empty score.
    pub metrics: Option<StructureMetrics>,
}

/// Derives the [`Analysis`] for a score: pick the busiest track, classify each
/// of its bars, merge equal-class runs into sections, and measure structure.
#[must_use]
pub fn analyze(score: &Score) -> Analysis {
    let focus_track = pick_focus_track(score);
    let sections = sections_for(score, focus_track);
    let metrics = measure_structure(score, focus_track).ok();
    Analysis {
        focus_track,
        sections,
        metrics,
    }
}

/// Counts note atoms across all voices of a track.
fn voice_note_count(voice: &Voice) -> usize {
    voice
        .event_groups
        .iter()
        .flat_map(|g| &g.atoms)
        .filter(|a| matches!(a, AtomEvent::Note(_)))
        .count()
}

/// Picks the track with the most note atoms (ties resolved to the lower index).
fn pick_focus_track(score: &Score) -> usize {
    score
        .tracks
        .iter()
        .enumerate()
        .max_by_key(|(_, t)| t.voices.iter().map(voice_note_count).sum::<usize>())
        .map_or(0, |(i, _)| i)
}

/// Classifies each bar of the track's voices and merges consecutive equal
/// classes into [`Section`]s.
fn sections_for(score: &Score, track_index: usize) -> Vec<Section> {
    let Some(track) = score.tracks.get(track_index) else {
        return Vec::new();
    };
    let mut out: Vec<Section> = Vec::new();

    for bar in &score.master_bars {
        let class = classify_bar(bar_features_across_voices(&track.voices, bar.tick_range));
        let (ts, te) = (bar.tick_range.start.0, bar.tick_range.end.0);
        match out.last_mut() {
            Some(last) if last.class == class && last.bar_end == bar.index => {
                last.bar_end = bar.index.saturating_add(1);
                last.tick_end = te;
            }
            _ => out.push(Section {
                class,
                bar_start: bar.index,
                bar_end: bar.index.saturating_add(1),
                tick_start: ts,
                tick_end: te,
            }),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::unwrap_used,
        clippy::panic,
        clippy::missing_assert_message,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        clippy::str_to_string
    )]

    use super::*;
    use griff_core::event::{Pitch, Tempo, Ticks, TimeSignature, Velocity};
    use griff_core::score::{
        AtomNote, EventGroup, EventGroupKind, LossReport, MasterBar, Track, Voice,
    };
    use griff_core::slice::TickRange;

    const PPQN: u16 = 480;
    const BAR: u32 = 1920;

    fn bar_of_notes(index: usize, pitches: &[(u8, u8)]) -> (MasterBar, Vec<AtomEvent>) {
        let start = u32::try_from(index).expect("small") * BAR;
        let mb = MasterBar {
            index,
            tick_range: TickRange::new(Ticks(start), Ticks(start + BAR)).expect("ordered"),
            time_signature: TimeSignature::new(4, 4).expect("4/4"),
            tempo: Tempo::new(120.0).expect("120 BPM"),
        };
        let atoms = pitches
            .iter()
            .enumerate()
            .map(|(i, &(pitch, vel))| {
                AtomEvent::Note(AtomNote {
                    absolute_start: Ticks(start + u32::try_from(i).expect("small") * 120),
                    duration: Ticks(110),
                    pitch: Pitch::new(pitch).expect("pitch"),
                    velocity: Velocity::new(vel).expect("vel"),
                    articulation: None,
                })
            })
            .collect();
        (mb, atoms)
    }

    fn voice_from_atoms(id: u8, atoms: Vec<AtomEvent>) -> Voice {
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

    fn score_from(bars: Vec<Vec<(u8, u8)>>) -> Score {
        let mut master_bars = Vec::new();
        let mut atoms = Vec::new();
        for (i, pitches) in bars.into_iter().enumerate() {
            let (mb, bar_atoms) = bar_of_notes(i, &pitches);
            master_bars.push(mb);
            atoms.extend(bar_atoms);
        }
        Score {
            ticks_per_quarter: PPQN,
            master_bars,
            tracks: vec![Track {
                name: Some("gtr".into()),
                channel: 0,
                voices: vec![voice_from_atoms(0, atoms)],
            }],
            source_meta: None,
            loss: LossReport::new(),
        }
    }

    #[test]
    fn merges_consecutive_equal_classes() {
        // Two loud riff bars (then one clean bar) → two sections.
        let riff = vec![(40, 100), (43, 100), (45, 100), (47, 100), (40, 100)];
        let clean = vec![(60, 50), (62, 50), (64, 50)];
        let score = score_from(vec![riff.clone(), riff, clean]);
        let a = analyze(&score);
        assert_eq!(a.sections.len(), 2, "two riff bars merge, clean splits off");
        assert_eq!(a.sections.first().map(Section::bar_count), Some(2));
        assert_eq!(a.sections.last().map(|s| s.class), Some(BarClass::Clean));
    }

    #[test]
    fn empty_score_has_no_sections_and_no_metrics() {
        let score = score_from(vec![]);
        let a = analyze(&score);
        assert!(a.sections.is_empty());
        assert!(
            a.metrics.is_none(),
            "empty score yields no structure metrics"
        );
    }

    #[test]
    fn section_ticks_track_the_bars() {
        let score = score_from(vec![vec![
            (40, 100),
            (43, 100),
            (45, 100),
            (47, 100),
            (40, 100),
        ]]);
        let a = analyze(&score);
        let s = a.sections.first().expect("one section");
        assert_eq!((s.tick_start, s.tick_end), (0, BAR));
    }

    #[test]
    fn sections_classify_notes_from_all_voices_in_the_focus_track() {
        let (bar, second_voice_notes) =
            bar_of_notes(0, &[(40, 100), (43, 100), (45, 100), (47, 100), (40, 100)]);
        let score = Score {
            ticks_per_quarter: PPQN,
            master_bars: vec![bar],
            tracks: vec![Track {
                name: Some("gtr".into()),
                channel: 0,
                voices: vec![
                    voice_from_atoms(0, Vec::new()),
                    voice_from_atoms(1, second_voice_notes),
                ],
            }],
            source_meta: None,
            loss: LossReport::new(),
        };

        let a = analyze(&score);

        assert_eq!(a.focus_track, 0);
        assert_eq!(a.sections.first().map(|s| s.class), Some(BarClass::Riff));
    }
}
