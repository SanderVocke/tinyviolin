//! Post-processing effects for multichannel audio.

use core::array;
use core::f32::consts::TAU;

const VOCODER_BANDS: usize = 16;
const VOCODER_MIN_FREQUENCY_HZ: f32 = 100.0;
const VOCODER_MAX_FREQUENCY_HZ: f32 = 8_000.0;
const VOCODER_FILTER_Q: f32 = 2.0;
const VOCODER_ATTACK_SECONDS: f32 = 0.005;
const VOCODER_RELEASE_SECONDS: f32 = 0.05;
const VOCODER_MAX_SENSITIVITY_GAIN: f32 = 20.0;

/// Settings for the post-processing effects.
///
/// Vocoder mix, vocoder sensitivity, reverb amount, and compressor amount are
/// normalized values in `0.0..=1.0`. Distortion drive is a linear multiplier
/// in `1.0..=20.0`. The equalizer's low, mid, and high controls are gains in
/// decibels in `-12.0..=12.0`. Every effect is bypassed by default.
#[derive(Clone, Copy, Debug, PartialEq)]
#[allow(clippy::struct_excessive_bools)] // Each independent effect has a public bypass toggle.
pub struct EffectSettings {
    /// Enable the 16-band vocoder.
    pub vocoder_enabled: bool,
    /// Vocoder dry/wet mix in `0.0..=1.0`.
    pub vocoder_mix: f32,
    /// Vocoder modulator sensitivity in `0.0..=1.0`.
    pub vocoder_sensitivity: f32,
    /// Enable the algorithmic reverb.
    pub reverb_enabled: bool,
    /// Reverb dry/wet amount in `0.0..=1.0`.
    pub reverb_amount: f32,
    /// Enable the waveshaping distortion.
    pub distortion_enabled: bool,
    /// Distortion input drive in `1.0..=20.0`.
    pub distortion_drive: f32,
    /// Enable the one-knob compressor.
    pub compressor_enabled: bool,
    /// Compression strength in `0.0..=1.0`.
    pub compressor_amount: f32,
    /// Enable the three-band equalizer.
    pub eq_enabled: bool,
    /// Low-band gain in decibels in `-12.0..=12.0`.
    pub eq_low_db: f32,
    /// Mid-band gain in decibels in `-12.0..=12.0`.
    pub eq_mid_db: f32,
    /// High-band gain in decibels in `-12.0..=12.0`.
    pub eq_high_db: f32,
}

impl Default for EffectSettings {
    fn default() -> Self {
        Self {
            vocoder_enabled: false,
            vocoder_mix: 1.0,
            vocoder_sensitivity: 0.5,
            reverb_enabled: false,
            reverb_amount: 0.25,
            distortion_enabled: false,
            distortion_drive: 4.0,
            compressor_enabled: false,
            compressor_amount: 0.5,
            eq_enabled: false,
            eq_low_db: 0.0,
            eq_mid_db: 0.0,
            eq_high_db: 0.0,
        }
    }
}

pub(crate) struct Vocoder {
    bands: [VocoderBand; VOCODER_BANDS],
}

impl Vocoder {
    pub(crate) fn new(sample_rate: f32) -> Self {
        let ratio = VOCODER_MAX_FREQUENCY_HZ / VOCODER_MIN_FREQUENCY_HZ;
        Self {
            bands: array::from_fn(|index| {
                #[allow(clippy::cast_precision_loss)]
                let position = index as f32 / (VOCODER_BANDS - 1) as f32;
                let center_hz = VOCODER_MIN_FREQUENCY_HZ * ratio.powf(position);
                VocoderBand::new(sample_rate, center_hz)
            }),
        }
    }

    pub(crate) fn reset(&mut self) {
        for band in &mut self.bands {
            band.reset();
        }
    }

    pub(crate) fn process(&mut self, modulator: f32, carrier: f32, sensitivity: f32) -> f32 {
        let sensitivity_gain = sensitivity * VOCODER_MAX_SENSITIVITY_GAIN;
        self.bands
            .iter_mut()
            .map(|band| band.process(modulator, carrier, sensitivity_gain))
            .sum()
    }
}

