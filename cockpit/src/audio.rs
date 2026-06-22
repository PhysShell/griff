//! Playback audio for the cockpit — the placeholder oscillator synth ADR-0024 §4
//! specifies for the web front, realised in-wasm over the WebAudio API (`web-sys`,
//! no JS). As the playhead crosses a note, [`sound`] schedules a short plucked
//! sawtooth on the audio clock; the focused track is what you hear.
//!
//! Audio is the per-target playback-driver seam (ADR-0024 §4, ADR-0027 §5). Like
//! the storage seam (`save_chunk` / `opfs_save`), it is a cfg-gated free function
//! rather than a runtime trait: WebAudio on web, and a silent no-op on native
//! until a `cpal`/`midir` driver lands — same signature, so `eframe::App::ui`
//! stays target-agnostic.

/// Sounds each voice the playhead just crossed this frame. Native builds have no
/// audio backend yet, so playback is silent; the slice is accepted so the call
/// site is identical across targets.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) const fn sound(_voices: &[crate::Voice]) {}

#[cfg(target_arch = "wasm32")]
pub(crate) use wasm::sound;

#[cfg(target_arch = "wasm32")]
mod wasm {
    use std::cell::RefCell;

    use wasm_bindgen::JsValue;
    use web_sys::{console, AudioContext, GainNode, OscillatorNode, OscillatorType};

    use crate::Voice;

    thread_local! {
        // The page's single AudioContext, created on the first sounded note —
        // after the user pressed play, the gesture browsers want before audio
        // (ADR-0024). `None` until then, and left `None` if the browser denies one.
        static CTX: RefCell<Option<AudioContext>> = const { RefCell::new(None) };
    }

    /// Schedules every crossed voice as a plucked note on the shared
    /// AudioContext. A no-op for an empty batch, so a silent frame never touches
    /// audio; the context is booted (and resumed) lazily on the first sound.
    pub(crate) fn sound(voices: &[Voice]) {
        if voices.is_empty() {
            return;
        }
        CTX.with(|cell| {
            let mut slot = cell.borrow_mut();
            if slot.is_none() {
                match AudioContext::new() {
                    Ok(ctx) => *slot = Some(ctx),
                    Err(err) => {
                        console::error_1(&err);
                        return;
                    }
                }
            }
            let Some(ctx) = slot.as_ref() else {
                return;
            };
            // A context can boot suspended; the play gesture lets resume() run it.
            let _resume = ctx.resume();
            let at = ctx.current_time();
            for voice in voices {
                if let Err(err) = pluck(ctx, *voice, at) {
                    console::error_1(&err);
                }
            }
        });
    }

    /// One plucked note: a sawtooth oscillator through a gain envelope (a fast
    /// attack, then an exponential decay over the note's clamped ring), wired to
    /// the speakers and scheduled to start at `at` and stop once it has decayed.
    fn pluck(ctx: &AudioContext, voice: Voice, at: f64) -> Result<(), JsValue> {
        let osc: OscillatorNode = ctx.create_oscillator()?;
        let gain: GainNode = ctx.create_gain()?;
        osc.set_type(OscillatorType::Sawtooth);
        osc.frequency().set_value(voice.freq);

        // Clamp the ring so a held note neither clicks nor drones (0.05–2.5 s).
        let secs = f64::from(voice.secs).clamp(0.05, 2.5);
        let env = gain.gain();
        env.set_value_at_time(0.0001, at)?;
        env.linear_ramp_to_value_at_time(0.18, at + 0.006)?;
        env.exponential_ramp_to_value_at_time(0.0001, at + secs)?;

        osc.connect_with_audio_node(&gain)?;
        gain.connect_with_audio_node(&ctx.destination())?;
        osc.start_with_when(at)?;
        osc.stop_with_when(at + secs + 0.03)?;
        Ok(())
    }
}
