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
use processor::{ShowcaseProcessor, apply_gain};
use std::sync::Arc;

const DEFAULT_SAMPLE_RATE: f32 = 48_000.0;
const DEFAULT_CHANNELS: usize = 2;
const MAX_PLUGIN_CHANNELS: usize = 63;
const EDITOR_WIDTH: f32 = 640.0;
const EDITOR_HEIGHT: f32 = 440.0;

const fn audio_layout(channel_count: u32) -> AudioIOLayout {
    AudioIOLayout {
        main_input_channels: NonZeroU32::new(channel_count),
        main_output_channels: NonZeroU32::new(channel_count),
        ..AudioIOLayout::const_default()
    }
}

const fn audio_layouts() -> [AudioIOLayout; MAX_PLUGIN_CHANNELS] {
    let mut layouts = [AudioIOLayout::const_default(); MAX_PLUGIN_CHANNELS];
    // Keep stereo as the host default, followed by mono and all remaining
    // channel counts representable by a VST3 speaker arrangement.
    layouts[0] = audio_layout(2);
    layouts[1] = audio_layout(1);
    let mut index = 2;
    let mut channel_count = 3;
    while index < MAX_PLUGIN_CHANNELS {
        layouts[index] = audio_layout(channel_count);
        index += 1;
        channel_count += 1;
    }
    layouts
}

const AUDIO_IO_LAYOUTS: [AudioIOLayout; MAX_PLUGIN_CHANNELS] = audio_layouts();

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

    #[id = "reverb-enabled"]
    reverb_enabled: BoolParam,

    #[id = "reverb-amount"]
    reverb_amount: FloatParam,

    #[id = "distortion-enabled"]
    distortion_enabled: BoolParam,

    #[id = "distortion-drive"]
    distortion_drive: FloatParam,

    #[id = "compressor-enabled"]
    compressor_enabled: BoolParam,

    #[id = "compressor-amount"]
    compressor_amount: FloatParam,

    #[id = "eq-enabled"]
    eq_enabled: BoolParam,

    #[id = "eq-low"]
    eq_low_db: FloatParam,

    #[id = "eq-mid"]
    eq_mid_db: FloatParam,

    #[id = "eq-high"]
    eq_high_db: FloatParam,
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
            reverb_enabled: BoolParam::new("Reverb", false),
            reverb_amount: FloatParam::new(
                "Reverb Amount",
                0.25,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_unit(" %")
            .with_value_to_string(formatters::v2s_f32_percentage(0))
            .with_string_to_value(formatters::s2v_f32_percentage()),
            distortion_enabled: BoolParam::new("Distortion", false),
            distortion_drive: FloatParam::new(
                "Distortion Drive",
                4.0,
                FloatRange::Skewed {
                    min: 1.0,
                    max: 20.0,
                    factor: FloatRange::skew_factor(-1.0),
                },
            )
            .with_unit("x")
            .with_value_to_string(formatters::v2s_f32_rounded(1)),
            compressor_enabled: BoolParam::new("Compressor", false),
            compressor_amount: FloatParam::new(
                "Compression",
                0.5,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_unit(" %")
            .with_value_to_string(formatters::v2s_f32_percentage(0))
            .with_string_to_value(formatters::s2v_f32_percentage()),
            eq_enabled: BoolParam::new("3-Band EQ", false),
            eq_low_db: eq_parameter("EQ Low"),
            eq_mid_db: eq_parameter("EQ Mid"),
            eq_high_db: eq_parameter("EQ High"),
        }
    }
}

fn eq_parameter(name: &'static str) -> FloatParam {
    FloatParam::new(
        name,
        0.0,
        FloatRange::Linear {
            min: -12.0,
            max: 12.0,
        },
    )
    .with_unit(" dB")
    .with_value_to_string(formatters::v2s_f32_rounded(1))
}

