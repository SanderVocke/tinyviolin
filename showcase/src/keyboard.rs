use crate::Preset;
use crate::processor::ShowcaseProcessor;
use rtrb::{Consumer, Producer, RingBuffer};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tinyviolin::ProcessError;

pub const FIRST_NOTE: u8 = 48;
pub const NOTE_COUNT: u8 = 24;
pub const WHITE_KEY_COUNT: u8 = 14;
pub const KEYBOARD_HEIGHT: f32 = 150.0;
const BLACK_HEIGHT_RATIO: f32 = 0.62;
const BLACK_WIDTH_RATIO: f32 = 0.62;
const GUI_VELOCITY: f32 = 0.8;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeyboardCommand {
    NoteOn(u8),
    NoteOff(u8),
}

pub struct EditorKeyboard {
    producer: Producer<KeyboardCommand>,
    emergency_release: Arc<AtomicBool>,
    active_note: Option<u8>,
}

pub struct AudioKeyboard {
    consumer: Consumer<KeyboardCommand>,
    emergency_release: Arc<AtomicBool>,
    active_notes: [bool; 128],
    editor_was_open: bool,
}

#[must_use]
pub fn keyboard_channel(capacity: usize) -> (AudioKeyboard, EditorKeyboard) {
    let (producer, consumer) = RingBuffer::new(capacity);
    let emergency_release = Arc::new(AtomicBool::new(false));
    (
        AudioKeyboard {
            consumer,
            emergency_release: emergency_release.clone(),
            active_notes: [false; 128],
            editor_was_open: false,
        },
        EditorKeyboard {
            producer,
            emergency_release,
            active_note: None,
        },
    )
}

impl EditorKeyboard {
    pub fn set_active_note(&mut self, note: Option<u8>) {
        let note = note.filter(|note| *note < 128);
        if note == self.active_note {
            return;
        }
        if let Some(previous) = self.active_note
            && self
                .producer
                .push(KeyboardCommand::NoteOff(previous))
                .is_err()
        {
            self.emergency_release.store(true, Ordering::Release);
        }
        if let Some(current) = note
            && self
                .producer
                .push(KeyboardCommand::NoteOn(current))
                .is_err()
        {
            self.emergency_release.store(true, Ordering::Release);
        }
        self.active_note = note;
    }
}

impl AudioKeyboard {
    pub fn reset(&mut self) {
        while self.consumer.pop().is_ok() {}
        self.active_notes.fill(false);
        self.editor_was_open = false;
        self.emergency_release.store(false, Ordering::Release);
    }

    /// Apply queued commands and editor lifecycle recovery before rendering a block.
    ///
    /// # Errors
    ///
    /// Propagates validation errors from the synthesizer.
    pub fn synchronize(
        &mut self,
        processor: &mut ShowcaseProcessor,
        preset: Preset,
        editor_is_open: bool,
    ) -> Result<(), ProcessError> {
        while let Ok(command) = self.consumer.pop() {
            match command {
                KeyboardCommand::NoteOn(note) => {
                    processor.gui_note_on(preset, note, GUI_VELOCITY)?;
                    self.active_notes[usize::from(note)] = true;
                }
                KeyboardCommand::NoteOff(note) => {
                    processor.gui_note_off(note)?;
                    self.active_notes[usize::from(note)] = false;
                }
            }
        }

        let editor_closed = self.editor_was_open && !editor_is_open;
        let emergency = self.emergency_release.swap(false, Ordering::AcqRel);
        if editor_closed || emergency {
            self.release_all(processor)?;
        }
        self.editor_was_open = editor_is_open;
        Ok(())
    }

    fn release_all(&mut self, processor: &mut ShowcaseProcessor) -> Result<(), ProcessError> {
        for (note, active) in self.active_notes.iter_mut().enumerate() {
            if *active {
                processor.gui_note_off(u8::try_from(note).unwrap_or(127))?;
                *active = false;
            }
        }
        Ok(())
    }
}

#[must_use]
pub const fn is_black(note: u8) -> bool {
    matches!(note % 12, 1 | 3 | 6 | 8 | 10)
}

