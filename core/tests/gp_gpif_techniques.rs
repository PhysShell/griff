//! Red → GPIF (GP6 `.gpx` / GP7 `.gp`) imports keep tapping and read
//! hammer-on/pull-off with the GP3/4/5 semantics.
//!
//! Two losses in the `guitarpro` 0.4.2 GPIF conversion, measured on a 410-file
//! corpus (31 of 145 GPIF files carry tapping):
//!
//! - the `Tapped` note property is never read (the beat's tap effect is left a
//!   placeholder), so no GP6/GP7 tapping reached griff;
//! - `HopoOrigin` and `HopoDestination` both set the one legacy `hammer` flag.
//!   GP3/4/5 set it on the origin note only, and griff's `HammerOn` span
//!   follows that note. On GPIF input the destination got a span too, so two
//!   adjacent independent hammer-on pairs were indistinguishable from one chain.
//!
//! The fixtures are authored GPIF text packed with the crate's public writer;
//! no copyrighted tab.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::missing_assert_message,
    clippy::indexing_slicing
)]

use griff_core::{
    event::{NoteMark, SpanTechnique},
    gp::import_gp_score,
    score::{AtomEvent, Score},
};
use guitarpro::io::gpx::{compress_bcfz, pack_bcfs};
use std::fmt::Write as _;

/// One note: GPIF raw string (0 = lowest), fret, and enabled note properties.
struct GpifNote {
    string: u8,
    fret: u8,
    enabled: &'static [&'static str],
}

const fn n(string: u8, fret: u8, enabled: &'static [&'static str]) -> GpifNote {
    GpifNote {
        string,
        fret,
        enabled,
    }
}

/// A one-track Standard-E GPIF, one quarter-note beat per entry of `beats`.
fn gpif(beats: &[Vec<GpifNote>]) -> String {
    let flat: Vec<(usize, &GpifNote)> = beats
        .iter()
        .enumerate()
        .flat_map(|(b, notes)| notes.iter().map(move |note| (b, note)))
        .collect();
    let mut note_xml = String::new();
    for (id, (_, note)) in flat.iter().enumerate() {
        write!(
            note_xml,
            r#"<Note id="{id}"><Properties><Property name="Fret"><Fret>{}</Fret></Property><Property name="String"><String>{}</String></Property>"#,
            note.fret, note.string
        )
        .expect("writing to a String");
        for p in note.enabled {
            write!(note_xml, r#"<Property name="{p}"><Enable /></Property>"#)
                .expect("writing to a String");
        }
        note_xml.push_str("</Properties></Note>");
    }
    let mut beat_xml = String::new();
    for b in 0..beats.len() {
        let ids: Vec<String> = flat
            .iter()
            .enumerate()
            .filter(|(_, (beat, _))| *beat == b)
            .map(|(id, _)| id.to_string())
            .collect();
        write!(
            beat_xml,
            r#"<Beat id="{b}"><Rhythm ref="0"/><Notes>{}</Notes></Beat>"#,
            ids.join(" ")
        )
        .expect("writing to a String");
    }
    let beat_ids: Vec<String> = (0..beats.len()).map(|b| b.to_string()).collect();
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<GPIF>
<GPVersion>7</GPVersion>
<Score><Title><![CDATA[synthetic]]></Title></Score>
<MasterTrack><Tracks>0</Tracks></MasterTrack>
<Tracks>
<Track id="0">
<Name><![CDATA[Guitar]]></Name>
<ShortName><![CDATA[gtr]]></ShortName>
<Properties><Property name="Tuning"><Pitches>40 45 50 55 59 64</Pitches></Property></Properties>
</Track>
</Tracks>
<MasterBars>
<MasterBar><Time>4/4</Time><Bars>0</Bars></MasterBar>
</MasterBars>
<Bars><Bar id="0"><Voices>0 -1 -1 -1</Voices></Bar></Bars>
<Voices><Voice id="0"><Beats>{}</Beats></Voice></Voices>
<Beats>{beat_xml}</Beats>
<Notes>{note_xml}</Notes>
<Rhythms><Rhythm id="0"><NoteValue>Quarter</NoteValue></Rhythm></Rhythms>
</GPIF>"#,
        beat_ids.join(" ")
    )
}

