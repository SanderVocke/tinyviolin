//! Fixed-capacity MIDI control for [`crate::Synth`].
//!
//! The wrapper accepts self-contained MIDI 1.0 messages. Running status, `SysEx`,
//! and MIDI 2.0 UMP packets are intentionally outside its scope.

use crate::state::{Reader, push_f32, push_u16, push_u32};
use crate::{Event, Instrument, Preset, ProcessError, StateError, Synth, VoiceId};

const MIDI_STATE_MAGIC: &[u8; 4] = b"TVMS";
const MIDI_STATE_VERSION: u16 = 1;

/// Maximum accepted storage size for one self-contained MIDI 1.0 message.
pub const MAX_MESSAGE_BYTES: usize = 4;

/// A self-contained MIDI message backed by at most four bytes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MidiMessage {
    bytes: [u8; MAX_MESSAGE_BYTES],
    len: u8,
}

impl MidiMessage {
    /// Copy a message of no more than four bytes into fixed storage.
    ///
    /// # Errors
    ///
    /// Returns [`MidiError::InvalidLength`] for an empty slice or one longer
    /// than [`MAX_MESSAGE_BYTES`]. Message syntax is checked on dispatch.
    pub fn new(bytes: &[u8]) -> Result<Self, MidiError> {
        if bytes.is_empty() || bytes.len() > MAX_MESSAGE_BYTES {
            return Err(MidiError::InvalidLength);
        }
        let mut storage = [0; MAX_MESSAGE_BYTES];
        storage[..bytes.len()].copy_from_slice(bytes);
        let len = match bytes.len() {
            1 => 1,
            2 => 2,
            3 => 3,
            4 => 4,
            _ => return Err(MidiError::InvalidLength),
        };
        Ok(Self {
            bytes: storage,
            len,
        })
    }

    /// Return the message bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..usize::from(self.len)]
    }
}

/// Pitch assigned to one MIDI mapping layer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MidiPitch {
    /// Standard equal temperament, where note 69 is 440 Hz.
    Note,
    /// Ignore note number and use this positive finite frequency in hertz.
    Fixed(f32),
}

/// One instrument layer triggered by a MIDI channel/note mapping.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MidiLayer {
    /// Instrument to trigger.
    pub instrument: Instrument,
    /// Pitch derivation for the triggered voice.
    pub pitch: MidiPitch,
    /// Linear gain multiplied by MIDI velocity, in `0.0..=1.0`.
    pub gain: f32,
}

/// A MIDI message positioned within an output block.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TimedMidiMessage {
    /// Sample at which to apply the message; block length means block end.
    pub sample_offset: usize,
    /// Fixed-size message.
    pub message: MidiMessage,
}

impl TimedMidiMessage {
    /// Construct a timed message.
    #[must_use]
    pub const fn new(sample_offset: usize, message: MidiMessage) -> Self {
        Self {
            sample_offset,
            message,
        }
    }
}

/// MIDI message, mapping, or timed-stream error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MidiError {
    /// Message storage length was zero or greater than four.
    InvalidLength,
    /// A status, data byte, or status-specific length was malformed.
    MalformedMessage,
    /// The status is well-formed but outside the supported subset.
    UnsupportedMessage,
    /// Channel was outside `0..16`.
    InvalidChannel,
    /// Note was outside `0..128`.
    InvalidNote,
    /// Layer index exceeded the wrapper's fixed layer count, or it is zero.
    InvalidLayer,
    /// Layer gain or fixed frequency was invalid.
    InvalidLayerSettings,
    /// Timed messages were unordered or outside the output block.
    InvalidTiming,
    /// The underlying synthesizer rejected configuration.
    Synth,
    /// No currently available preset has the requested stable ID.
    UnknownPreset,
}

impl core::fmt::Display for MidiError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::InvalidLength => "MIDI message length must be in 1..=4",
            Self::MalformedMessage => "malformed MIDI message",
            Self::UnsupportedMessage => "unsupported MIDI message",
            Self::InvalidChannel => "MIDI channel must be in 0..16",
            Self::InvalidNote => "MIDI note must be in 0..128",
            Self::InvalidLayer => "MIDI layer capacity or index is invalid",
            Self::InvalidLayerSettings => "MIDI layer frequency or gain is invalid",
            Self::InvalidTiming => "timed MIDI messages are unordered or outside the block",
            Self::Synth => "synthesizer rejected MIDI configuration",
            Self::UnknownPreset => "unknown preset ID",
        })
    }
}