#[must_use]
pub fn hit_test(x: f32, y: f32, width: f32, height: f32) -> Option<u8> {
    if !x.is_finite()
        || !y.is_finite()
        || !width.is_finite()
        || !height.is_finite()
        || x < 0.0
        || y < 0.0
        || x >= width
        || y >= height
        || width <= 0.0
        || height <= 0.0
    {
        return None;
    }

    let white_width = width / f32::from(WHITE_KEY_COUNT);
    let black_width = white_width * BLACK_WIDTH_RATIO;
    if y < height * BLACK_HEIGHT_RATIO {
        for note in FIRST_NOTE..FIRST_NOTE + NOTE_COUNT {
            if is_black(note) {
                let boundary = f32::from(white_keys_before(note)) * white_width;
                if x >= boundary - black_width * 0.5 && x < boundary + black_width * 0.5 {
                    return Some(note);
                }
            }
        }
    }

    let mut right_edge = white_width;
    for note in FIRST_NOTE..FIRST_NOTE + NOTE_COUNT {
        if !is_black(note) {
            if x < right_edge {
                return Some(note);
            }
            right_edge += white_width;
        }
    }
    None
}

#[must_use]
pub const fn white_keys_before(note: u8) -> u8 {
    let mut candidate = FIRST_NOTE;
    let mut count = 0;
    while candidate < note {
        if !is_black(candidate) {
            count += 1;
        }
        candidate += 1;
    }
    count
}

#[cfg(test)]
mod tests {
    use super::{
        FIRST_NOTE, KEYBOARD_HEIGHT, NOTE_COUNT, WHITE_KEY_COUNT, hit_test, is_black,
        keyboard_channel, white_keys_before,
    };
    use crate::Preset;
    use crate::processor::ShowcaseProcessor;

    #[test]
    fn keyboard_hit_testing_assigns_white_and_black_notes() {
        let width = 700.0;
        assert_eq!(hit_test(25.0, 140.0, width, KEYBOARD_HEIGHT), Some(48));
        assert_eq!(hit_test(50.0, 20.0, width, KEYBOARD_HEIGHT), Some(49));
        assert_eq!(hit_test(75.0, 140.0, width, KEYBOARD_HEIGHT), Some(50));
        assert_eq!(
            hit_test(699.0, 140.0, width, KEYBOARD_HEIGHT),
            Some(FIRST_NOTE + NOTE_COUNT - 1)
        );
        assert_eq!(hit_test(-1.0, 0.0, width, KEYBOARD_HEIGHT), None);

        let white_width = width / f32::from(WHITE_KEY_COUNT);
        for note in FIRST_NOTE..FIRST_NOTE + NOTE_COUNT {
            let x = if is_black(note) {
                f32::from(white_keys_before(note)) * white_width
            } else {
                (f32::from(white_keys_before(note)) + 0.5) * white_width
            };
            let y = if is_black(note) { 20.0 } else { 140.0 };
            assert_eq!(hit_test(x, y, width, KEYBOARD_HEIGHT), Some(note));
        }
    }

    #[test]
    fn command_order_and_editor_close_release_gui_notes_only() {
        let (mut audio, mut editor) = keyboard_channel(8);
        let mut processor = ShowcaseProcessor::new(48_000.0).unwrap();
        processor.host_note_on(Preset::Sine, 0, 60, 1.0).unwrap();

        editor.set_active_note(Some(60));
        audio
            .synchronize(&mut processor, Preset::Pad, true)
            .unwrap();
        assert_eq!(processor.active_voice_count(), 2);

        editor.set_active_note(Some(62));
        audio
            .synchronize(&mut processor, Preset::Pad, true)
            .unwrap();
        assert_eq!(processor.active_voice_count(), 3);

        audio
            .synchronize(&mut processor, Preset::Pad, false)
            .unwrap();
        assert_eq!(processor.active_voice_count(), 3);

        let mut release = vec![0.0; 48_000];
        processor.render(&mut release).unwrap();
        assert_eq!(processor.active_voice_count(), 1);
    }

    #[test]
    fn queue_overflow_requests_emergency_release() {
        let (mut audio, mut editor) = keyboard_channel(1);
        let mut processor = ShowcaseProcessor::new(48_000.0).unwrap();
        editor.set_active_note(Some(60));
        editor.set_active_note(Some(62));
        audio
            .synchronize(&mut processor, Preset::Lead, true)
            .unwrap();

        let mut release = vec![0.0; 8_000];
        processor.render(&mut release).unwrap();
        assert_eq!(processor.active_voice_count(), 0);
    }
}
