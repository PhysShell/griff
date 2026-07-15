//! The cockpit's voice — the first, deliberately basic playback synthesis.
//!
//! Slice 1 moved a visual playhead across the roll but stayed silent (the crate
//! doc still reads "the cockpit does not synthesise audio"). This module gives
//! it the smallest possible voice: as the playhead crosses a note's onset, a
//! short sine "blip" at that note's pitch. No sample library, no envelope
//! shaping beyond a click-free attack/decay, no per-track timbre — just enough
//! that a run you can *see* is a run you can also *hear*.
//!
//! Only the browser front (ADR-0027's canonical web target) makes sound: it
//! drives the platform mixer through the Web Audio API, which needs no new
//! crate — just a few more `web-sys` features. The native `eframe` window keeps
//! a zero-cost no-op [`Synth`]; wiring a desktop audio backend (cpal/rodio) is a
//! later slice's call, not this one's.
//!
//! The pitch→frequency map ([`midi_to_hz`]) is pure and lives outside the cfg
//! split so it stays unit-testable on the native target.

/// Equal-temperament frequency (Hz) of a MIDI pitch, tuned to A4 = 440 Hz
/// (pitch 69). `hz = 440 · 2^((pitch − 69) / 12)`.
///
/// The cockpit only ever feeds this the pitches its own view already holds
/// (`NoteRect::pitch`, a `u8` in 0–127), so the whole MIDI range maps to an
/// audible, finite frequency.
#[must_use]
pub fn midi_to_hz(pitch: u8) -> f64 {
    let semitones_from_a4 = f64::from(i16::from(pitch) - 69);
    440.0 * (semitones_from_a4 / 12.0).exp2()
}

#[cfg(target_arch = "wasm32")]
pub use web::Synth;

#[cfg(not(target_arch = "wasm32"))]
pub use native::Synth;

// ── web (wasm32): a Web Audio blip per note onset ────────────────────────────
#[cfg(target_arch = "wasm32")]
mod web {
    use web_sys::{AudioContext, GainNode, OscillatorNode, OscillatorType};

    use super::midi_to_hz;

    /// Peak gain of one blip. Kept low so a chord — several oscillators started
    /// on the same frame — sums well short of clipping the destination.
    const PEAK_GAIN: f32 = 0.14;
    /// Seconds of linear fade-in — long enough to kill the click a hard start
    /// would make, short enough to still read as an attack.
    const ATTACK_SECS: f64 = 0.005;
    /// Total seconds a blip sounds, attack included. A note's own duration is
    /// ignored: Slice-2 sustain is out of scope, so every onset is one short pip.
    const BLIP_SECS: f64 = 0.18;

    /// The browser's voice: a lazily-opened [`AudioContext`] the playhead pipes
    /// note onsets into. Created on the first blip — which only ever fires while
    /// the user is playing back, i.e. after the click or Space that satisfies
    /// the browser's autoplay gesture requirement.
    #[derive(Debug, Default)]
    pub struct Synth {
        ctx: Option<AudioContext>,
    }

    impl Synth {
        /// A silent synth; the audio context opens on the first note.
        #[must_use]
        pub const fn new() -> Self {
            Self { ctx: None }
        }

        /// Sounds a single note at `pitch`. Best-effort: every Web Audio call
        /// can fail (a refused context, a closed tab), and audio must never
        /// panic the render loop, so a failure is simply silence.
        pub fn note_on(&mut self, pitch: u8) {
            if let Some(ctx) = self.context() {
                drop(blip(ctx, midi_to_hz(pitch)));
            }
        }

        /// The audio context, opened on first use. A browser that denies one
        /// (or has no Web Audio at all) leaves this `None` forever — the cockpit
        /// then simply plays silently.
        fn context(&mut self) -> Option<&AudioContext> {
            if self.ctx.is_none() {
                self.ctx = AudioContext::new().ok();
            }
            // A context suspended by the autoplay policy resumes once a gesture
            // has landed; the returned promise is fire-and-forget.
            if let Some(ctx) = &self.ctx {
                drop(ctx.resume());
            }
            self.ctx.as_ref()
        }
    }

    /// Schedules one sine blip on `ctx` at `freq` Hz: a fresh oscillator through
    /// a gain node whose envelope ramps up over [`ATTACK_SECS`] and decays to
    /// near-zero by [`BLIP_SECS`], then both nodes stop and are dropped.
    fn blip(ctx: &AudioContext, freq: f64) -> Result<(), wasm_bindgen::JsValue> {
        let now = ctx.current_time();
        let osc: OscillatorNode = ctx.create_oscillator()?;
        let gain: GainNode = ctx.create_gain()?;

        osc.set_type(OscillatorType::Sine);
        osc.frequency().set_value(freq as f32);

        // Ramp 0 → peak → ~0. The decay target is a small positive number, not
        // 0, because an exponential ramp cannot reach zero.
        let env = gain.gain();
        env.set_value_at_time(0.0, now)?;
        env.linear_ramp_to_value_at_time(PEAK_GAIN, now + ATTACK_SECS)?;
        env.exponential_ramp_to_value_at_time(0.0001, now + BLIP_SECS)?;

        osc.connect_with_audio_node(&gain)?;
        gain.connect_with_audio_node(&ctx.destination())?;
        osc.start_with_when(now)?;
        osc.stop_with_when(now + BLIP_SECS)?;
        Ok(())
    }
}

// ── native: silence ──────────────────────────────────────────────────────────
#[cfg(not(target_arch = "wasm32"))]
mod native {
    /// The native window's voice — none yet. A desktop audio backend is a later
    /// slice's decision; until then the native cockpit plays silently and this
    /// carries no state.
    #[derive(Debug, Clone, Copy, Default)]
    pub struct Synth;

    impl Synth {
        /// A silent synth — the native constructor, mirroring the web one.
        #[must_use]
        pub const fn new() -> Self {
            Self
        }

        /// No-op: the native target synthesises nothing (see the module doc).
        #[allow(clippy::unused_self)]
        pub const fn note_on(&mut self, _pitch: u8) {}
    }
}

#[cfg(test)]
mod tests {
    use super::midi_to_hz;

    #[test]
    fn a4_is_concert_pitch() {
        assert!((midi_to_hz(69) - 440.0).abs() < 1e-9);
    }

    #[test]
    fn an_octave_up_doubles_the_frequency() {
        assert!((midi_to_hz(81) - 880.0).abs() < 1e-9);
        assert!((midi_to_hz(57) - 220.0).abs() < 1e-9);
    }

    #[test]
    fn middle_c_matches_the_standard() {
        // C4 (MIDI 60) ≈ 261.63 Hz.
        assert!((midi_to_hz(60) - 261.625_565).abs() < 1e-3);
    }
}
