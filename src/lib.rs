#![forbid(unsafe_code)]
//! Small, generated instruments for real-time audio applications.
//!
//! [`Synth`] fills caller-owned mono `f32` buffers. Its voice storage is fixed
//! at compile time and its event and audio methods neither allocate nor lock.
//! Events passed to [`Synth::process`] are ordered by [`TimedEvent::sample_offset`].
//! The sample rate is fixed for an engine's lifetime; create engines and MIDI
//! mappings outside the audio callback.

mod engine;
mod event;
mod instrument;
pub mod midi;

pub use engine::Synth;
pub use event::{Event, ProcessError, TimedEvent, VoiceId};
pub use instrument::Instrument;

#[cfg(test)]
mod tests {
    use super::{Event, TimedEvent};
    use crate::midi::MidiMessage;

    fn assert_copy<T: Copy>() {}

    #[test]
    fn callback_inputs_are_copy_types() {
        assert_copy::<Event>();
        assert_copy::<TimedEvent>();
        assert_copy::<MidiMessage>();
        assert!(size_of::<MidiMessage>() <= 5);
    }
}
