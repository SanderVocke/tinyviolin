use core::ops::Range;

use crate::effects::ChannelEffects;
use crate::engine::validate_events;
use crate::midi::{
    MidiError, MidiLayer, MidiMessage, MidiSynth, TimedMidiMessage, validate_timed_messages,
};
use crate::state::{Reader, push_f32, push_u16, push_u32};
use crate::{EffectSettings, Event, Preset, ProcessError, StateError, Synth, TimedEvent};

const AUDIO_STATE_MAGIC: &[u8; 4] = b"TVAS";
const AUDIO_STATE_VERSION: u16 = 1;

/// A polyphonic synthesizer, per-channel input mixer, and post-effects chain.
///
/// The channel count is selected during construction and may be any nonzero
/// value. Processing is in-place: each channel initially contains its audio
/// input. The same synthesized sample is added to every channel, then each
/// channel is independently passed through distortion and reverb. Construction
/// allocates effect delay storage; dispatch and processing do not allocate.
pub struct AudioProcessor<const VOICES: usize = 32, const MIDI_LAYERS: usize = 2> {
    midi: MidiSynth<VOICES, MIDI_LAYERS>,
    channel_effects: Vec<ChannelEffects>,
    settings: EffectSettings,
}

/// An error from multichannel processing with MIDI events.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AudioMidiError {
    /// Audio block or processor configuration was invalid.
    Process(ProcessError),
    /// MIDI mapping, message, or timing was invalid.
    Midi(MidiError),
}

impl core::fmt::Display for AudioMidiError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Process(error) => write!(formatter, "audio processing failed: {error}"),
            Self::Midi(error) => write!(formatter, "MIDI processing failed: {error}"),
        }
    }
}

impl std::error::Error for AudioMidiError {}

impl From<ProcessError> for AudioMidiError {
    fn from(error: ProcessError) -> Self {
        Self::Process(error)
    }
}

impl From<MidiError> for AudioMidiError {
    fn from(error: MidiError) -> Self {
        Self::Midi(error)
    }
}

impl<const VOICES: usize, const MIDI_LAYERS: usize> AudioProcessor<VOICES, MIDI_LAYERS> {
    /// Construct a processor for a fixed sample rate and channel count.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessError::ZeroChannels`] for zero channels,
    /// [`ProcessError::ZeroMidiLayers`] when `MIDI_LAYERS` is zero, or the same
    /// sample-rate and voice-capacity errors as [`Synth::new`].
    pub fn new(sample_rate: f32, channel_count: usize) -> Result<Self, ProcessError> {
        if channel_count == 0 {
            return Err(ProcessError::ZeroChannels);
        }
        let synth = Synth::new(sample_rate)?;
        let midi = MidiSynth::from_synth(synth).map_err(|_| ProcessError::ZeroMidiLayers)?;
        let channel_effects = (0..channel_count)
            .map(|_| ChannelEffects::new(sample_rate))
            .collect();
        Ok(Self {
            midi,
            channel_effects,
            settings: EffectSettings::default(),
        })
    }

    /// Return the configured sample rate in hertz.
    #[must_use]
    pub const fn sample_rate(&self) -> f32 {
        self.midi.engine().sample_rate()
    }

    /// Return the configured number of input/output channels.
    #[must_use]
    pub fn channel_count(&self) -> usize {
        self.channel_effects.len()
    }

    /// Return the current post-effect settings.
    #[must_use]
    pub const fn effect_settings(&self) -> EffectSettings {
        self.settings
    }

    /// Replace all post-effect settings.
    ///
    /// Disabling or re-enabling reverb clears its previous tail. No settings
    /// change occurs when validation fails.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessError::InvalidReverbAmount`] or
    /// [`ProcessError::InvalidDistortionDrive`] for an out-of-range or
    /// non-finite control value.
    pub fn set_effect_settings(&mut self, settings: EffectSettings) -> Result<(), ProcessError> {
        validate_effect_settings(settings)?;
        if settings.reverb_enabled != self.settings.reverb_enabled {
            for effects in &mut self.channel_effects {
                effects.reset_dsp();
            }
        }
        self.settings = settings;
        Ok(())
    }

