use core::ops::Range;

use crate::effects::ChannelEffects;
use crate::engine::validate_events;
use crate::midi::{
    MidiError, MidiLayer, MidiMessage, MidiSynth, TimedMidiMessage, validate_timed_messages,
};
use crate::state::{Reader, push_f32, push_u16, push_u32};
use crate::{EffectSettings, Event, Preset, ProcessError, StateError, Synth, TimedEvent};

const AUDIO_STATE_MAGIC: &[u8; 4] = b"TVAS";
const AUDIO_STATE_VERSION: u16 = 3;

/// A polyphonic synthesizer, per-channel input mixer, and post-effects chain.
///
/// The channel count is selected during construction and may be any nonzero
/// value. Processing is in-place: each channel initially contains its audio
/// input. The same synthesized sample is available to every channel as a dry
/// source or vocoder carrier. Vocoder/dry mixing precedes distortion,
/// three-band EQ, compression, and reverb. Construction allocates effect delay
/// storage and initializes fixed vocoder state; dispatch and processing do not
/// allocate.
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
    /// Toggling a stateful effect clears that effect's previous DSP state. No
    /// settings change occurs when validation fails.
    ///
    /// # Errors
    ///
    /// Returns a control-specific [`ProcessError`] for an out-of-range or
    /// non-finite control value.
    pub fn set_effect_settings(&mut self, settings: EffectSettings) -> Result<(), ProcessError> {
        validate_effect_settings(settings)?;
        for effects in &mut self.channel_effects {
            if settings.vocoder_enabled != self.settings.vocoder_enabled {
                effects.reset_vocoder();
            }
            if settings.reverb_enabled != self.settings.reverb_enabled {
                effects.reset_reverb();
            }
            if settings.compressor_enabled != self.settings.compressor_enabled {
                effects.reset_compressor();
            }
            if settings.eq_enabled != self.settings.eq_enabled {
                effects.reset_equalizer();
            }
        }
        self.settings = settings;
        Ok(())
    }

    /// Enable or bypass the 16-band vocoder.
    pub fn set_vocoder_enabled(&mut self, enabled: bool) {
        if enabled != self.settings.vocoder_enabled {
            for effects in &mut self.channel_effects {
                effects.reset_vocoder();
            }
            self.settings.vocoder_enabled = enabled;
        }
    }

    /// Set the vocoder dry/wet mix in `0.0..=1.0`.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessError::InvalidVocoderMix`] for an out-of-range or
    /// non-finite value.
    pub fn set_vocoder_mix(&mut self, mix: f32) -> Result<(), ProcessError> {
        if !mix.is_finite() || !(0.0..=1.0).contains(&mix) {
            return Err(ProcessError::InvalidVocoderMix);
        }
        self.settings.vocoder_mix = mix;
        Ok(())
    }

    /// Set the vocoder modulator sensitivity in `0.0..=1.0`.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessError::InvalidVocoderSensitivity`] for an out-of-range
    /// or non-finite value.
    pub fn set_vocoder_sensitivity(&mut self, sensitivity: f32) -> Result<(), ProcessError> {
        if !sensitivity.is_finite() || !(0.0..=1.0).contains(&sensitivity) {
            return Err(ProcessError::InvalidVocoderSensitivity);
        }
        self.settings.vocoder_sensitivity = sensitivity;
        Ok(())
    }

    /// Enable or bypass reverb.
    pub fn set_reverb_enabled(&mut self, enabled: bool) {
        if enabled != self.settings.reverb_enabled {
            for effects in &mut self.channel_effects {
                effects.reset_reverb();
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

    /// Enable or bypass the one-knob compressor.
    pub fn set_compressor_enabled(&mut self, enabled: bool) {
        if enabled != self.settings.compressor_enabled {
            for effects in &mut self.channel_effects {
                effects.reset_compressor();
            }
            self.settings.compressor_enabled = enabled;
        }
    }

    /// Set compression strength in `0.0..=1.0`.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessError::InvalidCompressorAmount`] for an out-of-range or
    /// non-finite value.
    pub fn set_compressor_amount(&mut self, amount: f32) -> Result<(), ProcessError> {
        if !amount.is_finite() || !(0.0..=1.0).contains(&amount) {
            return Err(ProcessError::InvalidCompressorAmount);
        }
        self.settings.compressor_amount = amount;
        Ok(())
    }

    /// Enable or bypass the three-band equalizer.
    pub fn set_eq_enabled(&mut self, enabled: bool) {
        if enabled != self.settings.eq_enabled {
            for effects in &mut self.channel_effects {
                effects.reset_equalizer();
            }
            self.settings.eq_enabled = enabled;
        }
    }

    /// Set all three equalizer band gains in decibels.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessError::InvalidEqGain`] if any value is non-finite or
    /// outside `-12.0..=12.0`.
    pub fn set_eq_gains(
        &mut self,
        low_db: f32,
        mid_db: f32,
        high_db: f32,
    ) -> Result<(), ProcessError> {
        validate_eq_gain(low_db)?;
        validate_eq_gain(mid_db)?;
        validate_eq_gain(high_db)?;
        self.settings.eq_low_db = low_db;
        self.settings.eq_mid_db = mid_db;
        self.settings.eq_high_db = high_db;
        Ok(())
    }

    /// Set the equalizer low-band gain in decibels.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessError::InvalidEqGain`] for an out-of-range or non-finite
    /// value.
    pub fn set_eq_low_db(&mut self, gain_db: f32) -> Result<(), ProcessError> {
        validate_eq_gain(gain_db)?;
        self.settings.eq_low_db = gain_db;
        Ok(())
    }

    /// Set the equalizer mid-band gain in decibels.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessError::InvalidEqGain`] for an out-of-range or non-finite
    /// value.
    pub fn set_eq_mid_db(&mut self, gain_db: f32) -> Result<(), ProcessError> {
        validate_eq_gain(gain_db)?;
        self.settings.eq_mid_db = gain_db;
        Ok(())
    }

    /// Set the equalizer high-band gain in decibels.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessError::InvalidEqGain`] for an out-of-range or non-finite
    /// value.
    pub fn set_eq_high_db(&mut self, gain_db: f32) -> Result<(), ProcessError> {
        validate_eq_gain(gain_db)?;
        self.settings.eq_high_db = gain_db;
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

    /// Immediately clear voices, oscillators, controller state, and effect tails.
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
        output.push(u8::from(self.settings.compressor_enabled));
        push_f32(&mut output, self.settings.compressor_amount);
        output.push(u8::from(self.settings.eq_enabled));
        push_f32(&mut output, self.settings.eq_low_db);
        push_f32(&mut output, self.settings.eq_mid_db);
        push_f32(&mut output, self.settings.eq_high_db);
        output.push(u8::from(self.settings.vocoder_enabled));
        push_f32(&mut output, self.settings.vocoder_mix);
        push_f32(&mut output, self.settings.vocoder_sensitivity);
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
        let version = reader.u16()?;
        if !matches!(version, 1 | 2 | AUDIO_STATE_VERSION) {
            return Err(StateError::UnsupportedVersion);
        }
        let midi_len = usize::try_from(reader.u32()?).map_err(|_| StateError::InvalidData)?;
        let midi_state = reader.read_exact(midi_len)?;
        let mut settings = EffectSettings {
            reverb_enabled: read_bool(&mut reader)?,
            reverb_amount: reader.f32()?,
            distortion_enabled: read_bool(&mut reader)?,
            distortion_drive: reader.f32()?,
            ..EffectSettings::default()
        };
        if version >= 2 {
            settings.compressor_enabled = read_bool(&mut reader)?;
            settings.compressor_amount = reader.f32()?;
            settings.eq_enabled = read_bool(&mut reader)?;
            settings.eq_low_db = reader.f32()?;
            settings.eq_mid_db = reader.f32()?;
            settings.eq_high_db = reader.f32()?;
        }
        if version >= 3 {
            settings.vocoder_enabled = read_bool(&mut reader)?;
            settings.vocoder_mix = reader.f32()?;
            settings.vocoder_sensitivity = reader.f32()?;
        }
        if !reader.is_finished() {
            return Err(StateError::InvalidData);
        }
        validate_effect_settings(settings).map_err(|_| StateError::InvalidConfiguration)?;
        self.midi.load_state(midi_state)?;
        // Validation above makes this infallible and preserves effect-toggle
        // reset behavior.
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
                channel[frame] = effects.process(channel[frame], synthesized, self.settings);
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
    if !settings.vocoder_mix.is_finite() || !(0.0..=1.0).contains(&settings.vocoder_mix) {
        return Err(ProcessError::InvalidVocoderMix);
    }
    if !settings.vocoder_sensitivity.is_finite()
        || !(0.0..=1.0).contains(&settings.vocoder_sensitivity)
    {
        return Err(ProcessError::InvalidVocoderSensitivity);
    }
    if !settings.reverb_amount.is_finite() || !(0.0..=1.0).contains(&settings.reverb_amount) {
        return Err(ProcessError::InvalidReverbAmount);
    }
    if !settings.distortion_drive.is_finite() || !(1.0..=20.0).contains(&settings.distortion_drive)
    {
        return Err(ProcessError::InvalidDistortionDrive);
    }
    if !settings.compressor_amount.is_finite() || !(0.0..=1.0).contains(&settings.compressor_amount)
    {
        return Err(ProcessError::InvalidCompressorAmount);
    }
    validate_eq_gain(settings.eq_low_db)?;
    validate_eq_gain(settings.eq_mid_db)?;
    validate_eq_gain(settings.eq_high_db)?;
    Ok(())
}

fn validate_eq_gain(gain_db: f32) -> Result<(), ProcessError> {
    if !gain_db.is_finite() || !(-12.0..=12.0).contains(&gain_db) {
        return Err(ProcessError::InvalidEqGain);
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
                vocoder_enabled: false,
                vocoder_mix: 1.0,
                vocoder_sensitivity: 0.5,
                reverb_enabled: true,
                reverb_amount: 0.5,
                distortion_enabled: true,
                distortion_drive: 4.0,
                compressor_enabled: true,
                compressor_amount: 0.6,
                eq_enabled: true,
                eq_low_db: 2.0,
                eq_mid_db: -1.0,
                eq_high_db: 3.0,
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
    fn full_wet_vocoder_uses_input_channels_as_independent_modulators() {
        let mut processor = AudioProcessor::<1>::new(48_000.0, 2).unwrap();
        processor.select_preset(Preset::Square);
        processor
            .set_effect_settings(EffectSettings {
                vocoder_enabled: true,
                vocoder_mix: 1.0,
                vocoder_sensitivity: 0.75,
                ..EffectSettings::default()
            })
            .unwrap();
        processor
            .dispatch_midi(MidiMessage::new(&[0x90, 45, 127]).unwrap())
            .unwrap();

        let mut silent_modulator = [0.0; 4_096];
        let mut active_modulator = [0.2; 4_096];
        processor
            .process(&mut [&mut silent_modulator, &mut active_modulator], &[])
            .unwrap();

        assert!(silent_modulator.iter().all(|sample| *sample == 0.0));
        assert!(
            active_modulator[512..]
                .iter()
                .any(|sample| sample.abs() > 1.0e-5)
        );
        assert!(active_modulator.iter().all(|sample| sample.is_finite()));
    }

    #[test]
    fn full_wet_vocoder_does_not_leak_a_modulator_without_a_carrier() {
        let mut processor = AudioProcessor::<1>::new(48_000.0, 1).unwrap();
        processor
            .set_effect_settings(EffectSettings {
                vocoder_enabled: true,
                vocoder_mix: 1.0,
                vocoder_sensitivity: 1.0,
                ..EffectSettings::default()
            })
            .unwrap();
        let mut input = [0.25; 1_024];
        processor.process(&mut [&mut input], &[]).unwrap();
        assert!(input.iter().all(|sample| *sample == 0.0));
    }

    #[test]
    fn vocoder_zero_mix_is_the_existing_dry_path_and_toggle_resets_state() {
        let mut processor = AudioProcessor::<1>::new(48_000.0, 1).unwrap();
        processor.select_preset(Preset::Square);
        processor.set_vocoder_enabled(true);
        processor.set_vocoder_mix(0.0).unwrap();
        processor
            .dispatch_midi(MidiMessage::new(&[0x90, 45, 127]).unwrap())
            .unwrap();
        let mut input = [0.2; 512];
        processor.process(&mut [&mut input], &[]).unwrap();
        assert!(input.iter().any(|sample| (*sample - 0.2).abs() > 1.0e-5));

        processor.set_vocoder_mix(1.0).unwrap();
        processor.set_vocoder_enabled(false);
        processor.set_vocoder_enabled(true);
        let mut no_modulator = [0.0; 512];
        processor.process(&mut [&mut no_modulator], &[]).unwrap();
        assert!(no_modulator.iter().all(|sample| *sample == 0.0));
    }

    #[test]
    fn invalid_midi_is_rejected_before_audio_or_voice_state_changes() {
        let mut processor = AudioProcessor::<1>::new(48_000.0, 1).unwrap();
        let unsupported = MidiMessage::new(&[0xd0, 0, 0]).unwrap();
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
        assert_eq!(
            processor.set_vocoder_mix(-0.1),
            Err(ProcessError::InvalidVocoderMix)
        );
        assert_eq!(
            processor.set_vocoder_sensitivity(f32::INFINITY),
            Err(ProcessError::InvalidVocoderSensitivity)
        );
        assert_eq!(
            processor.set_compressor_amount(1.1),
            Err(ProcessError::InvalidCompressorAmount)
        );
        assert_eq!(
            processor.set_eq_gains(0.0, f32::NAN, 0.0),
            Err(ProcessError::InvalidEqGain)
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
                vocoder_enabled: true,
                vocoder_mix: 0.75,
                vocoder_sensitivity: 0.625,
                reverb_enabled: true,
                reverb_amount: 0.375,
                distortion_enabled: true,
                distortion_drive: 7.5,
                compressor_enabled: true,
                compressor_amount: 0.625,
                eq_enabled: true,
                eq_low_db: 4.0,
                eq_mid_db: -2.5,
                eq_high_db: 1.25,
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

        // Version 2 states ended after the EQ gains and supply bypassed
        // vocoder defaults.
        let mut version_two = state.clone();
        version_two[4..6].copy_from_slice(&2_u16.to_le_bytes());
        version_two.truncate(version_two.len() - 9);
        let mut legacy = AudioProcessor::<4, 1>::new(48_000.0, 1).unwrap();
        legacy.load_state(&version_two).unwrap();
        let legacy_settings = legacy.effect_settings();
        assert!(!legacy_settings.vocoder_enabled);
        assert_eq!(legacy_settings.vocoder_mix, 1.0);
        assert_eq!(legacy_settings.vocoder_sensitivity, 0.5);
        assert!(legacy_settings.compressor_enabled);
        assert!(legacy_settings.eq_enabled);

        // Version 1 states ended after the distortion drive and also supply
        // bypassed defaults for the version 2 effects.
        let mut version_one = version_two;
        version_one[4..6].copy_from_slice(&1_u16.to_le_bytes());
        version_one.truncate(version_one.len() - 18);
        legacy.load_state(&version_one).unwrap();
        let legacy_settings = legacy.effect_settings();
        assert!(legacy_settings.reverb_enabled);
        assert_eq!(legacy_settings.reverb_amount, 0.375);
        assert!(legacy_settings.distortion_enabled);
        assert_eq!(legacy_settings.distortion_drive, 7.5);
        assert!(!legacy_settings.compressor_enabled);
        assert!(!legacy_settings.eq_enabled);
        assert!(!legacy_settings.vocoder_enabled);
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

        let mut invalid_vocoder = before.clone();
        let sensitivity = invalid_vocoder.len() - size_of::<f32>();
        invalid_vocoder[sensitivity..].copy_from_slice(&f32::NAN.to_le_bytes());
        assert_eq!(
            processor.load_state(&invalid_vocoder),
            Err(StateError::InvalidConfiguration)
        );
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
