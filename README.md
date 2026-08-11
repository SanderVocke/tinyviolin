# tinyviolin

`tinyviolin` is a small dependency-free Rust library for generated instrument sounds and in-place multichannel audio processing. It is intended as a minimal demonstrator for integrating MIDI, synthesis, audio input mixing, and simple post-processing effects into music production software.

## Installation

Add the library to your project with:

```text
cargo add tinyviolin
```

Or add it directly to your manifest:

```toml
[dependencies]
tinyviolin = "0.3.0"
```

The minimum supported Rust version is 1.85.

## Instruments

| Instrument | Simple synthesis model | Typical frequency |
| --- | --- | ---: |
| `Sine`, `Square`, `Triangle` | One phase-accumulator waveform | 220–880 Hz |
| `Pluck` | Triangle body with a quickly fading harmonic/noise attack | 110–880 Hz |
| `Bass` | Sine plus triangle, short decay | 55–220 Hz |
| `Pad` | Three slightly detuned soft oscillators, slow attack/release | 110–440 Hz |
| `Lead` | Square plus triangle, fast envelope | 220–880 Hz |
| `BassDrum` | Sine with a fast downward pitch envelope | 40–80 Hz |
| `Tom` | Sine plus triangle with a smaller pitch drop | 80–240 Hz |
| `Snare` | Deterministic noise plus a pitched tone | 120–240 Hz |
| `HiHat` | Bright deterministic noise plus metallic square waves | 4–8 kHz |

The oscillators are intentionally elementary rather than band-limited, so very high oscillator frequencies can alias. Frequencies must be positive and finite and are clamped below Nyquist during synthesis.

Each instrument exposes `Instrument::default_gain()`, a linear calibration based on its waveform, spectrum, envelope, and duration at the typical pitches above. Built-in MIDI presets apply this calibration automatically, so equal MIDI velocities have similar perceived loudness. Direct `Event`s and custom `MidiLayer`s remain explicit: multiply velocity by `default_gain()` when the same behavior is desired. The calibration preserves the contrasting attacks and durations of pads and percussion rather than flattening their envelopes.

## Direct event control

```rust
use tinyviolin::{Event, Instrument, Synth, TimedEvent, VoiceId};

let mut synth = Synth::<32>::new(48_000.0)?;
let events = [
    TimedEvent::new(0, Event::NoteOn {
        id: VoiceId(1),
        instrument: Instrument::Bass,
        frequency_hz: 110.0,
        gain: Instrument::Bass.default_gain() * 0.5,
    }),
    TimedEvent::new(192, Event::NoteOff(VoiceId(1))),
];
let mut mono_buffer = [0.0_f32; 256];
synth.process(&mut mono_buffer, &events)?;
# Ok::<(), tinyviolin::ProcessError>(())
```

`Synth::process` fills rather than adds to the supplied mono buffer. Buffer size is inferred from the slice. Timed events must be in nondecreasing offset order and may use `output.len()` to change state at the end of a block. The complete event slice is validated before output or engine state changes.

A `VoiceId` identifies a logical note. A repeated note-on with the same ID restarts it, note-off releases it, and `AllNotesOff` releases every voice. For pluck and percussion voices, an early release request is deferred until the instrument's initial transient has played in full; a later request still releases the voice immediately. This makes short e-drum and trigger-pad taps sound complete without turning a held pluck into a fixed-length one-shot. Percussion voices also finish when their natural one-shot envelope ends. `reset_dsp`, `panic`, and MIDI All Sound Off remain immediate. Configuration and stream errors are ordinary values and leave state unchanged when validation fails:

```rust
use tinyviolin::{Event, Instrument, ProcessError, Synth, VoiceId};

let mut synth = Synth::<8>::new(48_000.0)?;
let result = synth.dispatch(Event::NoteOn {
    id: VoiceId(1),
    instrument: Instrument::Sine,
    frequency_hz: f32::NAN,
    gain: 0.5,
});
assert_eq!(result, Err(ProcessError::InvalidFrequency));
assert_eq!(synth.active_voice_count(), 0);
# Ok::<(), ProcessError>(())
```

## Multichannel input and effects

`AudioProcessor` configures any nonzero number of channels during setup. Processing is in-place: every channel slice initially contains that channel's audio input. The processor adds the same synthesized sample to every channel, then independently applies distortion, three-band EQ, compression, and reverb to each input+synth mix.

```rust
use tinyviolin::{AudioProcessor, EffectSettings};

let mut processor = AudioProcessor::<32>::new(48_000.0, 2)?;
processor.set_effect_settings(EffectSettings {
    reverb_enabled: true,
    reverb_amount: 0.3,
    distortion_enabled: true,
    distortion_drive: 4.0,
    compressor_enabled: true,
    compressor_amount: 0.5,
    eq_enabled: true,
    eq_low_db: 2.0,
    eq_mid_db: -1.0,
    eq_high_db: 1.5,
})?;

let mut left = [0.0_f32; 128];
let mut right = [0.0_f32; 128];
processor.process(&mut [&mut left, &mut right], &[])?;
# Ok::<(), tinyviolin::ProcessError>(())
```