    /// Enable or bypass reverb.
    pub fn set_reverb_enabled(&mut self, enabled: bool) {
        if enabled != self.settings.reverb_enabled {
            for effects in &mut self.channel_effects {
                effects.reset_dsp();
            }
            self.settings.reverb_enabled = enabled;
        }
    }

    /// Set the reverb dry/wet amount in `0.0..=1.0`.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessError::InvalidReverbAmount`] for an out-of-range or
    /// non-finite value.
    pub fn set_reverb_amount(&mut self, amount: f32) -> Result<(), ProcessError> {
        if !amount.is_finite() || !(0.0..=1.0).contains(&amount) {
            return Err(ProcessError::InvalidReverbAmount);
        }
        self.settings.reverb_amount = amount;
        Ok(())
    }

    /// Enable or bypass distortion.
    pub fn set_distortion_enabled(&mut self, enabled: bool) {
        self.settings.distortion_enabled = enabled;
    }

    /// Set distortion drive in `1.0..=20.0`.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessError::InvalidDistortionDrive`] for an out-of-range or
    /// non-finite value.
    pub fn set_distortion_drive(&mut self, drive: f32) -> Result<(), ProcessError> {
        if !drive.is_finite() || !(1.0..=20.0).contains(&drive) {
            return Err(ProcessError::InvalidDistortionDrive);
        }
        self.settings.distortion_drive = drive;
        Ok(())
    }

    /// Apply one synthesis event before the next processed sample.
    ///
    /// # Errors
    ///
    /// Propagates event validation errors from [`Synth::dispatch`].
    pub fn dispatch(&mut self, event: Event) -> Result<(), ProcessError> {
        self.midi.engine_mut().dispatch(event)
    }

    /// Apply one MIDI message before the next processed sample.
    ///
    /// The message uses the mappings configured with [`Self::set_midi_layer`]
    /// or [`Self::set_midi_channel_layer`].
    ///
    /// # Errors
    ///
    /// Returns a [`MidiError`] for a malformed or unsupported message.
    pub fn dispatch_midi(&mut self, message: MidiMessage) -> Result<(), MidiError> {
        self.midi.dispatch(message)
    }

    /// Set one MIDI channel/note mapping layer.
    ///
    /// # Errors
    ///
    /// Returns a [`MidiError`] for an invalid channel, note, layer, or setting.
    pub fn set_midi_layer(
        &mut self,
        channel: u8,
        note: u8,
        layer_index: usize,
        layer: MidiLayer,
    ) -> Result<(), MidiError> {
        self.midi.set_layer(channel, note, layer_index, layer)
    }

    /// Apply the same MIDI mapping layer to every note on one channel.
    ///
    /// # Errors
    ///
    /// Returns a [`MidiError`] for an invalid channel, layer, or setting.
    pub fn set_midi_channel_layer(
        &mut self,
        channel: u8,
        layer_index: usize,
        layer: MidiLayer,
    ) -> Result<(), MidiError> {
        self.midi.set_channel_layer(channel, layer_index, layer)
    }

    /// Remove one MIDI channel/note mapping layer.
    ///
    /// # Errors
    ///
    /// Returns a [`MidiError`] for an invalid channel, note, or layer.
    pub fn clear_midi_layer(
        &mut self,
        channel: u8,
        note: u8,
        layer_index: usize,
    ) -> Result<(), MidiError> {
        self.midi.clear_layer(channel, note, layer_index)
    }

    /// Return the number of synth voices that have not yet become silent.
    #[must_use]
    pub fn active_voice_count(&self) -> usize {
        self.midi.engine().active_voice_count()
    }

