#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

mod audio;
mod dsp;
mod effects;
mod engine;
mod event;
mod instrument;
pub mod midi;

pub use audio::{AudioMidiError, AudioProcessor};
pub use effects::EffectSettings;
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