fn import(beats: &[Vec<GpifNote>]) -> Score {
    let xml = gpif(beats);
    import_gp_score(&compress_bcfz(&pack_bcfs("score.gpif", xml.as_bytes())))
        .expect("synthetic GPIF imports")
}

/// Per event group: whether it carries a hammer-on span, and each note's tap mark.
fn groups(score: &Score) -> Vec<(bool, Vec<bool>)> {
    score.tracks[0].voices[0]
        .event_groups
        .iter()
        .map(|g| {
            let hammer = g
                .technique_spans
                .iter()
                .any(|s| s.technique == SpanTechnique::HammerOn);
            let taps = g
                .atoms
                .iter()
                .filter_map(|a| match a {
                    AtomEvent::Note(note) => Some(note.marks.contains(NoteMark::Tap)),
                    AtomEvent::Rest(_) => None,
                })
                .collect();
            (hammer, taps)
        })
        .collect()
}

#[test]
fn only_the_hopo_origin_carries_the_hammer_span() {
    // D string: 5 hammered to 7, then an unrelated 5.
    let score = import(&[
        vec![n(2, 5, &["HopoOrigin"])],
        vec![n(2, 7, &["HopoDestination"])],
        vec![n(2, 5, &[])],
    ]);
    let hammers: Vec<bool> = groups(&score).iter().map(|g| g.0).collect();
    assert_eq!(hammers, vec![true, false, false]);
}

#[test]
fn a_hopo_chain_marks_every_origin_and_not_the_last_destination() {
    // 5 → 7 → 5: the middle note is destination and origin.
    let score = import(&[
        vec![n(2, 5, &["HopoOrigin"])],
        vec![n(2, 7, &["HopoDestination", "HopoOrigin"])],
        vec![n(2, 5, &["HopoDestination"])],
    ]);
    let hammers: Vec<bool> = groups(&score).iter().map(|g| g.0).collect();
    assert_eq!(hammers, vec![true, true, false]);
}

#[test]
fn two_adjacent_hopo_pairs_stay_two_pairs() {
    // 5 → 7, then 5 → 7: no hammer span may join the first pair to the second.
    let score = import(&[
        vec![n(2, 5, &["HopoOrigin"])],
        vec![n(2, 7, &["HopoDestination"])],
        vec![n(2, 5, &["HopoOrigin"])],
        vec![n(2, 7, &["HopoDestination"])],
    ]);
    let hammers: Vec<bool> = groups(&score).iter().map(|g| g.0).collect();
    assert_eq!(hammers, vec![true, false, true, false]);
}

#[test]
fn tapped_notes_are_imported_as_taps() {
    // 5, tap 12, 5 on the D string.
    let score = import(&[
        vec![n(2, 5, &[])],
        vec![n(2, 12, &["Tapped"])],
        vec![n(2, 5, &[])],
    ]);
    let taps: Vec<Vec<bool>> = groups(&score).into_iter().map(|g| g.1).collect();
    assert_eq!(taps, vec![vec![false], vec![true], vec![false]]);
}

#[test]
fn a_tapped_note_in_a_chord_taps_its_beat() {
    // Guitar Pro's legacy model holds tapping per beat, as GP3/4/5 do: a chord
    // beat with one tapped note marks the beat's notes. Other beats stay untouched.
    let score = import(&[vec![n(2, 5, &[]), n(3, 7, &["Tapped"])], vec![n(2, 5, &[])]]);
    let taps: Vec<Vec<bool>> = groups(&score).into_iter().map(|g| g.1).collect();
    assert_eq!(taps, vec![vec![true, true], vec![false]]);
}