struct VocoderBand {
    analyzer: Biquad,
    carrier: Biquad,
    envelope: f32,
    attack_coefficient: f32,
    release_coefficient: f32,
}

impl VocoderBand {
    fn new(sample_rate: f32, center_hz: f32) -> Self {
        Self {
            analyzer: Biquad::band_pass(sample_rate, center_hz, VOCODER_FILTER_Q),
            carrier: Biquad::band_pass(sample_rate, center_hz, VOCODER_FILTER_Q),
            envelope: 0.0,
            attack_coefficient: envelope_coefficient(sample_rate, VOCODER_ATTACK_SECONDS),
            release_coefficient: envelope_coefficient(sample_rate, VOCODER_RELEASE_SECONDS),
        }
    }

    fn reset(&mut self) {
        self.analyzer.reset();
        self.carrier.reset();
        self.envelope = 0.0;
    }

    fn process(&mut self, modulator: f32, carrier: f32, sensitivity_gain: f32) -> f32 {
        let level = self.analyzer.process(modulator).abs();
        let coefficient = if level > self.envelope {
            self.attack_coefficient
        } else {
            self.release_coefficient
        };
        self.envelope += coefficient * (level - self.envelope);
        let shaped_envelope = (self.envelope * sensitivity_gain).min(1.0);
        self.carrier.process(carrier) * shaped_envelope
    }
}

struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
}

impl Biquad {
    fn band_pass(sample_rate: f32, frequency_hz: f32, q: f32) -> Self {
        let frequency_hz = frequency_hz.min(sample_rate * 0.45);
        let omega = TAU * frequency_hz / sample_rate;
        let sine = omega.sin();
        let cosine = omega.cos();
        let alpha = sine / (2.0 * q);
        let a0_recip = (1.0 + alpha).recip();
        Self {
            b0: alpha * a0_recip,
            b1: 0.0,
            b2: -alpha * a0_recip,
            a1: -2.0 * cosine * a0_recip,
            a2: (1.0 - alpha) * a0_recip,
            z1: 0.0,
            z2: 0.0,
        }
    }

    fn reset(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }

    fn process(&mut self, input: f32) -> f32 {
        let output = self.b0 * input + self.z1;
        self.z1 = suppress_denormal(self.b1 * input - self.a1 * output + self.z2);
        self.z2 = suppress_denormal(self.b2 * input - self.a2 * output);
        suppress_denormal(output)
    }
}

pub(crate) struct ChannelEffects {
    vocoder: Vocoder,
    equalizer: ThreeBandEq,
    compressor: Compressor,
    reverb: Reverb,
}

impl ChannelEffects {
    pub(crate) fn new(sample_rate: f32) -> Self {
        Self {
            vocoder: Vocoder::new(sample_rate),
            equalizer: ThreeBandEq::new(sample_rate),
            compressor: Compressor::new(sample_rate),
            reverb: Reverb::new(sample_rate),
        }
    }

    pub(crate) fn reset_dsp(&mut self) {
        self.vocoder.reset();
        self.equalizer.reset();
        self.compressor.reset();
        self.reverb.reset();
    }

    pub(crate) fn reset_vocoder(&mut self) {
        self.vocoder.reset();
    }

    pub(crate) fn reset_equalizer(&mut self) {
        self.equalizer.reset();
    }

    pub(crate) fn reset_compressor(&mut self) {
        self.compressor.reset();
    }

    pub(crate) fn reset_reverb(&mut self) {
        self.reverb.reset();
    }

