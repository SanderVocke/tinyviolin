#![allow(clippy::float_cmp)] // Exact equality proves invalid streams leave buffers untouched.

use tinyviolin::Instrument;
use tinyviolin::midi::{
    MAX_MESSAGE_BYTES, MidiError, MidiLayer, MidiMessage, MidiPitch, MidiSynth, TimedMidiMessage,
};

fn message(bytes: &[u8]) -> MidiMessage {
    MidiMessage::new(bytes).unwrap()
}

fn layer(instrument: Instrument, pitch: MidiPitch, gain: f32) -> MidiLayer {
    MidiLayer {
        instrument,
        pitch,
        gain,
    }
}

#[test]
fn message_storage_is_capped_at_four_bytes() {
    assert_eq!(MAX_MESSAGE_BYTES, 4);
    assert_eq!(message(&[0x90, 60, 127]).as_bytes(), &[0x90, 60, 127]);
    assert_eq!(MidiMessage::new(&[]), Err(MidiError::InvalidLength));
    assert_eq!(
        MidiMessage::new(&[1, 2, 3, 4, 5]),
        Err(MidiError::InvalidLength)
    );
}

#[test]
fn mappings_trigger_layers_with_note_and_fixed_pitch() {
    let mut midi = MidiSynth::<8, 2>::new(48_000.0).unwrap();
    midi.set_layer(3, 60, 0, layer(Instrument::Bass, MidiPitch::Note, 0.7))
        .unwrap();
    midi.set_layer(
        3,
        60,
        1,
        layer(Instrument::HiHat, MidiPitch::Fixed(6_000.0), 0.25),
    )
    .unwrap();

    midi.dispatch(message(&[0x93, 60, 100])).unwrap();
    assert_eq!(midi.engine().active_voice_count(), 2);
    let mut output = [0.0; 128];
    midi.process(&mut output, &[]).unwrap();
    assert!(output.iter().any(|sample| sample.abs() > 0.001));

    // Note-off IDs are independent of current mappings, so setup-time remapping
    // cannot strand voices that were already active.
    midi.clear_layer(3, 60, 0).unwrap();
    midi.clear_layer(3, 60, 1).unwrap();
    midi.dispatch(message(&[0x83, 60, 0])).unwrap();
    let mut release = vec![0.0; 30_000];
    midi.process(&mut release, &[]).unwrap();
    assert_eq!(midi.engine().active_voice_count(), 0);
}

#[test]
fn channel_mapping_and_channel_specific_controls_work() {
    let mut midi = MidiSynth::<8, 1>::new(48_000.0).unwrap();
    midi.set_channel_layer(0, 0, layer(Instrument::Square, MidiPitch::Note, 0.5))
        .unwrap();
    midi.set_channel_layer(15, 0, layer(Instrument::Triangle, MidiPitch::Note, 0.5))
        .unwrap();
    midi.dispatch(message(&[0x90, 0, 127])).unwrap();
    midi.dispatch(message(&[0x9f, 127, 127])).unwrap();
    assert_eq!(midi.engine().active_voice_count(), 2);

    midi.dispatch(message(&[0xb0, 123, 0])).unwrap();
    midi.dispatch(message(&[0xbf, 120, 0])).unwrap();
    assert_eq!(midi.engine().active_voice_count(), 1);
    let mut release = [0.0; 2_000];
    midi.process(&mut release, &[]).unwrap();
    assert_eq!(midi.engine().active_voice_count(), 0);
}

#[test]
fn timed_midi_is_sample_accurate_and_prevalidated() {
    let mut midi = MidiSynth::<4, 1>::new(48_000.0).unwrap();
    midi.set_layer(0, 69, 0, layer(Instrument::Square, MidiPitch::Note, 1.0))
        .unwrap();
    let events = [
        TimedMidiMessage::new(16, message(&[0x90, 69, 127])),
        TimedMidiMessage::new(48, message(&[0x80, 69, 0])),
    ];
    let mut output = [3.0; 64];
    midi.process(&mut output, &events).unwrap();
    assert!(output[..16].iter().all(|sample| *sample == 0.0));
    assert!(output[17..48].iter().any(|sample| sample.abs() > 0.1));

    let bad = [
        TimedMidiMessage::new(3, message(&[0x90, 69, 1])),
        TimedMidiMessage::new(2, message(&[0x80, 69, 0])),
    ];
    let mut untouched = [0.25; 8];
    assert_eq!(
        midi.process(&mut untouched, &bad),
        Err(MidiError::InvalidTiming)
    );
    assert!(untouched.iter().all(|sample| *sample == 0.25));
}

#[test]
fn mapping_validation_and_pool_overflow_are_deterministic() {
    let mut midi = MidiSynth::<1, 1>::new(48_000.0).unwrap();
    let valid = layer(Instrument::Lead, MidiPitch::Note, 0.5);
    assert_eq!(
        midi.set_layer(16, 0, 0, valid),
        Err(MidiError::InvalidChannel)
    );
    assert_eq!(
        midi.set_layer(0, 128, 0, valid),
        Err(MidiError::InvalidNote)
    );
    assert_eq!(midi.set_layer(0, 0, 1, valid), Err(MidiError::InvalidLayer));
    assert_eq!(
        midi.set_layer(0, 0, 0, layer(Instrument::Lead, MidiPitch::Note, f32::NAN)),
        Err(MidiError::InvalidLayerSettings)
    );
    assert_eq!(
        midi.set_layer(0, 0, 0, layer(Instrument::Lead, MidiPitch::Fixed(0.0), 0.5)),
        Err(MidiError::InvalidLayerSettings)
    );

    midi.set_channel_layer(0, 0, valid).unwrap();
    midi.dispatch(message(&[0x90, 1, 127])).unwrap();
    midi.dispatch(message(&[0x90, 2, 127])).unwrap();
    assert_eq!(midi.engine().active_voice_count(), 1);
}

#[test]
fn malformed_and_unsupported_messages_do_not_change_state() {
    let mut midi = MidiSynth::<2, 1>::new(48_000.0).unwrap();
    assert_eq!(
        midi.dispatch(message(&[0x90, 60])),
        Err(MidiError::MalformedMessage)
    );
    assert_eq!(
        midi.dispatch(message(&[0xe0, 0, 0])),
        Err(MidiError::UnsupportedMessage)
    );
    assert_eq!(midi.engine().active_voice_count(), 0);
}
