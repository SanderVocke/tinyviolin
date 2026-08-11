use crate::keyboard::{
    EditorKeyboard, KEYBOARD_HEIGHT, NOTE_COUNT, WHITE_KEY_COUNT, hit_test, is_black,
    white_keys_before,
};
use crate::{EDITOR_HEIGHT, EDITOR_WIDTH, Preset, ShowcaseParams};
use egui::{Color32, Rect, Sense, Stroke, StrokeKind, Vec2, pos2};
use nice_plug::editor::Editor;
use nice_plug::prelude::Enum;
use nice_plug_egui::resizable_window::ResizableWindow;
use nice_plug_egui::{EguiNiceSettings, create_egui_editor, widgets};
use std::sync::Arc;

pub(crate) fn create_editor(
    params: Arc<ShowcaseParams>,
    keyboard: EditorKeyboard,
) -> Option<Box<dyn Editor>> {
    create_egui_editor(
        params.editor_state.clone(),
        keyboard,
        EguiNiceSettings::default(),
        |_context, _queue, _keyboard| {},
        move |ui, setter, _queue, keyboard| {
            ResizableWindow::new("tinyviolin-window")
                .min_size(Vec2::new(EDITOR_WIDTH, EDITOR_HEIGHT))
                .show(ui, |ui| {
                    ui.heading("Tiny Violin");
                    ui.horizontal(|ui| {
                        ui.label("Preset");
                        let selected = params.preset.value();
                        egui::ComboBox::from_id_salt("preset-selector")
                            .selected_text(Preset::variants()[selected.to_index()])
                            .show_ui(ui, |ui| {
                                for preset in Preset::ALL {
                                    let label = Preset::variants()[preset.to_index()];
                                    if ui.selectable_label(selected == preset, label).clicked() {
                                        setter.begin_set_parameter(&params.preset);
                                        setter.set_parameter(&params.preset, preset);
                                        setter.end_set_parameter(&params.preset);
                                    }
                                }
                            });

                        ui.separator();
                        ui.label("Master Gain");
                        ui.add(
                            widgets::ParamSlider::for_param(&params.master_gain, setter)
                                .with_width(180.0),
                        );
                    });
                    ui.horizontal(|ui| {
                        let mut reverb_enabled = params.reverb_enabled.value();
                        if ui.checkbox(&mut reverb_enabled, "Reverb").changed() {
                            setter.begin_set_parameter(&params.reverb_enabled);
                            setter.set_parameter(&params.reverb_enabled, reverb_enabled);
                            setter.end_set_parameter(&params.reverb_enabled);
                        }
                        ui.label("Amount");
                        ui.add_enabled(
                            reverb_enabled,
                            widgets::ParamSlider::for_param(&params.reverb_amount, setter)
                                .with_width(130.0),
                        );

                        ui.separator();
                        let mut distortion_enabled = params.distortion_enabled.value();
                        if ui.checkbox(&mut distortion_enabled, "Distortion").changed() {
                            setter.begin_set_parameter(&params.distortion_enabled);
                            setter.set_parameter(&params.distortion_enabled, distortion_enabled);
                            setter.end_set_parameter(&params.distortion_enabled);
                        }
                        ui.label("Drive");
                        ui.add_enabled(
                            distortion_enabled,
                            widgets::ParamSlider::for_param(&params.distortion_drive, setter)
                                .with_width(130.0),
                        );
                    });
                    ui.horizontal(|ui| {
                        let mut compressor_enabled = params.compressor_enabled.value();
                        if ui.checkbox(&mut compressor_enabled, "Compressor").changed() {
                            setter.begin_set_parameter(&params.compressor_enabled);
                            setter.set_parameter(&params.compressor_enabled, compressor_enabled);
                            setter.end_set_parameter(&params.compressor_enabled);
                        }
                        ui.label("Amount");
                        ui.add_enabled(
                            compressor_enabled,
                            widgets::ParamSlider::for_param(&params.compressor_amount, setter)
                                .with_width(130.0),
                        );
                    });
                    ui.horizontal(|ui| {
                        let mut eq_enabled = params.eq_enabled.value();
                        if ui.checkbox(&mut eq_enabled, "3-Band EQ").changed() {
                            setter.begin_set_parameter(&params.eq_enabled);
                            setter.set_parameter(&params.eq_enabled, eq_enabled);
                            setter.end_set_parameter(&params.eq_enabled);
                        }
                        for (label, parameter) in [
                            ("Low", &params.eq_low_db),
                            ("Mid", &params.eq_mid_db),
                            ("High", &params.eq_high_db),
                        ] {
                            ui.label(label);
                            ui.add_enabled(
                                eq_enabled,
                                widgets::ParamSlider::for_param(parameter, setter).with_width(95.0),
                            );
                        }
                    });
                    ui.add_space(12.0);
                    draw_keyboard(ui, keyboard);
                });
        },
    )
}

fn draw_keyboard(ui: &mut egui::Ui, keyboard: &mut EditorKeyboard) {
    let desired = Vec2::new(ui.available_width().max(280.0), KEYBOARD_HEIGHT);
    let (rect, response) = ui.allocate_exact_size(desired, Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    let white_width = rect.width() / f32::from(WHITE_KEY_COUNT);

    for note in crate::keyboard::FIRST_NOTE..crate::keyboard::FIRST_NOTE + NOTE_COUNT {
        if !is_black(note) {
            let index = f32::from(white_keys_before(note));
            let key = Rect::from_min_max(
                pos2(rect.left() + index * white_width, rect.top()),
                pos2(rect.left() + (index + 1.0) * white_width, rect.bottom()),
            );
            painter.rect_filled(key.shrink(1.0), 2.0, Color32::from_gray(238));
            painter.rect_stroke(
                key.shrink(1.0),
                2.0,
                Stroke::new(1.0, Color32::from_gray(70)),
                StrokeKind::Inside,
            );
        }
    }

    let black_width = white_width * 0.62;
    for note in crate::keyboard::FIRST_NOTE..crate::keyboard::FIRST_NOTE + NOTE_COUNT {
        if is_black(note) {
            let boundary = f32::from(white_keys_before(note)) * white_width;
            let key = Rect::from_min_max(
                pos2(rect.left() + boundary - black_width * 0.5, rect.top()),
                pos2(
                    rect.left() + boundary + black_width * 0.5,
                    rect.top() + rect.height() * 0.62,
                ),
            );
            painter.rect_filled(key, 2.0, Color32::from_gray(28));
        }
    }

    let active_note = if response.is_pointer_button_down_on() {
        response.interact_pointer_pos().and_then(|position| {
            hit_test(
                position.x - rect.left(),
                position.y - rect.top(),
                rect.width(),
                rect.height(),
            )
        })
    } else {
        None
    };
    keyboard.set_active_note(active_note);
}
