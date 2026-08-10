#![allow(clippy::float_cmp)] // Exact equality proves untouched buffers and block invariance.

use tinyviolin::{Event, Instrument, ProcessError, Synth, TimedEvent, VoiceId};

fn note_on(id: u64, instrument: Instrument, frequency_hz: f32) -> Event {
    Event::NoteOn {
        id: VoiceId(id),
        instrument,
        frequency_hz,
        gain: 0.5,
    }
}

#[test]
fn events_are_applied_at_exact_block_offsets() {
    let mut synth = Synth::<4>::new(48_000.0).unwrap();
    let events = [
        TimedEvent::new(32, note_on(1, Instrument::Square, 440.0)),
        TimedEvent::new(96, Event::NoteOff(VoiceId(1))),
    ];
    let mut output = [9.0; 160];
    synth.process(&mut output, &events).unwrap();

    assert!(output[..32].iter().all(|sample| *sample == 0.0));
    assert!(output[33..96].iter().any(|sample| sample.abs() > 0.01));
    assert!(output.iter().all(|sample| (-1.0..=1.0).contains(sample)));
}

#[test]
fn invalid_timing_changes_neither_output_nor_engine() {
    let mut synth = Synth::<4>::new(48_000.0).unwrap();
    let events = [
        TimedEvent::new(12, note_on(1, Instrument::Sine, 440.0)),
        TimedEvent::new(4, Event::NoteOff(VoiceId(1))),
    ];
    let mut output = [0.25; 16];
    assert_eq!(
        synth.process(&mut output, &events),
        Err(ProcessError::EventsNotOrdered)
    );
    assert!(output.iter().all(|sample| *sample == 0.25));
    assert_eq!(synth.active_voice_count(), 0);
}

#[test]
fn processing_is_consistent_across_different_buffer_lengths() {
    let mut whole = Synth::<4>::new(48_000.0).unwrap();
    let mut chunked = Synth::<4>::new(48_000.0).unwrap();
    let event = [TimedEvent::new(0, note_on(5, Instrument::Triangle, 330.0))];
    let mut whole_output = [0.0; 512];
    whole.process(&mut whole_output, &event).unwrap();

    let mut chunked_output = [0.0; 512];
    chunked.process(&mut chunked_output[..73], &event).unwrap();
    chunked.process(&mut chunked_output[73..211], &[]).unwrap();
    chunked.process(&mut chunked_output[211..], &[]).unwrap();
    assert_eq!(whole_output, chunked_output);
}

#[test]
fn simultaneous_notes_and_all_notes_off_have_release_tails() {
    let mut synth = Synth::<4>::new(48_000.0).unwrap();
    let events = [
        TimedEvent::new(0, note_on(10, Instrument::Sine, 220.0)),
        TimedEvent::new(0, note_on(11, Instrument::Triangle, 330.0)),
    ];
    let mut attack = [0.0; 64];
    synth.process(&mut attack, &events).unwrap();
    assert_eq!(synth.active_voice_count(), 2);
    synth.dispatch(Event::AllNotesOff).unwrap();
    assert_eq!(synth.active_voice_count(), 2);
    let mut release = [0.0; 2_000];
    synth.process(&mut release, &[]).unwrap();
    assert!(release.iter().any(|sample| sample.abs() > 0.0));
    assert_eq!(synth.active_voice_count(), 0);
}

#[test]
fn event_beyond_block_is_rejected_before_mutation() {
    let mut synth = Synth::<1>::new(48_000.0).unwrap();
    let mut output = [0.75; 4];
    assert_eq!(
        synth.process(
            &mut output,
            &[TimedEvent::new(5, note_on(1, Instrument::Sine, 440.0))]
        ),
        Err(ProcessError::EventOutsideBlock)
    );
    assert!(output.iter().all(|sample| *sample == 0.75));
    assert_eq!(synth.active_voice_count(), 0);
}

#[test]
fn repeated_identity_retriggers_without_growing_polyphony() {
    let mut synth = Synth::<2>::new(48_000.0).unwrap();
    synth.dispatch(note_on(8, Instrument::Lead, 220.0)).unwrap();
    synth.dispatch(note_on(8, Instrument::Lead, 330.0)).unwrap();
    assert_eq!(synth.active_voice_count(), 1);
}

#[test]
fn full_pool_remains_bounded_and_percussion_expires() {
    let mut synth = Synth::<2>::new(48_000.0).unwrap();
    synth
        .dispatch(note_on(1, Instrument::BassDrum, 60.0))
        .unwrap();
    synth
        .dispatch(note_on(2, Instrument::Snare, 180.0))
        .unwrap();
    synth
        .dispatch(note_on(3, Instrument::HiHat, 6_000.0))
        .unwrap();
    assert_eq!(synth.active_voice_count(), 2);

    let mut output = vec![0.0; 32_000];
    synth.process(&mut output, &[]).unwrap();
    assert_eq!(synth.active_voice_count(), 0);
    assert!(output.iter().all(|sample| sample.is_finite()));
}

#[test]
fn block_end_event_affects_the_following_block() {
    let mut synth = Synth::<1>::new(48_000.0).unwrap();
    let mut empty = [];
    synth
        .process(
            &mut empty,
            &[TimedEvent::new(0, note_on(9, Instrument::Square, 220.0))],
        )
        .unwrap();
    assert_eq!(synth.active_voice_count(), 1);

    let mut output = [0.0; 16];
    synth.process(&mut output, &[]).unwrap();
    assert!(output[1..].iter().any(|sample| sample.abs() > 0.0));
}
