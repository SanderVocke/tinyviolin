use crate::Instrument;

/// Stable identity used to release or retrigger a note.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct VoiceId(pub u64);

/// A control event for [`crate::Synth`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    /// Start a voice, replacing any currently active voice with the same ID.
    NoteOn {
        /// Identity later supplied to [`Event::NoteOff`].
        id: VoiceId,
        /// Preset to synthesize.
        instrument: Instrument,
        /// Positive finite fundamental frequency in hertz.
        frequency_hz: f32,
        /// Finite linear gain in the inclusive range `0.0..=1.0`.
        gain: f32,
    },
    /// Put all voices with this identity into their release phase.
    NoteOff(VoiceId),
    /// Put every active voice into its release phase.
    AllNotesOff,
}

/// An event positioned relative to the beginning of one output block.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TimedEvent {
    /// Sample at which to apply the event; `output.len()` means block end.
    pub sample_offset: usize,
    /// Event to apply.
    pub event: Event,
}

impl TimedEvent {
    /// Construct a timed event.
    #[must_use]
    pub const fn new(sample_offset: usize, event: Event) -> Self {
        Self {
            sample_offset,
            event,
        }
    }
}

/// A configuration or event-stream error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProcessError {
    /// The sample rate was non-finite, below 1 Hz, or above 768 kHz.
    InvalidSampleRate,
    /// A zero-sized voice pool was requested.
    ZeroVoices,
    /// A zero-channel audio processor was requested.
    ZeroChannels,
    /// A zero-sized MIDI mapping layer set was requested.
    ZeroMidiLayers,
    /// A processing block did not match the configured channel count.
    ChannelCountMismatch,
    /// Channels in a processing block had different lengths.
    ChannelLengthMismatch,
    /// A requested frame range was reversed or outside the block.
    InvalidFrameRange,
    /// Reverb amount was non-finite or outside `0.0..=1.0`.
    InvalidReverbAmount,
    /// Distortion drive was non-finite or outside `1.0..=20.0`.
    InvalidDistortionDrive,
    /// Compressor amount was non-finite or outside `0.0..=1.0`.
    InvalidCompressorAmount,
    /// An equalizer band gain was non-finite or outside `-12.0..=12.0` dB.
    InvalidEqGain,
    /// A note frequency was not positive and finite.
    InvalidFrequency,
    /// A gain was non-finite or outside `0.0..=1.0`.
    InvalidGain,
    /// Timed events were not in nondecreasing sample-offset order.
    EventsNotOrdered,
    /// An event offset was greater than the output block length.
    EventOutsideBlock,
}

impl core::fmt::Display for ProcessError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::InvalidSampleRate => "sample rate must be finite and in 1..=768000 Hz",
            Self::ZeroVoices => "voice capacity must be nonzero",
            Self::ZeroChannels => "audio channel count must be nonzero",
            Self::ZeroMidiLayers => "MIDI layer capacity must be nonzero",
            Self::ChannelCountMismatch => "audio block channel count does not match configuration",
            Self::ChannelLengthMismatch => "audio channels must have equal lengths",
            Self::InvalidFrameRange => "frame range is outside the audio block",
            Self::InvalidReverbAmount => "reverb amount must be finite and in 0..=1",
            Self::InvalidDistortionDrive => "distortion drive must be finite and in 1..=20",
            Self::InvalidCompressorAmount => "compressor amount must be finite and in 0..=1",
            Self::InvalidEqGain => "equalizer gains must be finite and in -12..=12 dB",
            Self::InvalidFrequency => "frequency must be positive and finite",
            Self::InvalidGain => "gain must be finite and in 0..=1",
            Self::EventsNotOrdered => "events must be ordered by sample offset",
            Self::EventOutsideBlock => "event sample offset is outside the block",
        })
    }
}

impl std::error::Error for ProcessError {}
