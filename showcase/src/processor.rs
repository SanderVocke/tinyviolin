use crate::preset::Preset;
use tinyviolin::{Event, ProcessError, Synth, VoiceId};

pub const VOICE_CAPACITY: usize = 32;
const HOST_VOICE_NAMESPACE: u64 = 1_u64 << 63;
const GUI_VOICE_NAMESPACE: u64 = 1_u64 << 62;
const NOTE_GAIN: f32 = 0.7;

/// Callback-owned synthesis state, public only to support integration tests.
#[doc(hidden)]
pub struct ShowcaseProcessor {
    synth: Synth<VOICE_CAPACITY>,
}

impl ShowcaseProcessor {
    /// Construct callback-owned state for a host sample rate.
    ///
    /// # Errors
    ///
    /// Returns the core synthesizer's configuration error for an invalid rate.
    pub fn new(sample_rate: f32) -> Result<Self, ProcessError> {
        Ok(Self {
            synth: Synth::new(sample_rate)?,
        })
    }

    /// Render a section with no events inside it.
    ///
    /// # Errors
    ///
    /// Propagates a core processing error.
    pub fn render(&mut self, output: &mut [f32]) -> Result<(), ProcessError> {
        self.synth.process(output, &[])
    }

    /// Start a host note immediately.
    ///
    /// # Errors
    ///
    /// Propagates validation errors from the core synthesizer.
    pub fn host_note_on(
        &mut self,
        preset: Preset,
        channel: u8,
        note: u8,
        velocity: f32,
    ) -> Result<(), ProcessError> {
        self.synth.dispatch(note_on_event(
            host_voice_id(channel, note),
            preset,
            note,
            velocity,
        ))
    }

    /// Start a GUI note immediately.
    ///
    /// # Errors
    ///
    /// Propagates validation errors from the core synthesizer.
    pub fn gui_note_on(
        &mut self,
        preset: Preset,
        note: u8,
        velocity: f32,
    ) -> Result<(), ProcessError> {
        self.synth
            .dispatch(note_on_event(gui_voice_id(note), preset, note, velocity))
    }

    /// Release a GUI note immediately.
    ///
    /// # Errors
    ///
    /// Propagates validation errors from the core synthesizer.
    pub fn gui_note_off(&mut self, note: u8) -> Result<(), ProcessError> {
        self.synth.dispatch(Event::NoteOff(gui_voice_id(note)))
    }

    /// Release a host note immediately.
    ///
    /// # Errors
    ///
    /// Propagates validation errors from the core synthesizer.
    pub fn host_note_off(&mut self, channel: u8, note: u8) -> Result<(), ProcessError> {
        self.synth
            .dispatch(Event::NoteOff(host_voice_id(channel, note)))
    }

    #[must_use]
    pub const fn sample_rate(&self) -> f32 {
        self.synth.sample_rate()
    }

    #[must_use]
    pub fn active_voice_count(&self) -> usize {
        self.synth.active_voice_count()
    }
}

#[must_use]
pub const fn host_voice_id(channel: u8, note: u8) -> VoiceId {
    VoiceId(HOST_VOICE_NAMESPACE | (channel as u64) << 8 | note as u64)
}

#[must_use]
pub const fn gui_voice_id(note: u8) -> VoiceId {
    VoiceId(GUI_VOICE_NAMESPACE | note as u64)
}

fn note_on_event(id: VoiceId, preset: Preset, note: u8, velocity: f32) -> Event {
    let velocity = if velocity.is_finite() {
        velocity.clamp(0.0, 1.0)
    } else {
        0.0
    };
    Event::NoteOn {
        id,
        instrument: preset.instrument(),
        frequency_hz: preset.frequency_hz(note),
        gain: NOTE_GAIN * velocity,
    }
}

