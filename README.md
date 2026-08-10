# tinyviolin

`tinyviolin` is a small dependency-free Rust library for generated mono
instrument sounds. It provides basic oscillators, melodic presets, percussion,
fixed-capacity polyphony, sample-timed control events, and fixed-storage MIDI
control.

## Instruments

| Instrument | Simple synthesis model | Typical frequency |
| --- | --- | ---: |
| `Sine`, `Square`, `Triangle` | One phase-accumulator waveform | 220–880 Hz |
| `Bass` | Sine plus triangle, short decay | 55–220 Hz |
| `Pad` | Three slightly detuned soft oscillators, slow attack/release | 110–440 Hz |
| `Lead` | Square plus triangle, fast envelope | 220–880 Hz |
| `BassDrum` | Sine with a fast downward pitch envelope | 40–80 Hz |
| `Tom` | Sine plus triangle with a smaller pitch drop | 80–240 Hz |
| `Snare` | Deterministic noise plus a pitched tone | 120–240 Hz |
| `HiHat` | Bright deterministic noise plus metallic square waves | 4–8 kHz |

The oscillators are intentionally elementary rather than band-limited. Very
high oscillator frequencies can therefore alias. Frequencies are required to
be positive and finite and are clamped below Nyquist during synthesis.

## Direct event control

```rust
use tinyviolin::{Event, Instrument, Synth, TimedEvent, VoiceId};

let mut synth = Synth::<32>::new(48_000.0)?;
let events = [
    TimedEvent::new(0, Event::NoteOn {
        id: VoiceId(1),
        instrument: Instrument::Bass,
        frequency_hz: 110.0,
        gain: 0.5,
    }),
    TimedEvent::new(192, Event::NoteOff(VoiceId(1))),
];
let mut mono_buffer = [0.0_f32; 256];
synth.process(&mut mono_buffer, &events)?;
# Ok::<(), tinyviolin::ProcessError>(())
```

`process` fills (rather than adds to) the supplied mono buffer. Buffer size is
inferred from the slice. Timed events must be in nondecreasing offset order and
may use `output.len()` to change state at the end of a block. The complete event
slice is validated before output or engine state changes.

A `VoiceId` identifies a logical note. A repeated note-on with the same ID
restarts it, note-off releases it, and `AllNotesOff` releases every voice.
Percussion voices also finish on their own. Configuration and stream errors are
ordinary values and leave state unchanged when validation fails:

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

## MIDI control

The MIDI wrapper stores a direct mapping for each of 16 channels and 128 notes.
Each key has a compile-time fixed number of layers. Mappings are empty initially.

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

let messages = [TimedMidiMessage::new(
    0,
    MidiMessage::new(&[0x90, 60, 100])?,
)];
let mut mono_buffer = [0.0_f32; 128];
synth.process(&mut mono_buffer, &messages)?;
# Ok::<(), tinyviolin::midi::MidiError>(())
```

Messages are copied into a length-tagged `[u8; 4]` backing store. The supported
self-contained MIDI 1.0 messages are note-on, note-off, velocity-zero note-on,
All Sound Off (CC 120), and All Notes Off (CC 123). Running status, `SysEx`, MIDI
2.0 UMP, pitch bend, sustain, and general CC automation are not interpreted.
Malformed and unsupported messages return an error. MIDI note-off identity is
independent of the current mapping, so remapping cannot strand an active note.

## Real-time use and capacities

- Construct `Synth`/`MidiSynth`, configure mappings, and prepare event storage
  outside the audio callback.
- `Synth::dispatch`, `Synth::process`, `MidiSynth::dispatch`, and
  `MidiSynth::process` use fixed arrays and perform no allocation, locking, I/O,
  or logging.
- `Synth<VOICES>` caps simultaneous layers. When full, allocation uses an empty
  voice first, then the oldest released voice, then the oldest active voice.
- `MidiSynth<VOICES, LAYERS>` additionally fixes layers per channel/note. Its
  direct lookup table trades a bounded amount of memory for constant-time lookup.
- Sample rate is fixed for an engine's lifetime. Build a replacement engine away
  from the callback if the host rate changes.
- Output is mono `f32` in `-1.0..=1.0`; routing, panning, stereo duplication, and
  device integration belong to the host application.
- Passing prepared slices is allocation-free, but allocation of a caller's
  `Vec` or queue is the caller's responsibility. A fixed-capacity SPSC queue is
  a typical way to transfer events into an audio callback.

## Render example

```text
cargo run --release --example render_wav
```

This writes `rendered/tinyviolin_presets.wav`, with one labeled-in-source
section per preset. It writes a file only and never opens an audio device.

## Development

```text
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
cargo doc --no-deps
```