    /// Return every preset available in this library version.
    #[must_use]
    pub const fn available_presets(&self) -> &'static [Preset] {
        self.midi.available_presets()
    }

    /// Return the selected built-in preset, or `None` after manual mapping.
    #[must_use]
    pub const fn selected_preset(&self) -> Option<Preset> {
        self.midi.selected_preset()
    }

    /// Replace all MIDI mappings with a built-in preset on every channel.
    pub fn select_preset(&mut self, preset: Preset) {
        self.midi.select_preset(preset);
    }

    /// Select a built-in preset by its stable runtime ID.
    ///
    /// # Errors
    ///
    /// Returns [`MidiError::UnknownPreset`] when the ID is not advertised by
    /// [`Self::available_presets`].
    pub fn select_preset_by_id(&mut self, id: &str) -> Result<(), MidiError> {
        self.midi.select_preset_by_id(id)
    }

    /// Immediately clear voices, oscillators, and effect tails.
    ///
    /// MIDI mappings and effect settings are preserved. This host panic/reset
    /// operation performs no allocation or locking.
    pub fn reset_dsp(&mut self) {
        self.midi.reset_dsp();
        for effects in &mut self.channel_effects {
            effects.reset_dsp();
        }
    }

    /// Alias for [`Self::reset_dsp`] named after the conventional host action.
    pub fn panic(&mut self) {
        self.reset_dsp();
    }

    /// Serialize MIDI mappings, preset selection, and effect settings.
    ///
    /// Sample rate, channel layout, voices, oscillator phases, and effect tails
    /// are intentionally omitted. This setup operation allocates the returned
    /// byte vector for host session storage.
    #[must_use]
    pub fn serialize_state(&self) -> Vec<u8> {
        let midi_state = self.midi.serialize_state();
        let mut output = Vec::new();
        output.extend_from_slice(AUDIO_STATE_MAGIC);
        push_u16(&mut output, AUDIO_STATE_VERSION);
        push_u32(
            &mut output,
            u32::try_from(midi_state.len()).unwrap_or(u32::MAX),
        );
        output.extend_from_slice(&midi_state);
        output.push(u8::from(self.settings.reverb_enabled));
        push_f32(&mut output, self.settings.reverb_amount);
        output.push(u8::from(self.settings.distortion_enabled));
        push_f32(&mut output, self.settings.distortion_drive);
        output
    }

    /// Load configuration previously returned by [`Self::serialize_state`].
    ///
    /// Loading is transactional for malformed data and does not restore or
    /// clear voices, oscillator phases, or effect tails. Call [`Self::reset_dsp`]
    /// separately if loading a session should also silence the processor.
    ///
    /// # Errors
    ///
    /// Returns a [`StateError`] for malformed, unsupported, incompatible, or
    /// invalid configuration data.
    pub fn load_state(&mut self, state: &[u8]) -> Result<(), StateError> {
        let mut reader = Reader::new(state);
        if reader.read_exact(AUDIO_STATE_MAGIC.len())? != AUDIO_STATE_MAGIC {
            return Err(StateError::InvalidData);
        }
        if reader.u16()? != AUDIO_STATE_VERSION {
            return Err(StateError::UnsupportedVersion);
        }
        let midi_len = usize::try_from(reader.u32()?).map_err(|_| StateError::InvalidData)?;
        let midi_state = reader.read_exact(midi_len)?;
        let settings = EffectSettings {
            reverb_enabled: read_bool(&mut reader)?,
            reverb_amount: reader.f32()?,
            distortion_enabled: read_bool(&mut reader)?,
            distortion_drive: reader.f32()?,
        };
        if !reader.is_finished() {
            return Err(StateError::InvalidData);
        }
        validate_effect_settings(settings).map_err(|_| StateError::InvalidConfiguration)?;
        self.midi.load_state(midi_state)?;
        // Validation above makes this infallible and preserves the usual
        // reverb-toggle behavior.
        self.set_effect_settings(settings)
            .map_err(|_| StateError::InvalidConfiguration)
    }

    /// Mix and process a complete in-place multichannel block with timed events.
    ///
    /// All channels must match the channel count selected in [`Self::new`] and
    /// have the same length. The complete block and event stream are validated
    /// before audio, effect, or synth state changes.
    ///
    /// # Errors
    ///
    /// Returns a [`ProcessError`] for a channel mismatch, unequal channel
    /// lengths, or an invalid event stream.
    pub fn process(
        &mut self,
        channels: &mut [&mut [f32]],
        events: &[TimedEvent],
    ) -> Result<(), ProcessError> {
        let block_len = validate_channels(channels, self.channel_count())?;
        validate_events(events, block_len)?;

        let mut cursor = 0;
        for timed in events {
            self.render_validated(channels, cursor..timed.sample_offset);
            self.midi.engine_mut().dispatch_validated(timed.event);
            cursor = timed.sample_offset;
        }
        self.render_validated(channels, cursor..block_len);
        Ok(())
    }

    /// Mix and process a complete in-place block with timed MIDI messages.
    ///
    /// MIDI note messages use the processor's fixed channel/note mappings. The
    /// complete audio block and MIDI stream are validated before audio, effect,
    /// voice, or mapping state changes.
    ///
    /// # Errors
    ///
    /// Returns [`AudioMidiError::Process`] for invalid channels and
    /// [`AudioMidiError::Midi`] for invalid message data or timing.
    pub fn process_midi(
        &mut self,
        channels: &mut [&mut [f32]],
        messages: &[TimedMidiMessage],
    ) -> Result<(), AudioMidiError> {
        let block_len = validate_channels(channels, self.channel_count())?;
        validate_timed_messages(messages, block_len)?;

        let mut cursor = 0;
        for timed in messages {
            self.render_validated(channels, cursor..timed.sample_offset);
            self.midi.dispatch_validated(timed.message);
            cursor = timed.sample_offset;
        }
        self.render_validated(channels, cursor..block_len);
        Ok(())
    }

    /// Process part of an in-place block without timed events.
    ///
    /// This is useful for hosts that deliver events as a stream: render up to
    /// an event, call [`Self::dispatch`], then render the next range. Validation
    /// occurs before state or audio changes.
    ///
    /// # Errors
    ///
    /// Returns a [`ProcessError`] for invalid channels or a range outside the
    /// block.
    pub fn render_range(
        &mut self,
        channels: &mut [&mut [f32]],
        range: Range<usize>,
    ) -> Result<(), ProcessError> {
        let block_len = validate_channels(channels, self.channel_count())?;
        if range.start > range.end || range.end > block_len {
            return Err(ProcessError::InvalidFrameRange);
        }
        self.render_validated(channels, range);
        Ok(())
    }

    fn render_validated(&mut self, channels: &mut [&mut [f32]], range: Range<usize>) {
        for frame in range {
            let synthesized = self.midi.engine_mut().next_sample();
            for (channel, effects) in channels.iter_mut().zip(&mut self.channel_effects) {
                let mixed = channel[frame] + synthesized;
                channel[frame] = effects.process(mixed, self.settings);
            }
        }
    }
}