pub(crate) fn apply_gain_and_duplicate(
    channels: &mut [&mut [f32]],
    mut next_gain: impl FnMut() -> f32,
) {
    let Some((mono, additional)) = channels.split_first_mut() else {
        return;
    };
    for sample in mono.iter_mut() {
        *sample *= next_gain();
    }
    for channel in additional {
        channel.copy_from_slice(mono);
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::float_cmp)] // Exact equality proves equivalent processing paths.
    use super::{ShowcaseProcessor, apply_gain_and_duplicate, gui_voice_id, host_voice_id};
    use crate::preset::Preset;
    use tinyviolin::{Event, Synth, TimedEvent};

    #[test]
    fn voice_namespaces_are_separate_and_host_ids_include_channel() {
        assert_ne!(host_voice_id(0, 60), gui_voice_id(60));
        assert_ne!(host_voice_id(0, 60), host_voice_id(1, 60));
        assert_ne!(host_voice_id(0, 60), host_voice_id(0, 61));
    }

    #[test]
    fn host_velocity_is_bounded_and_scales_output() {
        let mut silent = ShowcaseProcessor::new(48_000.0).unwrap();
        silent.host_note_on(Preset::Sine, 0, 69, f32::NAN).unwrap();
        let mut silent_output = [1.0; 32];
        silent.render(&mut silent_output).unwrap();
        assert_eq!(silent_output, [0.0; 32]);

        let mut full = ShowcaseProcessor::new(48_000.0).unwrap();
        let mut half = ShowcaseProcessor::new(48_000.0).unwrap();
        full.host_note_on(Preset::Sine, 0, 69, 5.0).unwrap();
        half.host_note_on(Preset::Sine, 0, 69, 0.5).unwrap();
        let mut full_output = [0.0; 32];
        let mut half_output = [0.0; 32];
        full.render(&mut full_output).unwrap();
        half.render(&mut half_output).unwrap();
        for (full_sample, half_sample) in full_output.into_iter().zip(half_output) {
            assert!((half_sample - full_sample * 0.5).abs() < f32::EPSILON);
        }
    }

    #[test]
    fn every_preset_produces_finite_nonzero_output() {
        for (channel, preset) in (0_u8..).zip(Preset::ALL) {
            let mut processor = ShowcaseProcessor::new(48_000.0).unwrap();
            processor.host_note_on(preset, channel, 60, 1.0).unwrap();
            let mut output = vec![0.0; 4_096];
            processor.render(&mut output).unwrap();
            assert!(output.iter().all(|sample| sample.is_finite()));
            assert!(output.iter().any(|sample| sample.abs() > 0.000_1));
        }
    }

    #[test]
    fn segmented_rendering_matches_timed_core_rendering() {
        let mut segmented = ShowcaseProcessor::new(48_000.0).unwrap();
        let mut segmented_output = [0.0; 128];
        segmented.render(&mut segmented_output[..16]).unwrap();
        segmented.host_note_on(Preset::Square, 2, 69, 1.0).unwrap();
        segmented.render(&mut segmented_output[16..80]).unwrap();
        segmented.host_note_off(2, 69).unwrap();
        segmented.render(&mut segmented_output[80..]).unwrap();

        let mut direct = Synth::<32>::new(48_000.0).unwrap();
        let mut direct_output = [0.0; 128];
        direct
            .process(
                &mut direct_output,
                &[
                    TimedEvent::new(
                        16,
                        Event::NoteOn {
                            id: host_voice_id(2, 69),
                            instrument: Preset::Square.instrument(),
                            frequency_hz: 440.0,
                            gain: 0.7,
                        },
                    ),
                    TimedEvent::new(80, Event::NoteOff(host_voice_id(2, 69))),
                ],
            )
            .unwrap();

        assert_eq!(segmented_output, direct_output);
    }

    #[test]
    fn gain_is_applied_once_and_mono_is_duplicated() {
        let mut left = [1.0, -0.5, 0.25];
        let mut right = [9.0; 3];
        let mut channels: [&mut [f32]; 2] = [&mut left, &mut right];
        apply_gain_and_duplicate(&mut channels, || 0.5);
        assert_eq!(left, [0.5, -0.25, 0.125]);
        assert_eq!(right, left);
    }
}