Reverb has one dry/wet amount control in `0.0..=1.0`, and distortion has one linear drive control in `1.0..=20.0`. The one-knob compressor's amount in `0.0..=1.0` jointly lowers its threshold and raises its ratio. The three-band EQ provides low, mid, and high gains in `-12..=12` dB, with crossovers at approximately 250 Hz and 4 kHz. Each effect has an independent bypass toggle and all effects are bypassed by default. Use `set_effect_settings` to replace all controls together; dedicated setters are also available for every toggle and control.

`AudioProcessor::process` accepts sample-timed synthesis events for a complete block. `AudioProcessor::render_range` and `AudioProcessor::dispatch` support hosts that deliver events incrementally while traversing a block. Channel count, channel lengths, frame ranges, event timing, and effect values are validated before processing.

`AudioProcessor<VOICES, MIDI_LAYERS>` also has fixed-storage MIDI mappings and accepts MIDI directly. Configure mappings with `set_midi_layer` or `set_midi_channel_layer`, use `dispatch_midi` for an immediate `MidiMessage`, and use `process_midi` for sample-timed `TimedMidiMessage`s. MIDI-triggered voices follow the same input+synth mixing and effects path as direct `Event`s. `AudioMidiError` distinguishes audio-block errors from MIDI message or timing errors.

## MIDI control

The standalone `MidiSynth` wrapper and the MIDI-capable `AudioProcessor` store a direct mapping for each of 16 channels and 128 notes. Each key has a compile-time fixed number of layers, and mappings are empty initially. `MidiSynth` produces mono synth-only output, while `AudioProcessor::process_midi` adds audio input and effects.

Hosts can enumerate `Preset::available()` or call `available_presets()` on either MIDI-capable processor, display each preset's runtime `id()` and `name()`, and select it with `select_preset` or `select_preset_by_id`. Hosts therefore do not need to duplicate the library's preset list. Selecting a preset replaces mappings on every MIDI channel and clears extra layers.

```rust
use tinyviolin::Instrument;
use tinyviolin::midi::{
    MidiLayer, MidiMessage, MidiPitch, MidiSynth, TimedMidiMessage,
};

let mut synth = MidiSynth::<32, 2>::new(48_000.0)?;
synth.set_channel_layer(0, 0, MidiLayer {
    instrument: Instrument::Pad,
    pitch: MidiPitch::Note,
    gain: 0.4,
})?;
synth.set_layer(9, 36, 0, MidiLayer {
    instrument: Instrument::BassDrum,
    pitch: MidiPitch::Fixed(60.0),
    gain: 0.8,
})?;

// Or select any preset discovered at runtime.
synth.select_preset_by_id("percussion-kit")?;

let messages = [TimedMidiMessage::new(
    0,
    MidiMessage::new(&[0x90, 60, 100])?,
)];
let mut mono_buffer = [0.0_f32; 128];
synth.process(&mut mono_buffer, &messages)?;
# Ok::<(), tinyviolin::midi::MidiError>(())
```

Messages are copied into a length-tagged `[u8; 4]` backing store. The supported self-contained MIDI 1.0 messages are note-on, note-off, velocity-zero note-on, pitch bend, modulation wheel (CC1), All Sound Off (CC 120), and All Notes Off (CC 123). Pitch bend is channel-wide with a fixed range of ±2 semitones. The modulation wheel adds channel-wide 5.5 Hz vibrato with up to ±0.5 semitone depth. Both controls affect active voices without restarting their oscillator or envelope and are inherited by new notes; `reset_dsp` returns them to center/off. Running status, `SysEx`, MIDI 2.0 UMP, sustain, and other general MIDI CC automation are not interpreted. Malformed and unsupported messages return an error. MIDI note-off identity is independent of the current mapping, so remapping cannot strand an active note.

The `percussion-kit` preset puts bass drums on General MIDI keys 35/36, snares on 38/40, toms on 41/43/45/47/48/50, and hi-hats on 42/44/46. Every other key carries the preceding supported assignment forward (with bass drum below key 35), so all 128 keys produce sound in one `MidiSynth`.

## Panic and session state

`Synth`, `MidiSynth`, and `AudioProcessor` provide `reset_dsp()` and its host-oriented `panic()` alias. A reset immediately clears voices and oscillator state. MIDI-capable processors also center pitch bend and turn modulation off, while `AudioProcessor` additionally clears every effect tail. Sample rate, channel layout, MIDI mappings, selected preset, and effect settings are preserved.

`MidiSynth::serialize_state` saves its mappings and selected preset. `AudioProcessor::serialize_state` additionally saves effect settings. The corresponding `load_state` methods validate the complete versioned binary payload before replacing configuration; malformed or capacity-incompatible state returns `StateError` and leaves configuration unchanged. DSP state, sample rate, and channel count are deliberately not serialized. Loading state does not panic the current DSP state, so a host can call `reset_dsp()` separately when desired.