fn validate_channels(
    channels: &[&mut [f32]],
    expected_count: usize,
) -> Result<usize, ProcessError> {
    if channels.len() != expected_count {
        return Err(ProcessError::ChannelCountMismatch);
    }
    let block_len = channels.first().map_or(0, |channel| channel.len());
    if channels.iter().any(|channel| channel.len() != block_len) {
        return Err(ProcessError::ChannelLengthMismatch);
    }
    Ok(block_len)
}

fn read_bool(reader: &mut Reader<'_>) -> Result<bool, StateError> {
    match reader.u8()? {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(StateError::InvalidConfiguration),
    }
}

fn validate_effect_settings(settings: EffectSettings) -> Result<(), ProcessError> {
    if !settings.reverb_amount.is_finite() || !(0.0..=1.0).contains(&settings.reverb_amount) {
        return Err(ProcessError::InvalidReverbAmount);
    }
    if !settings.distortion_drive.is_finite() || !(1.0..=20.0).contains(&settings.distortion_drive)
    {
        return Err(ProcessError::InvalidDistortionDrive);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::float_cmp)]

    use super::{AudioMidiError, AudioProcessor};
    use crate::midi::{MidiError, MidiLayer, MidiMessage, MidiPitch, TimedMidiMessage};
    use crate::{
        EffectSettings, Event, Instrument, Preset, ProcessError, StateError, TimedEvent, VoiceId,
    };

    fn note() -> Event {
        Event::NoteOn {
            id: VoiceId(1),
            instrument: Instrument::Square,
            frequency_hz: 100.0,
            gain: 0.5,
        }
    }

    #[test]
    fn synth_is_equal_on_every_channel_and_input_is_preserved() {
        let mut processor = AudioProcessor::<2>::new(1_000.0, 3).unwrap();
        let mut first = [0.1; 8];
        let mut second = [-0.2; 8];
        let mut third = [0.0; 8];
        let mut channels: [&mut [f32]; 3] = [&mut first, &mut second, &mut third];
        processor
            .process(&mut channels, &[TimedEvent::new(0, note())])
            .unwrap();

        for frame in 0..8 {
            assert!((first[frame] - third[frame] - 0.1).abs() < f32::EPSILON);
            assert!((second[frame] - third[frame] + 0.2).abs() < f32::EPSILON);
        }
    }

    #[test]
    fn arbitrary_channel_count_is_not_hard_coded() {
        let mut processor = AudioProcessor::<1>::new(48_000.0, 7).unwrap();
        let mut storage = [[0.0; 4]; 7];
        let mut channels: Vec<&mut [f32]> =
            storage.iter_mut().map(<[f32; 4]>::as_mut_slice).collect();
        processor.process(&mut channels, &[]).unwrap();
        assert_eq!(processor.channel_count(), 7);
    }

    #[test]
    fn timed_midi_uses_mappings_and_the_multichannel_mix_path() {
        let mut processor = AudioProcessor::<2, 1>::new(1_000.0, 2).unwrap();
        processor
            .set_midi_channel_layer(
                3,
                0,
                MidiLayer {
                    instrument: Instrument::Square,
                    pitch: MidiPitch::Note,
                    gain: 0.5,
                },
            )
            .unwrap();
        let note_on = MidiMessage::new(&[0x93, 69, 127]).unwrap();
        let mut left = [0.1; 12];
        let mut right = [-0.1; 12];
        processor
            .process_midi(
                &mut [&mut left, &mut right],
                &[TimedMidiMessage::new(2, note_on)],
            )
            .unwrap();

        assert_eq!(left[..2], [0.1; 2]);
        assert_eq!(right[..2], [-0.1; 2]);
        assert!(left[3..].iter().any(|sample| *sample != 0.1));
        for frame in 2..12 {
            assert!((left[frame] - right[frame] - 0.2).abs() < f32::EPSILON);
        }
    }

    #[test]
    fn midi_triggered_audio_runs_through_enabled_effects() {
        let mut dry = AudioProcessor::<1, 1>::new(1_000.0, 1).unwrap();
        let mut effected = AudioProcessor::<1, 1>::new(1_000.0, 1).unwrap();
        let layer = MidiLayer {
            instrument: Instrument::Square,
            pitch: MidiPitch::Note,
            gain: 0.5,
        };
        dry.set_midi_layer(0, 69, 0, layer).unwrap();
        effected.set_midi_layer(0, 69, 0, layer).unwrap();
        effected
            .set_effect_settings(EffectSettings {
                reverb_enabled: true,
                reverb_amount: 0.5,
                distortion_enabled: true,
                distortion_drive: 4.0,
            })
            .unwrap();
        let events = [
            TimedMidiMessage::new(0, MidiMessage::new(&[0x90, 69, 127]).unwrap()),
            TimedMidiMessage::new(8, MidiMessage::new(&[0x80, 69, 0]).unwrap()),
        ];
        let mut dry_output = [0.0; 128];
        let mut effected_output = [0.0; 128];
        dry.process_midi(&mut [&mut dry_output], &events).unwrap();
        effected
            .process_midi(&mut [&mut effected_output], &events)
            .unwrap();

        assert!(dry_output[80..].iter().all(|sample| *sample == 0.0));
        assert!(
            effected_output[80..]
                .iter()
                .any(|sample| sample.abs() > 0.0)
        );
        assert!(effected_output.iter().all(|sample| sample.is_finite()));
    }

    #[test]
    fn invalid_midi_is_rejected_before_audio_or_voice_state_changes() {
        let mut processor = AudioProcessor::<1>::new(48_000.0, 1).unwrap();
        let unsupported = MidiMessage::new(&[0xe0, 0, 0]).unwrap();
        let mut output = [0.25; 4];
        assert_eq!(
            processor.process_midi(&mut [&mut output], &[TimedMidiMessage::new(0, unsupported)],),
            Err(AudioMidiError::Midi(MidiError::UnsupportedMessage))
        );
        assert_eq!(output, [0.25; 4]);
        assert_eq!(processor.active_voice_count(), 0);
    }

    #[test]
    fn invalid_channels_and_effect_controls_are_rejected() {
        assert!(matches!(
            AudioProcessor::<1>::new(48_000.0, 0),
            Err(ProcessError::ZeroChannels)
        ));
        assert!(matches!(
            AudioProcessor::<1, 0>::new(48_000.0, 1),
            Err(ProcessError::ZeroMidiLayers)
        ));
        let mut processor = AudioProcessor::<1>::new(48_000.0, 2).unwrap();
        let old = processor.effect_settings();
        let invalid = EffectSettings {
            reverb_amount: f32::NAN,
            ..old
        };
        assert_eq!(
            processor.set_effect_settings(invalid),
            Err(ProcessError::InvalidReverbAmount)
        );
        assert_eq!(processor.effect_settings(), old);

        let mut mono = [0.25; 4];
        assert_eq!(
            processor.process(&mut [&mut mono], &[]),
            Err(ProcessError::ChannelCountMismatch)
        );
        assert_eq!(mono, [0.25; 4]);
    }

    #[test]
    fn event_validation_precedes_audio_and_state_changes() {
        let mut processor = AudioProcessor::<1>::new(48_000.0, 2).unwrap();
        let mut left = [0.25; 4];
        let mut right = [0.5; 4];
        let events = [TimedEvent::new(5, note())];
        assert_eq!(
            processor.process(&mut [&mut left, &mut right], &events),
            Err(ProcessError::EventOutsideBlock)
        );
        assert_eq!(left, [0.25; 4]);
        assert_eq!(right, [0.5; 4]);
        assert_eq!(processor.active_voice_count(), 0);
    }

    #[test]
    fn audio_state_round_trips_all_mutable_configuration() {
        let mut source = AudioProcessor::<4, 1>::new(48_000.0, 2).unwrap();
        source.select_preset(Preset::PercussionKit);
        source
            .set_effect_settings(EffectSettings {
                reverb_enabled: true,
                reverb_amount: 0.375,
                distortion_enabled: true,
                distortion_drive: 7.5,
            })
            .unwrap();
        let state = source.serialize_state();

        let mut restored = AudioProcessor::<4, 1>::new(44_100.0, 1).unwrap();
        restored.load_state(&state).unwrap();
        assert_eq!(restored.selected_preset(), Some(Preset::PercussionKit));
        assert_eq!(restored.effect_settings(), source.effect_settings());
        assert_eq!(restored.serialize_state(), state);
        // Stream configuration is fixed by construction and not session state.
        assert_eq!(restored.sample_rate(), 44_100.0);
        assert_eq!(restored.channel_count(), 1);
    }

    #[test]
    fn audio_state_errors_leave_configuration_unchanged() {
        let mut processor = AudioProcessor::<2, 1>::new(48_000.0, 1).unwrap();
        processor.select_preset(Preset::Lead);
        processor.set_distortion_enabled(true);
        let before = processor.serialize_state();
        let mut corrupt = before.clone();
        corrupt.push(0);

        assert_eq!(processor.load_state(&corrupt), Err(StateError::InvalidData));
        assert_eq!(processor.serialize_state(), before);
    }

    #[test]
    fn audio_panic_clears_voices_and_effect_tails_but_not_configuration() {
        let mut processor = AudioProcessor::<2, 1>::new(1_000.0, 1).unwrap();
        processor.select_preset(Preset::Sine);
        processor
            .set_effect_settings(EffectSettings {
                reverb_enabled: true,
                reverb_amount: 1.0,
                ..EffectSettings::default()
            })
            .unwrap();
        processor
            .dispatch_midi(MidiMessage::new(&[0x90, 60, 127]).unwrap())
            .unwrap();
        let mut sounding = [0.0; 100];
        processor.process_midi(&mut [&mut sounding], &[]).unwrap();
        assert!(sounding.iter().any(|sample| sample.abs() > 0.0));

        processor.panic();
        assert_eq!(processor.active_voice_count(), 0);
        assert_eq!(processor.selected_preset(), Some(Preset::Sine));
        assert!(processor.effect_settings().reverb_enabled);
        let mut silent = [0.0; 100];
        processor.process_midi(&mut [&mut silent], &[]).unwrap();
        assert_eq!(silent, [0.0; 100]);
    }
}
