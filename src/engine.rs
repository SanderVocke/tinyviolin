use crate::dsp::Voice;
use crate::{Event, ProcessError, TimedEvent, VoiceId};

/// Fixed-capacity polyphonic synthesizer.
///
/// `VOICES` is the maximum number of simultaneous instrument layers. Buffer
/// size is inferred from the output slice supplied to [`Self::process`].
pub struct Synth<const VOICES: usize = 32> {
    sample_rate: f32,
    voices: [Voice; VOICES],
    age: u64,
}

impl<const VOICES: usize> Synth<VOICES> {
    /// Create a silent engine with a fixed sample rate.
    ///
    /// Construction is a setup operation and is not part of the real-time API.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessError::InvalidSampleRate`] for a non-finite rate or one
    /// outside 1–768,000 Hz, and [`ProcessError::ZeroVoices`] when `VOICES` is 0.
    pub fn new(sample_rate: f32) -> Result<Self, ProcessError> {
        if !sample_rate.is_finite() || !(1.0..=768_000.0).contains(&sample_rate) {
            return Err(ProcessError::InvalidSampleRate);
        }
        if VOICES == 0 {
            return Err(ProcessError::ZeroVoices);
        }
        Ok(Self {
            sample_rate,
            voices: [Voice::EMPTY; VOICES],
            age: 0,
        })
    }

    /// Return the configured sample rate in hertz.
    #[must_use]
    pub const fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    /// Return the number of voices that have not yet become silent.
    #[must_use]
    pub fn active_voice_count(&self) -> usize {
        self.voices.iter().filter(|voice| voice.active).count()
    }

    /// Immediately clear all voice and oscillator DSP state.
    ///
    /// The sample rate and other engine configuration are preserved. This is
    /// suitable for a host's panic/reset action and performs no allocation.
    pub fn reset_dsp(&mut self) {
        self.voices.fill(Voice::EMPTY);
        self.age = 0;
    }

    /// Alias for [`Self::reset_dsp`] named after the conventional host action.
    pub fn panic(&mut self) {
        self.reset_dsp();
    }

    /// Apply one event immediately, before the next generated sample.
    ///
    /// This method performs no allocation or locking.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessError::InvalidFrequency`] or
    /// [`ProcessError::InvalidGain`] for invalid note-on settings. No state is
    /// changed when validation fails.
    pub fn dispatch(&mut self, event: Event) -> Result<(), ProcessError> {
        validate_event(event)?;
        self.dispatch_validated(event);
        Ok(())
    }

    /// Fill a mono output block and apply ordered, sample-timed events.
    ///
    /// The method validates the complete event slice before changing engine or
    /// output state. It performs no allocation or locking. Output is always in
    /// `-1.0..=1.0` when processing succeeds. Frequencies above the representable
    /// audio band are deterministically clamped below Nyquist by each voice.
    ///
    /// # Errors
    ///
    /// Returns a [`ProcessError`] if note settings are invalid, event offsets
    /// are unordered, or an offset exceeds `output.len()`. Validation happens
    /// before output or engine state changes.
    pub fn process(
        &mut self,
        output: &mut [f32],
        events: &[TimedEvent],
    ) -> Result<(), ProcessError> {
        validate_events(events, output.len())?;
        let mut cursor = 0;
        for timed in events {
            self.render(&mut output[cursor..timed.sample_offset]);
            self.dispatch_validated(timed.event);
            cursor = timed.sample_offset;
        }
        self.render(&mut output[cursor..]);
        Ok(())
    }

    pub(crate) fn render(&mut self, output: &mut [f32]) {
        for sample in output {
            *sample = self.next_sample();
        }
    }

    pub(crate) fn next_sample(&mut self) -> f32 {
        self.voices
            .iter_mut()
            .map(Voice::next_sample)
            .sum::<f32>()
            .clamp(-1.0, 1.0)
    }

    pub(crate) fn dispatch_validated(&mut self, event: Event) {
        match event {
            Event::NoteOn {
                id,
                instrument,
                frequency_hz,
                gain,
            } => {
                self.stop_id(id);
                let index = self.voice_for_start();
                self.age = self.age.wrapping_add(1);
                self.voices[index].start(
                    id,
                    instrument,
                    frequency_hz,
                    gain,
                    self.sample_rate,
                    self.age,
                );
            }
            Event::NoteOff(id) => self.release_id(id),
            Event::PitchBend { id, semitones } => {
                for voice in &mut self.voices {
                    if voice.active && voice.id == id {
                        voice.set_pitch_bend(semitones);
                    }
                }
            }
            Event::Modulation { id, amount } => {
                for voice in &mut self.voices {
                    if voice.active && voice.id == id {
                        voice.set_modulation(amount);
                    }
                }
            }
            Event::AllNotesOff => {
                for voice in &mut self.voices {
                    voice.release();
                }
            }
        }
    }

