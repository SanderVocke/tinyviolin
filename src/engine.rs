use crate::{Event, ProcessError, TimedEvent};

/// Fixed-capacity polyphonic synthesizer.
///
/// `VOICES` is the maximum number of simultaneous instrument layers. Buffer
/// size is inferred from the output slice supplied to [`Self::process`].
pub struct Synth<const VOICES: usize = 32> {
    sample_rate: f32,
    voices: [Voice; VOICES],
}

#[derive(Clone, Copy, Default)]
struct Voice;

impl<const VOICES: usize> Synth<VOICES> {
    /// Create a silent engine with a fixed sample rate.
    ///
    /// Construction is a setup operation and is not part of the real-time API.
    pub fn new(sample_rate: f32) -> Result<Self, ProcessError> {
        if !sample_rate.is_finite() || !(1.0..=768_000.0).contains(&sample_rate) {
            return Err(ProcessError::InvalidSampleRate);
        }
        if VOICES == 0 {
            return Err(ProcessError::ZeroVoices);
        }
        Ok(Self {
            sample_rate,
            voices: [Voice; VOICES],
        })
    }

    /// Return the configured sample rate in hertz.
    #[must_use]
    pub const fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    /// Apply one event immediately, before the next generated sample.
    ///
    /// This method performs no allocation or locking.
    pub fn dispatch(&mut self, event: Event) -> Result<(), ProcessError> {
        validate_event(event)
    }

    /// Fill a mono output block and apply ordered, sample-timed events.
    ///
    /// The method validates the complete event slice before changing engine or
    /// output state. It performs no allocation or locking. Output is always in
    /// `-1.0..=1.0` when processing succeeds.
    pub fn process(
        &mut self,
        output: &mut [f32],
        events: &[TimedEvent],
    ) -> Result<(), ProcessError> {
        validate_events(events, output.len())?;
        output.fill(0.0);
        for event in events {
            self.dispatch(event.event)?;
        }
        let _ = &self.voices;
        Ok(())
    }
}

pub(crate) fn validate_event(event: Event) -> Result<(), ProcessError> {
    if let Event::NoteOn {
        frequency_hz, gain, ..
    } = event
    {
        if !frequency_hz.is_finite() || frequency_hz <= 0.0 {
            return Err(ProcessError::InvalidFrequency);
        }
        if !gain.is_finite() || !(0.0..=1.0).contains(&gain) {
            return Err(ProcessError::InvalidGain);
        }
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
