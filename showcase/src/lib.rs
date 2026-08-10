#![forbid(unsafe_code)]

mod editor;
pub mod keyboard;
mod preset;
pub mod processor;

use keyboard::{AudioKeyboard, EditorKeyboard, keyboard_channel};
use nice_plug::{editor::dpi::LogicalSize, prelude::*};
use nice_plug_egui::EguiState;
#[doc(hidden)]
pub use preset::Preset;
use processor::{ShowcaseProcessor, apply_gain_and_duplicate};
use std::sync::Arc;

const DEFAULT_SAMPLE_RATE: f32 = 48_000.0;
const EDITOR_WIDTH: f32 = 640.0;
const EDITOR_HEIGHT: f32 = 300.0;

pub struct TinyViolinShowcase {
    params: Arc<ShowcaseParams>,
    processor: ShowcaseProcessor,
    audio_keyboard: AudioKeyboard,
    initial_editor_keyboard: Option<EditorKeyboard>,
}

#[derive(Params)]
struct ShowcaseParams {
    #[persist = "editor-state"]
    editor_state: Arc<EguiState>,

    #[id = "preset"]
    preset: EnumParam<Preset>,

    #[id = "master-gain"]
    master_gain: FloatParam,
}

impl Default for ShowcaseParams {
    fn default() -> Self {
        Self {
            editor_state: EguiState::from_size(LogicalSize::new(EDITOR_WIDTH, EDITOR_HEIGHT)),
            preset: EnumParam::new("Preset", Preset::Sine),
            master_gain: FloatParam::new(
                "Master Gain",
                util::db_to_gain(-6.0),
                FloatRange::Skewed {
                    min: util::db_to_gain(-60.0),
                    max: util::db_to_gain(0.0),
                    factor: FloatRange::gain_skew_factor(-60.0, 0.0),
                },
            )
            .with_smoother(SmoothingStyle::Logarithmic(20.0))
            .with_unit(" dB")
            .with_value_to_string(formatters::v2s_f32_gain_to_db(1))
            .with_string_to_value(formatters::s2v_f32_gain_to_db()),
        }
    }
}

impl Default for TinyViolinShowcase {
    fn default() -> Self {
        let (audio_keyboard, editor_keyboard) = keyboard_channel(256);
        Self {
            params: Arc::new(ShowcaseParams::default()),
            processor: ShowcaseProcessor::new(DEFAULT_SAMPLE_RATE)
                .expect("the fixed default sample rate is valid"),
            audio_keyboard,
            initial_editor_keyboard: Some(editor_keyboard),
        }
    }
}

impl Plugin for TinyViolinShowcase {
    const NAME: &'static str = "Tiny Violin";
    const VENDOR: &'static str = "Sander Vocke";
    const URL: &'static str = "https://github.com/SanderVocke/tinyviolin";
    const EMAIL: &'static str = "sander.vocke@asmpt.com";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[
        AudioIOLayout {
            main_input_channels: None,
            main_output_channels: NonZeroU32::new(2),
            ..AudioIOLayout::const_default()
        },
        AudioIOLayout {
            main_input_channels: None,
            main_output_channels: NonZeroU32::new(1),
            ..AudioIOLayout::const_default()
        },
    ];
    const MIDI_INPUT: MidiConfig = MidiConfig::Basic;
    const SAMPLE_ACCURATE_AUTOMATION: bool = true;

    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        editor::create_editor(self.params.clone(), self.initial_editor_keyboard.take()?)
    }

    fn initialize(
        &mut self,
        _audio_io_layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        _context: &mut impl InitContext<Self>,
    ) -> bool {
        match ShowcaseProcessor::new(buffer_config.sample_rate) {
            Ok(processor) => {
                self.processor = processor;
                true
            }
            Err(_) => false,
        }
    }

    fn reset(&mut self) {
        let sample_rate = self.processor.sample_rate();
        if let Ok(processor) = ShowcaseProcessor::new(sample_rate) {
            self.processor = processor;
        }
        self.audio_keyboard.reset();
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        if let Err(error) = self.audio_keyboard.synchronize(
            &mut self.processor,
            self.params.preset.value(),
            self.params.editor_state.is_open(),
        ) {
            return process_error(error);
        }

        let channels = buffer.as_slice();
        let Some((mono, _)) = channels.split_first_mut() else {
            return ProcessStatus::Error("tinyviolin requires an output channel");
        };

        let mut cursor = 0;
        while let Some(event) = context.next_event() {
            let result = match event {
                NoteEvent::NoteOn {
                    timing,
                    channel,
                    note,
                    velocity,
                    ..
                } => {
                    let timing = (timing as usize).min(mono.len()).max(cursor);
                    if let Err(error) = self.processor.render(&mut mono[cursor..timing]) {
                        return process_error(error);
                    }
                    cursor = timing;
                    self.processor
                        .host_note_on(self.params.preset.value(), channel, note, velocity)
                }
                NoteEvent::NoteOff {
                    timing,
                    channel,
                    note,
                    ..
                }
                | NoteEvent::Choke {
                    timing,
                    channel,
                    note,
                    ..
                } => {
                    let timing = (timing as usize).min(mono.len()).max(cursor);
                    if let Err(error) = self.processor.render(&mut mono[cursor..timing]) {
                        return process_error(error);
                    }
                    cursor = timing;
                    self.processor.host_note_off(channel, note)
                }
                _ => continue,
            };
            if let Err(error) = result {
                return process_error(error);
            }
        }

        if let Err(error) = self.processor.render(&mut mono[cursor..]) {
            return process_error(error);
        }
        apply_gain_and_duplicate(channels, || self.params.master_gain.smoothed.next());

        ProcessStatus::KeepAlive
    }
}

fn process_error(_error: tinyviolin::ProcessError) -> ProcessStatus {
    ProcessStatus::Error("tinyviolin rejected a processing event")
}

impl ClapPlugin for TinyViolinShowcase {
    const CLAP_ID: &'static str = "com.sandervocke.tinyviolin";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("A tiny synthesized instrument showcase");
    const CLAP_MANUAL_URL: Option<&'static str> = Some(Self::URL);
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::Instrument,
        ClapFeature::Synthesizer,
        ClapFeature::Stereo,
        ClapFeature::Mono,
    ];
}

#[cfg(feature = "vst3")]
impl Vst3Plugin for TinyViolinShowcase {
    const VST3_CLASS_ID: [u8; 16] = *b"TinyViolinSynth1";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] = &[
        Vst3SubCategory::Instrument,
        Vst3SubCategory::Synth,
        Vst3SubCategory::Stereo,
    ];
}

nice_export_clap!(TinyViolinShowcase);
#[cfg(feature = "vst3")]
nice_export_vst3!(TinyViolinShowcase);

#[cfg(test)]
mod tests {
    use super::ShowcaseParams;

    #[test]
    fn master_gain_moves_smoothly_to_a_new_target() {
        let params = ShowcaseParams::default();
        params.master_gain.smoothed.reset(1.0);
        params.master_gain.smoothed.set_target(48_000.0, 0.1);
        let first = params.master_gain.smoothed.next();
        let mut last = first;
        for _ in 0..960 {
            last = params.master_gain.smoothed.next();
        }
        assert!(first < 1.0 && first > 0.1);
        assert!(last < first);
        assert!((last - 0.1).abs() < 0.001);
    }
}
