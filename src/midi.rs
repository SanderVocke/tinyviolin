//! Fixed-size MIDI message and mapping types.

use crate::Instrument;

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
    /// than [`MAX_MESSAGE_BYTES`].
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
    /// Ignore note number and use this frequency in hertz.
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
    /// A status or data byte was malformed.
    MalformedMessage,
    /// The status is well-formed but outside the supported subset.
    UnsupportedMessage,
    /// Channel was outside `0..16`.
    InvalidChannel,
    /// Note was outside `0..128`.
    InvalidNote,
    /// Layer index exceeded the wrapper's fixed layer count.
    InvalidLayer,
    /// Layer gain or fixed frequency was invalid.
    InvalidLayerSettings,
    /// Timed messages were unordered or outside the output block.
    InvalidTiming,
    /// The underlying synthesizer rejected configuration or processing.
    Synth,
}

impl core::fmt::Display for MidiError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::InvalidLength => "MIDI message length must be in 1..=4",
            Self::MalformedMessage => "malformed MIDI message",
            Self::UnsupportedMessage => "unsupported MIDI message",
            Self::InvalidChannel => "MIDI channel must be in 0..16",
            Self::InvalidNote => "MIDI note must be in 0..128",
            Self::InvalidLayer => "MIDI layer index exceeds fixed capacity",
            Self::InvalidLayerSettings => "MIDI layer frequency or gain is invalid",
            Self::InvalidTiming => "timed MIDI messages are unordered or outside the block",
            Self::Synth => "synthesizer rejected MIDI processing",
        })
    }
}

impl std::error::Error for MidiError {}
