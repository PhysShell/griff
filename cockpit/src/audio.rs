//! The cockpit's playback backends (S8 Slice 2).
//!
//! [`Synth`] is the platform's [`PlaybackSink`]: the shared
//! [`griff_ui_core::playback::Player`] drives it with real note-ons and
//! note-offs, so a note sounds for its written duration on every target.
//!
//! - **Native** speaks MIDI over `midir` to a chosen output port (a hardware
//!   synth, a DAW, the OS synth). The user picks the port; `all_notes_off`
//!   sends the panic controllers so nothing rings after a stop.
//! - **Web** drives the browser's Web Audio: a sustained sine per sounding
//!   pitch, started on note-on and released on note-off — not the fixed
//!   blip of the first sketch.
//! - Either backend can be **unavailable** (no MIDI port, a browser that
//!   refuses an audio context). It then plays silently and reports why, and
//!   the cockpit's "open externally" path stays the way to actually hear the
//!   `.mid`.
//!
//! Both `Synth`s expose the same device surface ([`Synth::ports`],
//! [`Synth::connect`], [`Synth::status`]) so `lib.rs` stays platform-neutral.
//! The pitch→frequency map ([`midi_to_hz`]) is pure and unit-tested.

/// Equal-temperament frequency (Hz) of a MIDI pitch, A4 = 440 Hz (pitch 69):
/// `hz = 440 · 2^((pitch − 69) / 12)`.
#[must_use]
pub fn midi_to_hz(pitch: u8) -> f64 {
    let semitones_from_a4 = f64::from(i16::from(pitch) - 69);
    440.0 * (semitones_from_a4 / 12.0).exp2()
}

#[cfg(target_arch = "wasm32")]
pub use web::Synth;

#[cfg(not(target_arch = "wasm32"))]
pub use native::Synth;

// ── native: MIDI out via midir ───────────────────────────────────────────────
#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::fmt;

    use griff_ui_core::playback::PlaybackSink;
    use midir::{MidiOutput, MidiOutputConnection};

    /// The MIDI channel playback sends on (0-based channel 1).
    const CHANNEL: u8 = 0;

    /// The native voice: an optional open MIDI output connection plus the
    /// list of ports the user can choose from.
    pub struct Synth {
        conn: Option<MidiOutputConnection>,
        ports: Vec<String>,
        selected: Option<usize>,
        status: String,
    }

    impl Default for Synth {
        fn default() -> Self {
            Self::new()
        }
    }

    impl Synth {
        /// A disconnected synth with the current port list scanned. No port is
        /// opened until the user connects one.
        #[must_use]
        pub fn new() -> Self {
            let mut synth = Self {
                conn: None,
                ports: Vec::new(),
                selected: None,
                status: String::new(),
            };
            synth.refresh_ports();
            synth
        }

        /// Rescans the available MIDI output ports (devices come and go).
        pub fn refresh_ports(&mut self) {
            self.ports = MidiOutput::new("griff-cockpit").map_or_else(
                |_| Vec::new(),
                |out| {
                    out.ports()
                        .iter()
                        .filter_map(|p| out.port_name(p).ok())
                        .collect()
                },
            );
            if self.ports.is_empty() {
                "no MIDI output — use \"open externally\"".clone_into(&mut self.status);
            } else if self.conn.is_none() {
                self.status = format!("{} MIDI port(s) — pick one", self.ports.len());
            }
        }

        /// The device names, for a picker.
        #[must_use]
        pub fn ports(&self) -> &[String] {
            &self.ports
        }

        /// The connected port index, if any.
        #[must_use]
        pub const fn selected(&self) -> Option<usize> {
            self.selected
        }

        /// A one-line backend status for the transport bar.
        #[must_use]
        pub fn status(&self) -> &str {
            &self.status
        }

        /// Opens output port `index`, replacing any current connection. A
        /// failure leaves the synth silent with a message, never a panic.
        pub fn connect(&mut self, index: usize) {
            self.conn = None;
            self.selected = None;
            let Ok(out) = MidiOutput::new("griff-cockpit") else {
                "MIDI is unavailable on this system".clone_into(&mut self.status);
                return;
            };
            let ports = out.ports();
            let Some(port) = ports.get(index) else {
                "that MIDI port is gone — rescan".clone_into(&mut self.status);
                return;
            };
            let name = out.port_name(port).unwrap_or_else(|_| "MIDI".to_owned());
            match out.connect(port, "griff-playback") {
                Ok(conn) => {
                    self.conn = Some(conn);
                    self.selected = Some(index);
                    self.status = format!("playing to {name}");
                }
                Err(_) => self.status = format!("could not open {name}"),
            }
        }

        /// Closes the connection (silencing first is the caller's job).
        pub fn disconnect(&mut self) {
            self.conn = None;
            self.selected = None;
            self.refresh_ports();
        }

        /// Sends a three-byte channel message, best-effort. A send that fails
        /// means the port vanished: drop the connection and say so, rather
        /// than panic or leave a phantom device selected.
        fn send(&mut self, status: u8, a: u8, b: u8) {
            let bytes = [status | CHANNEL, a & 0x7f, b & 0x7f];
            let lost = self
                .conn
                .as_mut()
                .is_some_and(|conn| conn.send(&bytes).is_err());
            if lost {
                self.conn = None;
                self.selected = None;
                "the MIDI port dropped — rescan".clone_into(&mut self.status);
            }
        }
    }

    impl fmt::Debug for Synth {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.debug_struct("Synth")
                .field("connected", &self.conn.is_some())
                .field("ports", &self.ports.len())
                .field("selected", &self.selected)
                .finish_non_exhaustive()
        }
    }

    impl PlaybackSink for Synth {
        fn note_on(&mut self, pitch: u8, velocity: u8) {
            self.send(0x90, pitch, velocity.max(1));
        }

        fn note_off(&mut self, pitch: u8) {
            self.send(0x80, pitch, 0);
        }

        fn all_notes_off(&mut self) {
            // CC 123 (all notes off) and CC 120 (all sound off) — belt and
            // braces so a stop leaves nothing ringing.
            self.send(0xB0, 123, 0);
            self.send(0xB0, 120, 0);
        }
    }
}

