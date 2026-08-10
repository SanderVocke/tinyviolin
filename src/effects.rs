//! Post-processing effects for multichannel audio.

/// Settings for the post-processing effects.
///
/// Reverb amount is a dry/wet value in `0.0..=1.0`. Distortion drive is a
/// linear multiplier in `1.0..=20.0`. Both effects are bypassed by default.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EffectSettings {
    /// Enable the algorithmic reverb.
    pub reverb_enabled: bool,
    /// Reverb dry/wet amount in `0.0..=1.0`.
    pub reverb_amount: f32,
    /// Enable the waveshaping distortion.
    pub distortion_enabled: bool,
    /// Distortion input drive in `1.0..=20.0`.
    pub distortion_drive: f32,
}

impl Default for EffectSettings {
    fn default() -> Self {
        Self {
            reverb_enabled: false,
            reverb_amount: 0.25,
            distortion_enabled: false,
            distortion_drive: 4.0,
        }
    }
}

pub(crate) struct ChannelEffects {
    reverb: Reverb,
}

impl ChannelEffects {
    pub(crate) fn new(sample_rate: f32) -> Self {
        Self {
            reverb: Reverb::new(sample_rate),
        }
    }

    pub(crate) fn reset_reverb(&mut self) {
        self.reverb.reset();
    }

    pub(crate) fn process(&mut self, input: f32, settings: EffectSettings) -> f32 {
        let distorted = if settings.distortion_enabled {
            distort(input, settings.distortion_drive)
        } else {
            input
        };
        let output = if settings.reverb_enabled {
            self.reverb.process(distorted, settings.reverb_amount)
        } else {
            distorted
        };
        output.clamp(-1.0, 1.0)
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
