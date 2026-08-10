use nice_assert_no_alloc::assert_no_alloc;
use tinyviolin_showcase::Preset;
use tinyviolin_showcase::keyboard::keyboard_channel;
use tinyviolin_showcase::processor::ShowcaseProcessor;

#[test]
fn prepared_showcase_processing_and_gui_drain_do_not_allocate() {
    let mut processor = ShowcaseProcessor::new(48_000.0).unwrap();
    let (mut audio_keyboard, mut editor_keyboard) = keyboard_channel(8);
    editor_keyboard.set_active_note(Some(60));
    let mut first = [0.0; 96];
    let mut second = [0.0; 160];

    let (gui_sync, first_render, note_on, note_off, second_render) = assert_no_alloc(|| {
        (
            audio_keyboard.synchronize(&mut processor, Preset::Pad, true),
            processor.render(&mut first),
            processor.host_note_on(Preset::Pad, 3, 60, 0.8),
            processor.host_note_off(3, 60),
            processor.render(&mut second),
        )
    });

    assert!(gui_sync.is_ok());
    assert!(first_render.is_ok());
    assert!(note_on.is_ok());
    assert!(note_off.is_ok());
    assert!(second_render.is_ok());
}