impl Default for TinyViolinShowcase {
    fn default() -> Self {
        let (audio_keyboard, editor_keyboard) = keyboard_channel(256);
        Self {
            params: Arc::new(ShowcaseParams::default()),
            processor: ShowcaseProcessor::with_channels(DEFAULT_SAMPLE_RATE, DEFAULT_CHANNELS)
                .expect("the fixed default audio configuration is valid"),
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

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &AUDIO_IO_LAYOUTS;
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
        audio_io_layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        _context: &mut impl InitContext<Self>,
    ) -> bool {
        let Some(input_channels) = audio_io_layout.main_input_channels else {
            return false;
        };
        if audio_io_layout.main_output_channels != Some(input_channels) {
            return false;
        }
        let Ok(channel_count) = usize::try_from(input_channels.get()) else {
            return false;
        };
        match ShowcaseProcessor::with_channels(buffer_config.sample_rate, channel_count) {
            Ok(processor) => {
                self.processor = processor;
                true
            }
            Err(_) => false,
        }
    }

    fn reset(&mut self) {
        let sample_rate = self.processor.sample_rate();
        let channel_count = self.processor.channel_count();
        if let Ok(processor) = ShowcaseProcessor::with_channels(sample_rate, channel_count) {
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

        let settings = tinyviolin::EffectSettings {
            reverb_enabled: self.params.reverb_enabled.value(),
            reverb_amount: self.params.reverb_amount.value(),
            distortion_enabled: self.params.distortion_enabled.value(),
            distortion_drive: self.params.distortion_drive.value(),
            compressor_enabled: self.params.compressor_enabled.value(),
            compressor_amount: self.params.compressor_amount.value(),
            eq_enabled: self.params.eq_enabled.value(),
            eq_low_db: self.params.eq_low_db.value(),
            eq_mid_db: self.params.eq_mid_db.value(),
            eq_high_db: self.params.eq_high_db.value(),
        };
        if let Err(error) = self.processor.set_effect_settings(settings) {
            return process_error(error);
        }

        let channels = buffer.as_slice();
        let Some(block_len) = channels.first().map(|channel| channel.len()) else {
            return ProcessStatus::Error("tinyviolin requires input and output channels");
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
                    let timing = (timing as usize).min(block_len).max(cursor);
                    if let Err(error) = self.processor.render_channels(channels, cursor..timing) {
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
                    let timing = (timing as usize).min(block_len).max(cursor);
                    if let Err(error) = self.processor.render_channels(channels, cursor..timing) {
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

        if let Err(error) = self.processor.render_channels(channels, cursor..block_len) {
            return process_error(error);
        }
        apply_gain(channels, || self.params.master_gain.smoothed.next());

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
        ClapFeature::AudioEffect,
        ClapFeature::Synthesizer,
        ClapFeature::MultiEffects,
        ClapFeature::Reverb,
        ClapFeature::Distortion,
        ClapFeature::Compressor,
        ClapFeature::Equalizer,
        ClapFeature::Stereo,
        ClapFeature::Mono,
        ClapFeature::Surround,
    ];
}

#[cfg(feature = "vst3")]
impl Vst3Plugin for TinyViolinShowcase {
    const VST3_CLASS_ID: [u8; 16] = *b"TinyViolinSynth1";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] = &[
        Vst3SubCategory::Instrument,
        Vst3SubCategory::Fx,
        Vst3SubCategory::Synth,
        Vst3SubCategory::Reverb,
        Vst3SubCategory::Distortion,
        Vst3SubCategory::Dynamics,
        Vst3SubCategory::Eq,
    ];
}

nice_export_clap!(TinyViolinShowcase);
#[cfg(feature = "vst3")]
nice_export_vst3!(TinyViolinShowcase);

#[cfg(test)]
mod tests {
    use super::{MAX_PLUGIN_CHANNELS, ShowcaseParams, TinyViolinShowcase};
    use nice_plug::prelude::Plugin;

    #[test]
    fn plugin_accepts_matched_input_output_layouts_through_vst3_limit() {
        let layouts = <TinyViolinShowcase as Plugin>::AUDIO_IO_LAYOUTS;
        assert_eq!(layouts.len(), MAX_PLUGIN_CHANNELS);
        assert_eq!(layouts[0].main_input_channels.unwrap().get(), 2);
        assert_eq!(layouts[1].main_input_channels.unwrap().get(), 1);
        for layout in layouts {
            assert_eq!(layout.main_input_channels, layout.main_output_channels);
        }
        assert_eq!(
            layouts.last().unwrap().main_input_channels.unwrap().get(),
            63
        );
    }

    #[test]
    fn effects_are_bypassed_by_default_with_useful_control_values() {
        let params = ShowcaseParams::default();
        assert!(!params.reverb_enabled.value());
        assert!((params.reverb_amount.value() - 0.25).abs() < f32::EPSILON);
        assert!(!params.distortion_enabled.value());
        assert!((params.distortion_drive.value() - 4.0).abs() < f32::EPSILON);
        assert!(!params.compressor_enabled.value());
        assert!((params.compressor_amount.value() - 0.5).abs() < f32::EPSILON);
        assert!(!params.eq_enabled.value());
        assert!(params.eq_low_db.value().abs() < f32::EPSILON);
        assert!(params.eq_mid_db.value().abs() < f32::EPSILON);
        assert!(params.eq_high_db.value().abs() < f32::EPSILON);
    }

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
