//! Post-processing effects for multichannel audio.

use core::f32::consts::TAU;

/// Settings for the post-processing effects.
///
/// Reverb and compressor amounts are normalized values in `0.0..=1.0`.
/// Distortion drive is a linear multiplier in `1.0..=20.0`. The equalizer's
/// low, mid, and high controls are gains in decibels in `-12.0..=12.0`. Every
/// effect is bypassed by default. The noise gate threshold is in decibels in
/// `-80.0..=0.0`.
#[derive(Clone, Copy, Debug, PartialEq)]
#[allow(clippy::struct_excessive_bools)] // Each independent effect has a public bypass toggle.
pub struct EffectSettings {
    /// Enable the noise gate.
    pub noise_gate_enabled: bool,
    /// Noise gate threshold in decibels in `-80.0..=0.0`.
    pub noise_gate_threshold_db: f32,
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
            noise_gate_enabled: false,
            noise_gate_threshold_db: -50.0,
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

pub(crate) struct ChannelEffects {
    noise_gate: NoiseGate,
    equalizer: ThreeBandEq,
    compressor: Compressor,
    reverb: Reverb,
}

impl ChannelEffects {
    pub(crate) fn new(sample_rate: f32) -> Self {
        Self {
            noise_gate: NoiseGate::new(sample_rate),
            equalizer: ThreeBandEq::new(sample_rate),
            compressor: Compressor::new(sample_rate),
            reverb: Reverb::new(sample_rate),
        }
    }

    pub(crate) fn reset_dsp(&mut self) {
        self.noise_gate.reset();
        self.equalizer.reset();
        self.compressor.reset();
        self.reverb.reset();
    }

    pub(crate) fn reset_noise_gate(&mut self) {
        self.noise_gate.reset();
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

    pub(crate) fn process(&mut self, input: f32, settings: EffectSettings) -> f32 {
        let gated = if settings.noise_gate_enabled {
            self.noise_gate
                .process(input, settings.noise_gate_threshold_db)
        } else {
            input
        };
        let distorted = if settings.distortion_enabled {
            distort(gated, settings.distortion_drive)
        } else {
            gated
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

/// A peak-operated gate with fixed musical timing: a fast 2 ms opening avoids
/// dulling transients, a 50 ms hold prevents chatter, and a 100 ms closing ramp
/// preserves natural note and room-noise decays.
struct NoiseGate {
    gain: f32,
    hold_samples: usize,
    hold_remaining: usize,
    attack_coefficient: f32,
    release_coefficient: f32,
}

impl NoiseGate {
    fn new(sample_rate: f32) -> Self {
        Self {
            gain: 0.0,
            hold_samples: delay_samples(sample_rate, 0.05),
            hold_remaining: 0,
            attack_coefficient: envelope_coefficient(sample_rate, 0.002),
            release_coefficient: envelope_coefficient(sample_rate, 0.1),
        }
    }

    fn reset(&mut self) {
        self.gain = 0.0;
        self.hold_remaining = 0;
    }

    fn process(&mut self, input: f32, threshold_db: f32) -> f32 {
        let open = input.abs() >= db_to_gain(threshold_db);
        let target = if open {
            self.hold_remaining = self.hold_samples;
            1.0
        } else if self.hold_remaining > 0 {
            self.hold_remaining -= 1;
            1.0
        } else {
            0.0
        };
        let coefficient = if target > self.gain {
            self.attack_coefficient
        } else {
            self.release_coefficient
        };
        self.gain += coefficient * (target - self.gain);
        input * self.gain
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
    use super::{ChannelEffects, EffectSettings};

    #[test]
    fn distortion_is_bounded_and_adds_harmonic_shaping() {
        let mut effects = ChannelEffects::new(48_000.0);
        let settings = EffectSettings {
            distortion_enabled: true,
            distortion_drive: 8.0,
            ..EffectSettings::default()
        };
        let quiet = effects.process(0.05, settings);
        let loud = effects.process(0.8, settings);
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
            output = effects.process(0.8, settings);
        }
        assert!(output > 0.0);
        assert!(output < 0.3);
    }

    #[test]
    fn noise_gate_opens_quickly_holds_and_closes_smoothly() {
        let mut effects = ChannelEffects::new(1_000.0);
        let settings = EffectSettings {
            noise_gate_enabled: true,
            noise_gate_threshold_db: -20.0,
            ..EffectSettings::default()
        };

        for _ in 0..10 {
            assert!(effects.process(0.01, settings).abs() < f32::EPSILON);
        }
        let opened = effects.process(0.5, settings);
        assert!(opened > 0.0 && opened < 0.5);
        for _ in 0..50 {
            let _ = effects.process(0.05, settings);
        }
        let held = effects.process(0.05, settings);
        assert!(held > 0.04);
        let mut closing = held;
        for _ in 0..500 {
            closing = effects.process(0.05, settings);
        }
        assert!(closing < 0.001);
    }

    #[test]
    fn equalizer_has_neutral_zero_db_and_independent_bands() {
        let mut neutral = ChannelEffects::new(48_000.0);
        let neutral_settings = EffectSettings {
            eq_enabled: true,
            ..EffectSettings::default()
        };
        for input in [0.25, -0.5, 0.75, -0.125] {
            assert!((neutral.process(input, neutral_settings) - input).abs() < 1.0e-6);
        }

        let mut boosted = ChannelEffects::new(48_000.0);
        let boosted_settings = EffectSettings {
            eq_enabled: true,
            eq_low_db: 12.0,
            ..EffectSettings::default()
        };
        let mut output = 0.0;
        for _ in 0..4_800 {
            output = boosted.process(0.1, boosted_settings);
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
        let _ = effects.process(1.0, settings);
        let mut tail = false;
        for _ in 0..100 {
            let sample = effects.process(0.0, settings);
            assert!(sample.is_finite());
            tail |= sample.abs() > 0.0;
        }
        assert!(tail);
    }
}