```rust
use tinyviolin::{AudioProcessor, Preset};

let mut source = AudioProcessor::<4, 1>::new(48_000.0, 2)?;
source.select_preset(Preset::Pad);
let session_bytes = source.serialize_state();
drop(source);

let mut restored = AudioProcessor::<4, 1>::new(48_000.0, 2)?;
restored.load_state(&session_bytes)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Real-time use and capacities

- Construct `Synth`, `MidiSynth`, or `AudioProcessor`, configure mappings, select presets, serialize/load session state, and prepare event storage outside the audio callback. `AudioProcessor` construction allocates fixed-per-stream reverb delay storage for each channel.
- Dispatch and processing methods perform no allocation, locking, I/O, or logging.
- `Synth<VOICES>` caps simultaneous layers. When full, allocation uses an empty voice first, then the oldest released voice, then the oldest active voice.
- `MidiSynth<VOICES, LAYERS>` additionally fixes layers per channel/note. Its direct lookup table trades a bounded amount of memory for constant-time lookup.
- Sample rate and `AudioProcessor` channel count are fixed for an engine's lifetime. Build a replacement engine away from the callback when either changes.
- `Synth` and `MidiSynth` produce mono `f32`. `AudioProcessor` processes any configured nonzero channel count in place. Successful output is bounded to `-1.0..=1.0`.
- Passing prepared slices is allocation-free, but allocation of a caller's `Vec` or queue is the caller's responsibility. A fixed-capacity SPSC queue is a typical way to transfer events into an audio callback.

## Render example

```text
cargo run --release --example render_wav
```

This writes `rendered/tinyviolin_presets.wav`, with one labeled-in-source section per synthesized instrument. It writes a file only and never opens an audio device.

## Plugin and standalone showcase

The `tinyviolin-showcase` workspace package wraps the library as a CLAP/VST3 instrument/effect and as a native nice-plug application. It exposes matched audio input/output layouts from mono through 63 channels, the maximum channel count representable by the VST3 layout wrapper. Synthesized sound is sent equally to every output channel, mixed with that channel's input, processed by distortion, three-band EQ, compression, and reverb, and then scaled by master gain.

The egui editor provides all twelve presets, smoothed master gain, reverb, distortion, one-knob compressor, three-band EQ, and a clickable two-octave piano. The same controls are exposed to plugin hosts with parameter IDs `preset`, `master-gain`, `reverb-enabled`, `reverb-amount`, `distortion-enabled`, `distortion-drive`, `compressor-enabled`, `compressor-amount`, `eq-enabled`, `eq-low`, `eq-mid`, and `eq-high`. The core `tinyviolin` package remains the workspace's dependency-free default member.

Install the nice-plug bundler and build all release artifacts with:

```text
cargo install cargo-nice-plug
cargo nice-plug bundle tinyviolin-showcase --release --features standalone
```

The bundles are written below `target/bundled/`. Run the native application with JACK using:

```text
cargo run -p tinyviolin-showcase --release --features standalone -- --backend jack
```

A JACK server must already be running. The standalone wrapper also supports ALSA on Linux, `CoreAudio` on macOS, and WASAPI on Windows. Pass an empty device argument to list devices before choosing one, for example:

```text
cargo run -p tinyviolin-showcase --features standalone -- --backend alsa --output-device ""
cargo run -p tinyviolin-showcase --features standalone -- --backend alsa --midi-input ""
```

On Debian or Ubuntu, compiling the JACK/OpenGL editor and standalone target requires:

```text
sudo apt-get install pkg-config libasound2-dev libgl-dev libjack-jackd2-dev \
  libx11-xcb-dev libxcb1-dev libxcb-dri2-0-dev libxcb-icccm4-dev \
  libxcursor-dev libxkbcommon-dev libxcb-shape0-dev libxcb-xfixes0-dev
```

The showcase supports note-on, note-off, choke, channel pitch bend with a fixed ±2-semitone range, and modulation wheel (CC1) as 5.5 Hz vibrato up to ±0.5 semitone. It does not support MPE, aftertouch, sustain, or other general MIDI CC automation. Host notes are identified by channel and note, so overlapping instances of the same key retrigger one logical voice. Bass drum, tom, snare, and hi-hat use fixed pitches of 60, 130, 180, and 6000 Hz respectively; melodic presets use equal-tempered MIDI pitch. Preset changes apply to new notes, while effects and master gain affect the complete input+synth mix. On Windows, Cargo currently emits a harmless PDB filename-collision warning when building the same package's library and standalone binary together; the resulting executable and plugin bundles are distinct and usable.

## Development

```text
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
cargo test --workspace --doc
cargo doc --workspace --no-deps
cargo check -p tinyviolin-showcase --all-features --all-targets
cargo publish -p tinyviolin --dry-run
```

## License

Licensed under the [MIT License](https://github.com/SanderVocke/tinyviolin/blob/master/LICENSE).
