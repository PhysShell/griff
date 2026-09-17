//! Red → GPIF (GP6 `.gpx` / GP7 `.gp`) imports put tuning and string numbers in
//! griff's orientation, with the right pitch.
//!
//! GPIF lists a track's tuning low string first (`<Pitches>38 45 50 55 59 64`)
//! and numbers a note's string from 0 = lowest. griff's model is string 1 =
//! highest (ADR-0018, `Tuning`). Two defects were measured on a 410-file corpus:
//!
//! - **track-level tuning** (the GP6 shape): the tuning was imported low string
//!   first and positions numbered from the low string — pitches right, every
//!   position mirrored;
//! - **staff-level tuning only** (the GP7 shape, `<Staves><Staff>`): the
//!   `guitarpro` crate never reached the staff tuning and fell back to a
//!   high-first Standard E, so pitches were wrong on every note (0.2% of
//!   344,274 GP7 notes matched their GPIF `Midi` value).
//!
//! The fixtures are authored GPIF text (no copyrighted tab), packed into a GP6
//! container with the crate's own public writer; the crate reads the same GPIF
//! model from both containers, so both tuning shapes are exercised here.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::missing_assert_message,
    clippy::indexing_slicing
)]

use griff_core::{
    event::{FretboardPosition, Pitch, Tuning},
    gp::import_gp_score,
    score::{AtomEvent, Score},
};
use guitarpro::io::gpx::{compress_bcfz, pack_bcfs};

/// Drop-D, low string first — as GPIF stores it.
const DROP_D_LOW_FIRST: &str = "38 45 50 55 59 64";

/// A one-track GPIF: two quarter notes, raw string 0 (lowest) fret 5, then raw
/// string 5 (highest) open. `track_block` places the tuning.
fn gpif(track_block: &str) -> String {
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
{track_block}
</Track>
</Tracks>
<MasterBars>
<MasterBar><Time>4/4</Time><Bars>0</Bars></MasterBar>
</MasterBars>
<Bars><Bar id="0"><Voices>0 -1 -1 -1</Voices></Bar></Bars>
<Voices><Voice id="0"><Beats>0 1</Beats></Voice></Voices>
<Beats>
<Beat id="0"><Rhythm ref="0"/><Notes>0</Notes></Beat>
<Beat id="1"><Rhythm ref="0"/><Notes>1</Notes></Beat>
</Beats>
<Notes>
<Note id="0"><Properties><Property name="Fret"><Fret>5</Fret></Property><Property name="String"><String>0</String></Property></Properties></Note>
<Note id="1"><Properties><Property name="Fret"><Fret>0</Fret></Property><Property name="String"><String>5</String></Property></Properties></Note>
</Notes>
<Rhythms><Rhythm id="0"><NoteValue>Quarter</NoteValue></Rhythm></Rhythms>
</GPIF>"#
    )
}

fn tuning_property(pitches: &str) -> String {
    format!(r#"<Property name="Tuning"><Pitches>{pitches}</Pitches></Property>"#)
}

fn import(track_block: &str) -> Score {
    let xml = gpif(track_block);
    let bytes = compress_bcfz(&pack_bcfs("score.gpif", xml.as_bytes()));
    import_gp_score(&bytes).expect("synthetic GPIF imports")
}

fn notes(score: &Score) -> Vec<(u8, Option<FretboardPosition>)> {
    score.tracks[0]
        .voices
        .iter()
        .flat_map(|v| &v.event_groups)
        .flat_map(|g| &g.atoms)
        .filter_map(|a| match a {
            AtomEvent::Note(n) => Some((n.pitch.0, n.position.map(|p| p.position))),
            AtomEvent::Rest(_) => None,
        })
        .collect()
}

fn drop_d_high_first() -> Tuning {
    Tuning::new([64, 59, 55, 50, 45, 38].map(Pitch).to_vec())
}

fn assert_drop_d_line(score: &Score) {
    assert_eq!(score.tracks[0].tuning, drop_d_high_first());
    assert_eq!(
        notes(score),
        vec![
            // Low D string (griff string 6), fret 5 → G2.
            (43, Some(FretboardPosition { string: 6, fret: 5 })),
            // High E string (griff string 1), open → E4.
            (64, Some(FretboardPosition { string: 1, fret: 0 })),
        ]
    );
    for (pitch, position) in notes(score) {
        let position = position.expect("positioned");
        assert_eq!(
            score.tracks[0].tuning.pitch_at(position),
            Some(Pitch(pitch)),
            "position and tuning agree on the pitch"
        );
    }
}

#[test]
fn track_level_tuning_is_imported_high_string_first() {
    let score = import(&format!(
        "<Properties>{}</Properties>",
        tuning_property(DROP_D_LOW_FIRST)
    ));
    assert_drop_d_line(&score);
}

#[test]
fn staff_level_tuning_is_read_and_pitches_are_right() {
    let score = import(&format!(
        "<Staves><Staff><Properties>{}</Properties></Staff></Staves>",
        tuning_property(DROP_D_LOW_FIRST)
    ));
    assert_drop_d_line(&score);
}

#[test]
fn staff_level_tuning_is_read_beside_other_track_properties() {
    // A track <Properties> block without a tuning must not hide the staff's.
    let score = import(&format!(
        r#"<Properties><Property name="Color"><Color>255 0 0</Color></Property></Properties>
<Staves><Staff><Properties>{}</Properties></Staff></Staves>"#,
        tuning_property(DROP_D_LOW_FIRST)
    ));
    assert_drop_d_line(&score);
}

#[test]
fn seven_string_tuning_keeps_every_string() {
    // B1 E2 A2 D3 G3 B3 E4, low first; raw string 0 fret 5 → E2 on griff string 7.
    let score = import(&format!(
        "<Properties>{}</Properties>",
        tuning_property("35 40 45 50 55 59 64")
    ));
    assert_eq!(
        score.tracks[0].tuning,
        Tuning::new([64, 59, 55, 50, 45, 40, 35].map(Pitch).to_vec())
    );
    let imported = notes(&score);
    assert_eq!(
        imported[0],
        (40, Some(FretboardPosition { string: 7, fret: 5 }))
    );
    // Raw string 5 of seven is B3, griff string 2.
    assert_eq!(
        imported[1],
        (59, Some(FretboardPosition { string: 2, fret: 0 }))
    );
}