    pub(crate) fn process(
        &mut self,
        modulator: f32,
        carrier: f32,
        settings: EffectSettings,
    ) -> f32 {
        let dry = modulator + carrier;
        let vocoded = if settings.vocoder_enabled {
            self.vocoder
                .process(modulator, carrier, settings.vocoder_sensitivity)
        } else {
            dry
        };
        let vocoder_output = dry + settings.vocoder_mix * (vocoded - dry);
        let distorted = if settings.distortion_enabled {
            distort(vocoder_output, settings.distortion_drive)
        } else {
            vocoder_output
        };
        let equalized = if settings.eq_enabled {
            self.equalizer.process(
                distorted,
                settings.eq_low_db,
                settings.eq_mid_db,
                settings.eq_high_db,
            )
        } else {
            distorted
        };
        let compressed = if settings.compressor_enabled {
            self.compressor
                .process(equalized, settings.compressor_amount)
        } else {
            equalized
        };
        let output = if settings.reverb_enabled {
            self.reverb.process(compressed, settings.reverb_amount)
        } else {
            compressed
        };
        output.clamp(-1.0, 1.0)
    }
}

struct ThreeBandEq {
    low: f32,
    low_and_mid: f32,
    low_coefficient: f32,
    high_coefficient: f32,
}

impl ThreeBandEq {
    fn new(sample_rate: f32) -> Self {
        Self {
            low: 0.0,
            low_and_mid: 0.0,
            low_coefficient: low_pass_coefficient(sample_rate, 250.0),
            high_coefficient: low_pass_coefficient(sample_rate, 4_000.0),
        }
    }

    fn reset(&mut self) {
        self.low = 0.0;
        self.low_and_mid = 0.0;
    }

    fn process(&mut self, input: f32, low_db: f32, mid_db: f32, high_db: f32) -> f32 {
        self.low += self.low_coefficient * (input - self.low);
        self.low_and_mid += self.high_coefficient * (input - self.low_and_mid);
        let mid = self.low_and_mid - self.low;
        let high = input - self.low_and_mid;
        self.low * db_to_gain(low_db) + mid * db_to_gain(mid_db) + high * db_to_gain(high_db)
    }
}

struct Compressor {
    envelope: f32,
    attack_coefficient: f32,
    release_coefficient: f32,
}

impl Compressor {
    fn new(sample_rate: f32) -> Self {
        Self {
            envelope: 0.0,
            attack_coefficient: envelope_coefficient(sample_rate, 0.01),
            release_coefficient: envelope_coefficient(sample_rate, 0.1),
        }
    }

    fn reset(&mut self) {
        self.envelope = 0.0;
    }

    fn process(&mut self, input: f32, amount: f32) -> f32 {
        let level = input.abs();
        let coefficient = if level > self.envelope {
            self.attack_coefficient
        } else {
            self.release_coefficient
        };
        self.envelope += coefficient * (level - self.envelope);

        if amount <= 0.0 {
            return input;
        }
        let threshold = db_to_gain(-30.0 * amount);
        if self.envelope <= threshold {
            return input;
        }
        let ratio = 1.0 + 7.0 * amount;
        let compressed_level = threshold * (self.envelope / threshold).powf(ratio.recip());
        input * (compressed_level / self.envelope)
    }
}

struct Reverb {
    combs: [Comb; 3],
    all_pass: Comb,
}

impl Reverb {
    fn new(sample_rate: f32) -> Self {
        // Mutually unrelated delay lengths keep the feedback echoes from lining
        // up. The values are intentionally small enough for inexpensive use on
        // every channel.
        Self {
            combs: [
                Comb::new(delay_samples(sample_rate, 0.0297)),
                Comb::new(delay_samples(sample_rate, 0.0371)),
                Comb::new(delay_samples(sample_rate, 0.0411)),
            ],
            all_pass: Comb::new(delay_samples(sample_rate, 0.005)),
        }
    }

    fn reset(&mut self) {
        for comb in &mut self.combs {
            comb.reset();
        }
        self.all_pass.reset();
    }

    fn process(&mut self, input: f32, amount: f32) -> f32 {
        let feedback = 0.68 + 0.2 * amount;
        let wet = self
            .combs
            .iter_mut()
            .map(|comb| comb.process_feedback(input, feedback))
            .sum::<f32>()
            / 3.0;
        let diffused = self.all_pass.process_all_pass(wet, 0.5);
        input + amount * (diffused - input)
    }
}

