#![allow(
    clippy::pedantic,
    clippy::restriction,
    clippy::nursery,
    clippy::complexity
)]
//! Phase-1 HarmonicContext non-regression dump (identical on both arms).
//!
//! For every track, runs the production `complement::analyze_part` and prints its
//! `harmony` verdict — Some/None, `tonic_pitch_class`, `mode`, and the exact bit
//! pattern of `scale_fit` — so the before/after arms can be compared bit-for-bit.
//! Uses only long-stable public API present on both arms.
use griff_core::complement::analyze_part;
use griff_core::import::import_score_auto;
use std::path::Path;

fn main() -> Result<(), String> {
    let input = std::env::args()
        .nth(1)
        .ok_or("usage: harmony_dump <input>")?;
    let bytes = std::fs::read(&input).map_err(|e| format!("read '{input}': {e}"))?;
    let score = import_score_auto(&bytes).map_err(|e| format!("import '{input}': {e}"))?;
    let iname = Path::new(&input)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .replace('"', "'");
    for i in 0..score.tracks.len() {
        match analyze_part(&score, i) {
            Ok(profile) => match &profile.harmony {
                Some(h) => println!(
                    "{{\"input\":\"{iname}\",\"track\":{i},\"harmony\":\"some\",\"tonic_pc\":{},\"mode\":\"{:?}\",\"scale_fit_bits\":{}}}",
                    h.tonic_pitch_class, h.mode, h.scale_fit.to_bits()
                ),
                None => println!("{{\"input\":\"{iname}\",\"track\":{i},\"harmony\":\"none\"}}"),
            },
            Err(e) => println!("{{\"input\":\"{iname}\",\"track\":{i},\"harmony\":\"err\",\"e\":\"{e:?}\"}}"),
        }
    }
    Ok(())
}