impl std::error::Error for MidiError {}

impl From<ProcessError> for MidiError {
    fn from(_: ProcessError) -> Self {
        Self::Synth
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct MidiMapping<const LAYERS: usize> {
    layers: [Option<MidiLayer>; LAYERS],
}

impl<const LAYERS: usize> MidiMapping<LAYERS> {
    const EMPTY: Self = Self {
        layers: [None; LAYERS],
    };
}

/// A synthesizer controlled through fixed channel/note MIDI mappings.
///
/// `VOICES` fixes polyphony and `LAYERS` fixes the number of instruments one
/// channel/note pair can trigger. Mapping mutation is a setup operation; message
/// dispatch and processing neither allocate nor lock.
pub struct MidiSynth<const VOICES: usize = 32, const LAYERS: usize = 2> {
    synth: Synth<VOICES>,
    mappings: [[MidiMapping<LAYERS>; 128]; 16],
    selected_preset: Option<Preset>,
}

impl<const VOICES: usize, const LAYERS: usize> MidiSynth<VOICES, LAYERS> {
    /// Create an engine with empty mappings.
    ///
    /// # Errors
    ///
    /// Returns [`MidiError::InvalidLayer`] if `LAYERS` is zero, or
    /// [`MidiError::Synth`] if the sample rate or voice count is invalid.
    pub fn new(sample_rate: f32) -> Result<Self, MidiError> {
        if LAYERS == 0 {
            return Err(MidiError::InvalidLayer);
        }
        Self::from_synth(Synth::new(sample_rate)?)
    }

    pub(crate) fn from_synth(synth: Synth<VOICES>) -> Result<Self, MidiError> {
        if LAYERS == 0 {
            return Err(MidiError::InvalidLayer);
        }
        Ok(Self {
            synth,
            mappings: [[MidiMapping::EMPTY; 128]; 16],
            selected_preset: None,
        })
    }

    /// Return the underlying engine for read-only inspection.
    #[must_use]
    pub const fn engine(&self) -> &Synth<VOICES> {
        &self.synth
    }

    pub(crate) const fn engine_mut(&mut self) -> &mut Synth<VOICES> {
        &mut self.synth
    }

    /// Return every preset available in this library version.
    #[must_use]
    pub const fn available_presets(&self) -> &'static [Preset] {
        Preset::available()
    }

    /// Return the selected built-in preset, or `None` after manual mapping.
    #[must_use]
    pub const fn selected_preset(&self) -> Option<Preset> {
        self.selected_preset
    }

    /// Replace all channel/note mappings with a built-in preset.
    ///
    /// The preset is applied to all 16 MIDI channels. Additional mapping layers
    /// are cleared. This setup operation performs bounded fixed-storage writes.
    pub fn select_preset(&mut self, preset: Preset) {
        for channel in &mut self.mappings {
            for (note, mapping) in (0_u8..=127).zip(channel) {
                *mapping = MidiMapping::EMPTY;
                let instrument = preset.instrument(note);
                mapping.layers[0] = Some(MidiLayer {
                    instrument,
                    pitch: if preset.uses_midi_pitch() {
                        MidiPitch::Note
                    } else {
                        MidiPitch::Fixed(percussion_frequency(instrument))
                    },
                    gain: 1.0,
                });
            }
        }
        self.selected_preset = Some(preset);
    }

    /// Select a built-in preset by its stable runtime ID.
    ///
    /// # Errors
    ///
    /// Returns [`MidiError::UnknownPreset`] if the ID is not advertised by
    /// [`Self::available_presets`].
    pub fn select_preset_by_id(&mut self, id: &str) -> Result<(), MidiError> {
        let preset = Preset::from_id(id).ok_or(MidiError::UnknownPreset)?;
        self.select_preset(preset);
        Ok(())
    }

    /// Immediately clear voice and oscillator state while preserving mappings.
    pub fn reset_dsp(&mut self) {
        self.synth.reset_dsp();
    }

    /// Alias for [`Self::reset_dsp`] named after the conventional host action.
    pub fn panic(&mut self) {
        self.reset_dsp();
    }

    /// Serialize MIDI mappings and preset selection for host session storage.
    ///
    /// Voice, oscillator, and other DSP state is intentionally omitted. This
    /// setup operation allocates the returned byte vector.
    #[must_use]
    pub fn serialize_state(&self) -> Vec<u8> {
        let mut output = Vec::new();
        output.extend_from_slice(MIDI_STATE_MAGIC);
        push_u16(&mut output, MIDI_STATE_VERSION);
        push_u32(&mut output, u32::try_from(LAYERS).unwrap_or(u32::MAX));
        if let Some(preset) = self.selected_preset {
            let id = preset.id().as_bytes();
            output.push(u8::try_from(id.len()).unwrap_or(u8::MAX));
            output.extend_from_slice(id);
        } else {
            output.push(0);
        }
        for channel in &self.mappings {
            for mapping in channel {
                for layer in mapping.layers {
                    match layer {
                        None => output.push(0),
                        Some(layer) => {
                            output.push(1);
                            output.push(instrument_code(layer.instrument));
                            match layer.pitch {
                                MidiPitch::Note => output.push(0),
                                MidiPitch::Fixed(frequency) => {
                                    output.push(1);
                                    push_f32(&mut output, frequency);
                                }
                            }
                            push_f32(&mut output, layer.gain);
                        }
                    }
                }
            }
        }
        output
    }

    /// Load MIDI configuration previously returned by [`Self::serialize_state`].
    ///
    /// Loading is transactional: malformed or incompatible data leaves mappings
    /// unchanged. Existing voices and oscillator state are intentionally left
    /// untouched; call [`Self::reset_dsp`] separately when a panic is desired.
    ///
    /// # Errors
    ///
    /// Returns a [`StateError`] for malformed, unsupported, incompatible, or
    /// invalid configuration data.
    pub fn load_state(&mut self, state: &[u8]) -> Result<(), StateError> {
        let mut reader = Reader::new(state);
        if reader.read_exact(MIDI_STATE_MAGIC.len())? != MIDI_STATE_MAGIC {
            return Err(StateError::InvalidData);
        }
        if reader.u16()? != MIDI_STATE_VERSION {
            return Err(StateError::UnsupportedVersion);
        }
        let layers = usize::try_from(reader.u32()?).map_err(|_| StateError::InvalidData)?;
        if layers != LAYERS {
            return Err(StateError::IncompatibleMidiLayers);
        }
        let preset_id_len = usize::from(reader.u8()?);
        let selected_preset = if preset_id_len == 0 {
            None
        } else {
            let id = core::str::from_utf8(reader.read_exact(preset_id_len)?)
                .map_err(|_| StateError::InvalidData)?;
            Preset::from_id(id)
        };

        let mut mappings = [[MidiMapping::EMPTY; 128]; 16];
        for channel in &mut mappings {
            for mapping in channel {
                for layer in &mut mapping.layers {
                    *layer = match reader.u8()? {
                        0 => None,
                        1 => {
                            let instrument = instrument_from_code(reader.u8()?)
                                .ok_or(StateError::InvalidConfiguration)?;
                            let pitch = match reader.u8()? {
                                0 => MidiPitch::Note,
                                1 => MidiPitch::Fixed(reader.f32()?),
                                _ => return Err(StateError::InvalidConfiguration),
                            };
                            let value = MidiLayer {
                                instrument,
                                pitch,
                                gain: reader.f32()?,
                            };
                            validate_layer(value).map_err(|_| StateError::InvalidConfiguration)?;
                            Some(value)
                        }
                        _ => return Err(StateError::InvalidConfiguration),
                    };
                }
            }
        }
        if !reader.is_finished() {
            return Err(StateError::InvalidData);
        }

        self.mappings = mappings;
        self.selected_preset = selected_preset;
        Ok(())
    }

    /// Set one channel/note mapping layer.
    ///
    /// This setup method is not intended for an audio callback.
    ///
    /// # Errors
    ///
    /// Returns a [`MidiError`] for an out-of-range channel, note, or layer, or
    /// for a non-finite/out-of-range gain or invalid fixed frequency.
    pub fn set_layer(
        &mut self,
        channel: u8,
        note: u8,
        layer_index: usize,
        layer: MidiLayer,
    ) -> Result<(), MidiError> {
        let slot = mapping_slot(channel, note)?;
        if layer_index >= LAYERS {
            return Err(MidiError::InvalidLayer);
        }
        validate_layer(layer)?;
        self.mappings[slot.0][slot.1].layers[layer_index] = Some(layer);
        self.selected_preset = None;
        Ok(())
    }

    /// Apply the same layer to all 128 notes on one channel.
    ///
    /// This setup convenience performs bounded fixed-storage writes.
    ///
    /// # Errors
    ///
    /// Returns a [`MidiError`] for an invalid channel, layer index, or settings.
    pub fn set_channel_layer(
        &mut self,
        channel: u8,
        layer_index: usize,
        layer: MidiLayer,
    ) -> Result<(), MidiError> {
        if channel >= 16 {
            return Err(MidiError::InvalidChannel);
        }
        if layer_index >= LAYERS {
            return Err(MidiError::InvalidLayer);
        }
        validate_layer(layer)?;
        for mapping in &mut self.mappings[usize::from(channel)] {
            mapping.layers[layer_index] = Some(layer);
        }
        self.selected_preset = None;
        Ok(())
    }

    /// Remove one mapping layer.
    ///
    /// # Errors
    ///
    /// Returns a [`MidiError`] for an out-of-range channel, note, or layer.
    pub fn clear_layer(
        &mut self,
        channel: u8,
        note: u8,
        layer_index: usize,
    ) -> Result<(), MidiError> {
        let slot = mapping_slot(channel, note)?;
        if layer_index >= LAYERS {
            return Err(MidiError::InvalidLayer);
        }
        self.mappings[slot.0][slot.1].layers[layer_index] = None;
        self.selected_preset = None;
        Ok(())
    }

    /// Parse and apply one MIDI message immediately.
    ///
    /// This callback-safe method performs no allocation or locking.
    ///
    /// # Errors
    ///
    /// Returns [`MidiError::MalformedMessage`] or
    /// [`MidiError::UnsupportedMessage`] when parsing fails.
    pub fn dispatch(&mut self, message: MidiMessage) -> Result<(), MidiError> {
        let parsed = parse(message)?;
        self.dispatch_parsed(parsed);
        Ok(())
    }

    pub(crate) fn dispatch_validated(&mut self, message: MidiMessage) {
        if let Ok(parsed) = parse(message) {
            self.dispatch_parsed(parsed);
        }
    }

    /// Fill a mono block while applying ordered MIDI messages at sample offsets.
    ///
    /// The complete stream is validated before output or engine state changes.
    /// This callback-safe method performs no allocation or locking.
    ///
    /// # Errors
    ///
    /// Returns [`MidiError::InvalidTiming`] for unordered/out-of-block offsets,
    /// or a parsing error for malformed or unsupported messages.
    pub fn process(
        &mut self,
        output: &mut [f32],
        messages: &[TimedMidiMessage],
    ) -> Result<(), MidiError> {
        validate_timed_messages(messages, output.len())?;
        let mut cursor = 0;
        for timed in messages {
            self.synth.render(&mut output[cursor..timed.sample_offset]);
            // Parsing cannot fail after the complete pre-validation pass.
            if let Ok(parsed) = parse(timed.message) {
                self.dispatch_parsed(parsed);
            }
            cursor = timed.sample_offset;
        }
        self.synth.render(&mut output[cursor..]);
        Ok(())
    }

    fn dispatch_parsed(&mut self, message: ParsedMessage) {
        match message {
            ParsedMessage::NoteOn {
                channel,
                note,
                velocity,
            } => {
                let mapping = self.mappings[usize::from(channel)][usize::from(note)];
                for (index, layer) in mapping.layers.into_iter().enumerate() {
                    if let Some(layer) = layer {
                        let frequency_hz = match layer.pitch {
                            MidiPitch::Note => midi_frequency(note),
                            MidiPitch::Fixed(frequency) => frequency,
                        };
                        self.synth.dispatch_validated(Event::NoteOn {
                            id: midi_voice_id(channel, note, index),
                            instrument: layer.instrument,
                            frequency_hz,
                            gain: layer.gain * (f32::from(velocity) / 127.0),
                        });
                    }
                }
            }
            ParsedMessage::NoteOff { channel, note } => {
                self.visit_note_ids(channel, note, false);
            }
            ParsedMessage::AllNotesOff(channel) => {
                for note in 0_u8..=127 {
                    self.visit_note_ids(channel, note, false);
                }
            }
            ParsedMessage::AllSoundOff(channel) => {
                for note in 0_u8..=127 {
                    self.visit_note_ids(channel, note, true);
                }
            }
        }
    }

    fn visit_note_ids(&mut self, channel: u8, note: u8, stop: bool) {
        for index in 0..LAYERS {
            let id = midi_voice_id(channel, note, index);
            if stop {
                self.synth.stop_id(id);
            } else {
                self.synth.release_id(id);
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ParsedMessage {
    NoteOn { channel: u8, note: u8, velocity: u8 },
    NoteOff { channel: u8, note: u8 },
    AllNotesOff(u8),
    AllSoundOff(u8),
}

fn parse(message: MidiMessage) -> Result<ParsedMessage, MidiError> {
    let bytes = message.as_bytes();
    let status = bytes[0];
    if status < 0x80 {
        return Err(MidiError::MalformedMessage);
    }
    let kind = status & 0xf0;
    if !matches!(kind, 0x80 | 0x90 | 0xb0) {
        return Err(MidiError::UnsupportedMessage);
    }
    if bytes.len() != 3 || bytes[1] >= 0x80 || bytes[2] >= 0x80 {
        return Err(MidiError::MalformedMessage);
    }
    let channel = status & 0x0f;
    match kind {
        0x80 => Ok(ParsedMessage::NoteOff {
            channel,
            note: bytes[1],
        }),
        0x90 if bytes[2] == 0 => Ok(ParsedMessage::NoteOff {
            channel,
            note: bytes[1],
        }),
        0x90 => Ok(ParsedMessage::NoteOn {
            channel,
            note: bytes[1],
            velocity: bytes[2],
        }),
        0xb0 if bytes[1] == 120 => Ok(ParsedMessage::AllSoundOff(channel)),
        0xb0 if bytes[1] == 123 => Ok(ParsedMessage::AllNotesOff(channel)),
        _ => Err(MidiError::UnsupportedMessage),
    }
}

pub(crate) fn validate_timed_messages(
    messages: &[TimedMidiMessage],
    output_len: usize,
) -> Result<(), MidiError> {
    let mut previous = 0;
    for timed in messages {
        if timed.sample_offset > output_len || timed.sample_offset < previous {
            return Err(MidiError::InvalidTiming);
        }
        parse(timed.message)?;
        previous = timed.sample_offset;
    }
    Ok(())
}

fn mapping_slot(channel: u8, note: u8) -> Result<(usize, usize), MidiError> {
    if channel >= 16 {
        return Err(MidiError::InvalidChannel);
    }
    if note >= 128 {
        return Err(MidiError::InvalidNote);
    }
    Ok((usize::from(channel), usize::from(note)))
}

fn validate_layer(layer: MidiLayer) -> Result<(), MidiError> {
    if !layer.gain.is_finite() || !(0.0..=1.0).contains(&layer.gain) {
        return Err(MidiError::InvalidLayerSettings);
    }
    if let MidiPitch::Fixed(frequency) = layer.pitch {
        if !frequency.is_finite() || frequency <= 0.0 {
            return Err(MidiError::InvalidLayerSettings);
        }
    }
    Ok(())
}

fn percussion_frequency(instrument: Instrument) -> f32 {
    match instrument {
        Instrument::BassDrum => 60.0,
        Instrument::Tom => 130.0,
        Instrument::Snare => 180.0,
        Instrument::HiHat => 6_000.0,
        _ => 440.0,
    }
}

const fn instrument_code(instrument: Instrument) -> u8 {
    match instrument {
        Instrument::Sine => 0,
        Instrument::Square => 1,
        Instrument::Triangle => 2,
        Instrument::Bass => 3,
        Instrument::Pad => 4,
        Instrument::Lead => 5,
        Instrument::BassDrum => 6,
        Instrument::Tom => 7,
        Instrument::Snare => 8,
        Instrument::HiHat => 9,
    }
}

const fn instrument_from_code(code: u8) -> Option<Instrument> {
    match code {
        0 => Some(Instrument::Sine),
        1 => Some(Instrument::Square),
        2 => Some(Instrument::Triangle),
        3 => Some(Instrument::Bass),
        4 => Some(Instrument::Pad),
        5 => Some(Instrument::Lead),
        6 => Some(Instrument::BassDrum),
        7 => Some(Instrument::Tom),
        8 => Some(Instrument::Snare),
        9 => Some(Instrument::HiHat),
        _ => None,
    }
}

fn midi_frequency(note: u8) -> f32 {
    440.0 * 2.0_f32.powf((f32::from(note) - 69.0) / 12.0)
}

fn midi_voice_id(channel: u8, note: u8, layer_index: usize) -> VoiceId {
    let layer = u64::try_from(layer_index).unwrap_or(u64::MAX);
    VoiceId((1_u64 << 63) | (u64::from(channel) << 56) | (u64::from(note) << 48) | layer)
}

#[cfg(test)]
mod tests {
    use super::{
        MidiError, MidiMessage, MidiPitch, MidiSynth, ParsedMessage, midi_frequency, parse,
    };
    use crate::{Instrument, Preset};

    fn message(bytes: &[u8]) -> MidiMessage {
        MidiMessage::new(bytes).unwrap()
    }

    #[test]
    fn parses_supported_channel_messages() {
        for channel in 0..16 {
            assert_eq!(
                parse(message(&[0x90 | channel, 127, 64])),
                Ok(ParsedMessage::NoteOn {
                    channel,
                    note: 127,
                    velocity: 64
                })
            );
            assert_eq!(
                parse(message(&[0x80 | channel, 0, 0])),
                Ok(ParsedMessage::NoteOff { channel, note: 0 })
            );
            assert_eq!(
                parse(message(&[0x90 | channel, 42, 0])),
                Ok(ParsedMessage::NoteOff { channel, note: 42 })
            );
            assert_eq!(
                parse(message(&[0xb0 | channel, 123, 0])),
                Ok(ParsedMessage::AllNotesOff(channel))
            );
            assert_eq!(
                parse(message(&[0xb0 | channel, 120, 0])),
                Ok(ParsedMessage::AllSoundOff(channel))
            );
        }
    }

    #[test]
    fn rejects_bad_lengths_data_and_statuses() {
        assert_eq!(
            parse(message(&[0x90, 60])),
            Err(MidiError::MalformedMessage)
        );
        assert_eq!(
            parse(message(&[0x90, 60, 1, 2])),
            Err(MidiError::MalformedMessage)
        );
        assert_eq!(
            parse(message(&[0x90, 0x80, 1])),
            Err(MidiError::MalformedMessage)
        );
        assert_eq!(
            parse(message(&[0x70, 60, 1])),
            Err(MidiError::MalformedMessage)
        );
        assert_eq!(
            parse(message(&[0xe0, 0, 0])),
            Err(MidiError::UnsupportedMessage)
        );
        assert_eq!(
            parse(message(&[0xb0, 1, 0])),
            Err(MidiError::UnsupportedMessage)
        );
    }

    #[test]
    fn equal_tempered_pitch_has_expected_reference() {
        assert!((midi_frequency(69) - 440.0).abs() < f32::EPSILON);
        assert!((midi_frequency(81) - 880.0).abs() < 0.001);
    }

    #[test]
    fn percussion_preset_populates_one_synth_on_all_keys_and_channels() {
        let mut midi = MidiSynth::<8, 2>::new(48_000.0).unwrap();
        midi.select_preset(Preset::PercussionKit);

        for channel in &midi.mappings {
            for mapping in channel {
                assert!(mapping.layers[0].is_some());
                assert!(mapping.layers[1].is_none());
            }
        }
        let channel = &midi.mappings[9];
        assert_eq!(
            channel[35].layers[0].unwrap().instrument,
            Instrument::BassDrum
        );
        assert_eq!(channel[38].layers[0].unwrap().instrument, Instrument::Snare);
        assert_eq!(channel[41].layers[0].unwrap().instrument, Instrument::Tom);
        assert_eq!(channel[42].layers[0].unwrap().instrument, Instrument::HiHat);
        assert_eq!(channel[44].layers[0].unwrap().instrument, Instrument::HiHat);
        assert_eq!(channel[46].layers[0].unwrap().instrument, Instrument::HiHat);
        assert_eq!(channel[49].layers[0].unwrap().instrument, Instrument::Tom);
        assert_eq!(
            channel[42].layers[0].unwrap().pitch,
            MidiPitch::Fixed(6_000.0)
        );
    }
}