struct Comb {
    delay: Box<[f32]>,
    index: usize,
}

impl Comb {
    fn new(length: usize) -> Self {
        Self {
            delay: vec![0.0; length].into_boxed_slice(),
            index: 0,
        }
    }

    fn reset(&mut self) {
        self.delay.fill(0.0);
        self.index = 0;
    }

    fn process_feedback(&mut self, input: f32, feedback: f32) -> f32 {
        let delayed = self.delay[self.index];
        self.delay[self.index] = suppress_denormal(input + delayed * feedback);
        self.advance();
        delayed
    }

    fn process_all_pass(&mut self, input: f32, feedback: f32) -> f32 {
        let delayed = self.delay[self.index];
        let output = delayed - input;
        self.delay[self.index] = suppress_denormal(input + delayed * feedback);
        self.advance();
        output
    }

    fn advance(&mut self) {
        self.index += 1;
        if self.index == self.delay.len() {
            self.index = 0;
        }
    }
}

fn low_pass_coefficient(sample_rate: f32, frequency_hz: f32) -> f32 {
    let frequency_hz = frequency_hz.min(sample_rate * 0.45);
    1.0 - (-TAU * frequency_hz / sample_rate).exp()
}

fn envelope_coefficient(sample_rate: f32, seconds: f32) -> f32 {
    1.0 - (-1.0 / (sample_rate * seconds)).exp()
}

