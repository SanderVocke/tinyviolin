#![allow(unsafe_code)] // Instrumentation wraps the system allocator; the library remains safe Rust.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use tinyviolin::midi::{MidiLayer, MidiMessage, MidiPitch, MidiSynth, TimedMidiMessage};
use tinyviolin::{Event, Instrument, Synth, TimedEvent, VoiceId};

struct CountingAllocator;

static TRACKING: AtomicBool = AtomicBool::new(false);
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if TRACKING.load(Ordering::Relaxed) {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        }
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) };
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

#[test]
fn prepared_core_and_midi_processing_do_not_allocate() {
    // Keep both checks in one test so unrelated test-harness threads cannot be
    // created while the process-wide allocator counter is enabled.
    let mut core = Synth::<8>::new(48_000.0).unwrap();
    let core_events = [
        TimedEvent::new(
            0,
            Event::NoteOn {
                id: VoiceId(1),
                instrument: Instrument::Pad,
                frequency_hz: 220.0,
                gain: 0.4,
            },
        ),
        TimedEvent::new(96, Event::NoteOff(VoiceId(1))),
    ];
    let mut core_output = [0.0; 256];

    let mut midi = MidiSynth::<8, 2>::new(48_000.0).unwrap();
    midi.set_layer(
        0,
        60,
        0,
        MidiLayer {
            instrument: Instrument::Bass,
            pitch: MidiPitch::Note,
            gain: 0.5,
        },
    )
    .unwrap();
    let midi_events = [
        TimedMidiMessage::new(0, MidiMessage::new(&[0x90, 60, 100]).unwrap()),
        TimedMidiMessage::new(96, MidiMessage::new(&[0x80, 60, 0]).unwrap()),
    ];
    let mut midi_output = [0.0; 256];

    ALLOCATIONS.store(0, Ordering::SeqCst);
    TRACKING.store(true, Ordering::SeqCst);
    let core_result = core.process(&mut core_output, &core_events);
    let midi_result = midi.process(&mut midi_output, &midi_events);
    TRACKING.store(false, Ordering::SeqCst);

    assert!(core_result.is_ok());
    assert!(midi_result.is_ok());
    assert_eq!(ALLOCATIONS.load(Ordering::SeqCst), 0);
}