    pub(crate) fn release_id(&mut self, id: VoiceId) {
        for voice in &mut self.voices {
            if voice.active && voice.id == id {
                voice.release();
            }
        }
    }

    pub(crate) fn stop_id(&mut self, id: VoiceId) {
        for voice in &mut self.voices {
            if voice.active && voice.id == id {
                voice.stop();
            }
        }
    }

    pub(crate) fn assign_control_group(
        &mut self,
        id: VoiceId,
        group: u8,
        pitch_bend_semitones: f32,
        modulation: f32,
    ) {
        for voice in &mut self.voices {
            if voice.active && voice.id == id {
                voice.set_control_group(group);
                voice.set_pitch_bend(pitch_bend_semitones);
                voice.set_modulation(modulation);
            }
        }
    }

    pub(crate) fn set_group_pitch_bend(&mut self, group: u8, semitones: f32) {
        for voice in &mut self.voices {
            if voice.active && voice.control_group() == Some(group) {
                voice.set_pitch_bend(semitones);
            }
        }
    }

    pub(crate) fn set_group_modulation(&mut self, group: u8, amount: f32) {
        for voice in &mut self.voices {
            if voice.active && voice.control_group() == Some(group) {
                voice.set_modulation(amount);
            }
        }
    }

    fn voice_for_start(&self) -> usize {
        if let Some(index) = self.voices.iter().position(|voice| !voice.active) {
            return index;
        }
        self.voices
            .iter()
            .enumerate()
            .filter(|(_, voice)| voice.released)
            .min_by_key(|(_, voice)| voice.started_at)
            .or_else(|| {
                self.voices
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, voice)| voice.started_at)
            })
            .map_or(0, |(index, _)| index)
    }
}

pub(crate) fn validate_event(event: Event) -> Result<(), ProcessError> {
    match event {
        Event::NoteOn {
            frequency_hz, gain, ..
        } => {
            if !frequency_hz.is_finite() || frequency_hz <= 0.0 {
                return Err(ProcessError::InvalidFrequency);
            }
            if !gain.is_finite() || !(0.0..=1.0).contains(&gain) {
                return Err(ProcessError::InvalidGain);
            }
        }
        Event::PitchBend { semitones, .. } => {
            if !semitones.is_finite() || !(-128.0..=128.0).contains(&semitones) {
                return Err(ProcessError::InvalidPitchBend);
            }
        }
        Event::Modulation { amount, .. } => {
            if !amount.is_finite() || !(0.0..=1.0).contains(&amount) {
                return Err(ProcessError::InvalidModulation);
            }
        }
        Event::NoteOff(_) | Event::AllNotesOff => {}
    }
    Ok(())
}

pub(crate) fn validate_events(
    events: &[TimedEvent],
    output_len: usize,
) -> Result<(), ProcessError> {
    let mut previous = 0;
    for event in events {
        if event.sample_offset > output_len {
            return Err(ProcessError::EventOutsideBlock);
        }
        if event.sample_offset < previous {
            return Err(ProcessError::EventsNotOrdered);
        }
        validate_event(event.event)?;
        previous = event.sample_offset;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::Synth;
    use crate::{Event, Instrument, VoiceId};

    fn note(id: u64) -> Event {
        Event::NoteOn {
            id: VoiceId(id),
            instrument: Instrument::Sine,
            frequency_hz: 440.0,
            gain: 0.5,
        }
    }

    #[test]
    fn invalid_note_settings_do_not_change_state() {
        let mut synth = Synth::<2>::new(48_000.0).unwrap();
        let mut invalid = note(1);
        if let Event::NoteOn {
            ref mut frequency_hz,
            ..
        } = invalid
        {
            *frequency_hz = f32::NAN;
        }
        assert_eq!(
            synth.dispatch(invalid),
            Err(crate::ProcessError::InvalidFrequency)
        );
        assert_eq!(synth.active_voice_count(), 0);
    }

    #[test]
    fn stealing_prefers_released_then_oldest() {
        let mut synth = Synth::<2>::new(48_000.0).unwrap();
        synth.dispatch(note(1)).unwrap();
        synth.dispatch(note(2)).unwrap();
        synth.dispatch(Event::NoteOff(VoiceId(2))).unwrap();
        synth.dispatch(note(3)).unwrap();
        assert!(synth.voices.iter().any(|voice| voice.id == VoiceId(1)));
        assert!(synth.voices.iter().any(|voice| voice.id == VoiceId(3)));

        synth.dispatch(note(4)).unwrap();
        assert!(synth.voices.iter().any(|voice| voice.id == VoiceId(3)));
        assert!(synth.voices.iter().any(|voice| voice.id == VoiceId(4)));
    }
}