fn db_to_gain(db: f32) -> f32 {
    10.0_f32.powf(db / 20.0)
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn delay_samples(sample_rate: f32, seconds: f32) -> usize {
    (sample_rate * seconds).round().max(1.0) as usize
}

fn suppress_denormal(value: f32) -> f32 {
    if value.abs() < 1.0e-20 { 0.0 } else { value }
}

fn distort(input: f32, drive: f32) -> f32 {
    (input * drive).tanh() / drive.tanh()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::float_cmp)] // Exact zero proves that neither raw vocoder source leaks.
    use core::f32::consts::TAU;

    use super::{Biquad, ChannelEffects, EffectSettings, Vocoder};

    fn sine(phase: usize, frequency_hz: f32, sample_rate: f32) -> f32 {
        #[allow(clippy::cast_precision_loss)]
        let seconds = phase as f32 / sample_rate;
        (TAU * frequency_hz * seconds).sin()
    }

    #[test]
    fn band_pass_is_frequency_selective_and_resettable() {
        let sample_rate = 48_000.0;
        let energy = |frequency_hz| {
            let mut filter = Biquad::band_pass(sample_rate, 1_000.0, 2.0);
            (0..48_000)
                .map(|frame| filter.process(sine(frame, frequency_hz, sample_rate)).abs())
                .skip(4_800)
                .sum::<f32>()
        };
        assert!(energy(1_000.0) > energy(100.0) * 5.0);

        let mut filter = Biquad::band_pass(sample_rate, 1_000.0, 2.0);
        for frame in 0..1_000 {
            let _ = filter.process(sine(frame, 1_000.0, sample_rate));
        }
        filter.reset();
        assert_eq!(filter.process(0.0), 0.0);
    }

    #[test]
    fn vocoder_requires_both_sources_and_sensitivity_controls_response() {
        let sample_rate = 48_000.0;
        let render = |modulator_gain: f32, carrier_gain: f32, sensitivity: f32| {
            let mut vocoder = Vocoder::new(sample_rate);
            (0..24_000)
                .map(|frame| {
                    let modulator = modulator_gain
                        * (sine(frame, 220.0, sample_rate) + 0.5 * sine(frame, 880.0, sample_rate));
                    let carrier = carrier_gain
                        * (sine(frame, 110.0, sample_rate)
                            + 0.5 * sine(frame, 330.0, sample_rate)
                            + 0.25 * sine(frame, 990.0, sample_rate));
                    vocoder.process(modulator, carrier, sensitivity).abs()
                })
                .skip(4_800)
                .sum::<f32>()
        };

        assert_eq!(render(1.0, 0.0, 1.0), 0.0);
        assert_eq!(render(0.0, 1.0, 1.0), 0.0);
        let low = render(0.05, 0.25, 0.25);
        let high = render(0.05, 0.25, 0.75);
        assert!(low > 0.0);
        assert!(high > low * 2.0);
    }

    #[test]
    fn vocoder_envelope_releases_and_reset_clears_all_state() {
        let mut vocoder = Vocoder::new(1_000.0);
        for frame in 0..500 {
            let sample = sine(frame, 100.0, 1_000.0);
            let _ = vocoder.process(sample, sample, 1.0);
        }
        let mut tail = 0.0_f32;
        for frame in 0..1_000 {
            tail = vocoder.process(0.0, sine(frame, 100.0, 1_000.0), 1.0).abs();
        }
        assert!(tail < 1.0e-5);

        vocoder.reset();
        assert_eq!(vocoder.process(0.0, 0.0, 1.0), 0.0);
    }

    #[test]
    fn vocoder_remains_finite_at_supported_sample_rate_extremes() {
        for sample_rate in [1.0, 8_000.0, 48_000.0, 768_000.0] {
            let mut vocoder = Vocoder::new(sample_rate);
            for frame in 0..10_000 {
                #[allow(clippy::cast_precision_loss)]
                let input = (frame as f32 * 0.17).sin();
                let output = vocoder.process(input, -input, 1.0);
                assert!(output.is_finite());
            }
        }
    }

    #[test]
    fn distortion_is_bounded_and_adds_harmonic_shaping() {
        let mut effects = ChannelEffects::new(48_000.0);
        let settings = EffectSettings {
            distortion_enabled: true,
            distortion_drive: 8.0,
            ..EffectSettings::default()
        };
        let quiet = effects.process(0.05, 0.0, settings);
        let loud = effects.process(0.8, 0.0, settings);
        assert!(quiet > 0.05);
        assert!(loud <= 1.0);
    }

    #[test]
    fn compressor_reduces_sustained_loud_audio() {
        let mut effects = ChannelEffects::new(48_000.0);
        let settings = EffectSettings {
            compressor_enabled: true,
            compressor_amount: 1.0,
            ..EffectSettings::default()
        };
        let mut output = 0.0;
        for _ in 0..4_800 {
            output = effects.process(0.8, 0.0, settings);
        }
        assert!(output > 0.0);
        assert!(output < 0.3);
    }

    #[test]
    fn equalizer_has_neutral_zero_db_and_independent_bands() {
        let mut neutral = ChannelEffects::new(48_000.0);
        let neutral_settings = EffectSettings {
            eq_enabled: true,
            ..EffectSettings::default()
        };
        for input in [0.25, -0.5, 0.75, -0.125] {
            assert!((neutral.process(input, 0.0, neutral_settings) - input).abs() < 1.0e-6);
        }

        let mut boosted = ChannelEffects::new(48_000.0);
        let boosted_settings = EffectSettings {
            eq_enabled: true,
            eq_low_db: 12.0,
            ..EffectSettings::default()
        };
        let mut output = 0.0;
        for _ in 0..4_800 {
            output = boosted.process(0.1, 0.0, boosted_settings);
        }
        assert!(output > 0.3);
    }

    #[test]
    fn reverb_produces_a_finite_tail() {
        let mut effects = ChannelEffects::new(1_000.0);
        let settings = EffectSettings {
            reverb_enabled: true,
            reverb_amount: 0.5,
            ..EffectSettings::default()
        };
        let _ = effects.process(1.0, 0.0, settings);
        let mut tail = false;
        for _ in 0..100 {
            let sample = effects.process(0.0, 0.0, settings);
            assert!(sample.is_finite());
            tail |= sample.abs() > 0.0;
        }
        assert!(tail);
    }
}