// ── web: sustained Web Audio voices ──────────────────────────────────────────
#[cfg(target_arch = "wasm32")]
mod web {
    use std::fmt;

    use griff_ui_core::playback::PlaybackSink;
    use web_sys::{AudioContext, GainNode, OscillatorNode, OscillatorType};

    use super::midi_to_hz;

    /// Peak gain of one voice, low enough that a chord sums short of clipping.
    const VOICE_GAIN: f32 = 0.14;
    /// Click-free attack, seconds.
    const ATTACK_SECS: f64 = 0.005;
    /// Release ramp, seconds — a short fade so a note-off is not a click.
    const RELEASE_SECS: f64 = 0.03;

    /// One sounding pitch: its oscillator and gain, kept until note-off.
    struct Voice {
        pitch: u8,
        osc: OscillatorNode,
        gain: GainNode,
    }

    /// The browser voice: a lazily-opened context and the set of sounding
    /// voices, each started on note-on and released on note-off.
    #[derive(Default)]
    pub struct Synth {
        ctx: Option<AudioContext>,
        voices: Vec<Voice>,
        status: String,
    }

    impl fmt::Debug for Synth {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.debug_struct("Synth")
                .field("open", &self.ctx.is_some())
                .field("voices", &self.voices.len())
                .finish_non_exhaustive()
        }
    }

    impl Synth {
        /// A silent synth; the audio context opens on the first note.
        #[must_use]
        pub fn new() -> Self {
            Self {
                ctx: None,
                voices: Vec::new(),
                status: "Web Audio (browser output)".to_owned(),
            }
        }

        /// The browser has one implicit output; there is no port list.
        pub fn refresh_ports(&mut self) {}

        /// No selectable ports on the web target.
        #[must_use]
        pub fn ports(&self) -> &[String] {
            &[]
        }

        /// The web target has no selectable port.
        #[must_use]
        pub const fn selected(&self) -> Option<usize> {
            None
        }

        /// The backend status line.
        #[must_use]
        pub fn status(&self) -> &str {
            &self.status
        }

        /// No-op on the web target: there is nothing to connect to.
        pub fn connect(&mut self, _index: usize) {}

        /// The audio context, opened on first use and resumed past the
        /// autoplay policy. `None` forever in a browser that refuses one.
        fn context(&mut self) -> Option<AudioContext> {
            if self.ctx.is_none() {
                self.ctx = AudioContext::new().ok();
                if self.ctx.is_none() {
                    self.status = "no Web Audio — use \"open externally\"".to_owned();
                }
            }
            if let Some(ctx) = &self.ctx {
                let _ = ctx.resume();
            }
            self.ctx.clone()
        }
    }

    impl PlaybackSink for Synth {
        fn note_on(&mut self, pitch: u8, velocity: u8) {
            let Some(ctx) = self.context() else {
                return;
            };
            if let Ok(voice) = start_voice(&ctx, pitch, velocity) {
                self.voices.push(voice);
            }
        }

        fn note_off(&mut self, pitch: u8) {
            if let Some(pos) = self.voices.iter().position(|v| v.pitch == pitch) {
                let voice = self.voices.swap_remove(pos);
                if let Some(ctx) = &self.ctx {
                    release_voice(ctx, &voice);
                }
            }
        }

        fn all_notes_off(&mut self) {
            if let Some(ctx) = self.ctx.clone() {
                for voice in self.voices.drain(..) {
                    release_voice(&ctx, &voice);
                }
            } else {
                self.voices.clear();
            }
        }
    }

    /// Starts a sustained sine at `pitch`, ramping to a velocity-scaled gain.
    fn start_voice(
        ctx: &AudioContext,
        pitch: u8,
        velocity: u8,
    ) -> Result<Voice, wasm_bindgen::JsValue> {
        let now = ctx.current_time();
        let osc: OscillatorNode = ctx.create_oscillator()?;
        let gain: GainNode = ctx.create_gain()?;
        osc.set_type(OscillatorType::Sine);
        osc.frequency().set_value(midi_to_hz(pitch) as f32);

        let peak = VOICE_GAIN * (f32::from(velocity.max(1)) / 127.0);
        let env = gain.gain();
        env.set_value_at_time(0.0, now)?;
        env.linear_ramp_to_value_at_time(peak, now + ATTACK_SECS)?;

        osc.connect_with_audio_node(&gain)?;
        gain.connect_with_audio_node(&ctx.destination())?;
        osc.start_with_when(now)?;
        Ok(Voice { pitch, osc, gain })
    }

    /// Releases a voice: a short gain fade, then the oscillator stops.
    fn release_voice(ctx: &AudioContext, voice: &Voice) {
        let now = ctx.current_time();
        let env = voice.gain.gain();
        // Cancel any pending ramp, then fade from the current value to ~0.
        let _ = env.cancel_scheduled_values(now);
        let _ = env.set_value_at_time(env.value(), now);
        let _ = env.exponential_ramp_to_value_at_time(0.0001, now + RELEASE_SECS);
        let _ = voice.osc.stop_with_when(now + RELEASE_SECS);
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
        assert!((midi_to_hz(60) - 261.625_565).abs() < 1e-3);
    }
}
