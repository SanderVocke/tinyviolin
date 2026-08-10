use nice_assert_no_alloc::assert_no_alloc;
use tinyviolin::EffectSettings;
use tinyviolin_showcase::Preset;
use tinyviolin_showcase::keyboard::keyboard_channel;
use tinyviolin_showcase::processor::ShowcaseProcessor;

#[test]
fn prepared_showcase_processing_and_gui_drain_do_not_allocate() {
    let mut processor = ShowcaseProcessor::with_channels(48_000.0, 3).unwrap();
    processor
        .set_effect_settings(EffectSettings {
            reverb_enabled: true,
            distortion_enabled: true,
            ..EffectSettings::default()
        })
        .unwrap();
    let (mut audio_keyboard, mut editor_keyboard) = keyboard_channel(8);
    editor_keyboard.set_active_note(Some(60));
    let mut first_left = [0.0; 96];
    let mut first_right = [0.0; 96];
    let mut first_aux = [0.0; 96];
    let mut first_channels: [&mut [f32]; 3] = [&mut first_left, &mut first_right, &mut first_aux];
    let mut second_left = [0.0; 160];
    let mut second_right = [0.0; 160];
    let mut second_aux = [0.0; 160];
    let mut second_channels: [&mut [f32]; 3] =
        [&mut second_left, &mut second_right, &mut second_aux];

    let (gui_sync, first_render, note_on, note_off, second_render) = assert_no_alloc(|| {
        (
            audio_keyboard.synchronize(&mut processor, Preset::Pad, true),
            processor.render_channels(&mut first_channels, 0..96),
            processor.host_note_on(Preset::Pad, 3, 60, 0.8),
            processor.host_note_off(3, 60),
            processor.render_channels(&mut second_channels, 0..160),
        )
    });

    assert!(gui_sync.is_ok());
    assert!(first_render.is_ok());
    assert!(note_on.is_ok());
    assert!(note_off.is_ok());
    assert!(second_render.is_ok());
}
