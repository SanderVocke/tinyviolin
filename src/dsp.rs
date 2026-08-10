use core::f32::consts::TAU;

use crate::{Instrument, VoiceId};

#[derive(Clone, Copy)]
pub(crate) struct Voice {
    pub(crate) active: bool,
    pub(crate) released: bool,
    pub(crate) id: VoiceId,
    pub(crate) started_at: u64,
    instrument: Instrument,
    frequency_hz: f32,
    gain: f32,
    sample_rate: f32,
    elapsed_seconds: f32,
    release_elapsed_seconds: f32,
    release_start: f32,
    phase: [f32; 3],
    noise: u32,
    previous_noise: f32,
}

impl Voice {
    pub(crate) const EMPTY: Self = Self {
        active: false,
        released: false,
        id: VoiceId(0),
        started_at: 0,
        instrument: Instrument::Sine,
        frequency_hz: 440.0,
        gain: 0.0,
        sample_rate: 48_000.0,
        elapsed_seconds: 0.0,
        release_elapsed_seconds: 0.0,
        release_start: 0.0,
        phase: [0.0; 3],
        noise: 1,
        previous_noise: 0.0,
    };

    pub(crate) fn start(
        &mut self,
        id: VoiceId,
        instrument: Instrument,
        frequency_hz: f32,
        gain: f32,
        sample_rate: f32,
        started_at: u64,
    ) {
        let id_bytes = id.0.to_le_bytes();
        let folded_id = u32::from_le_bytes([id_bytes[0], id_bytes[1], id_bytes[2], id_bytes[3]])
            ^ u32::from_le_bytes([id_bytes[4], id_bytes[5], id_bytes[6], id_bytes[7]]);
        let seed = folded_id
            .wrapping_mul(747_796_405)
            .wrapping_add((instrument as u32).wrapping_mul(2_891_336_453))
            | 1;
        *self = Self {
            active: true,
            released: false,
            id,
            started_at,
            instrument,
            frequency_hz,
            gain,
            sample_rate,
            elapsed_seconds: 0.0,
            release_elapsed_seconds: 0.0,
            release_start: 0.0,
            phase: [0.0, 0.25, 0.5],
            noise: seed,
            previous_noise: 0.0,
        };
    }

    pub(crate) fn release(&mut self) {
        if self.active && !self.released {
            self.release_start = self.unreleased_amplitude();
            self.released = true;
            self.release_elapsed_seconds = 0.0;
        }
    }

    pub(crate) fn stop(&mut self) {
        self.active = false;
    }

    pub(crate) fn next_sample(&mut self) -> f32 {
        if !self.active {
            return 0.0;
        }

        let amplitude = self.amplitude();
        if !self.active {
            return 0.0;
        }
        let frequency = self
            .frequency_at(self.elapsed_seconds)
            .min(self.sample_rate * 0.49);
        let raw = match self.instrument {
            Instrument::Sine | Instrument::BassDrum => sine(self.phase[0]),
            Instrument::Square => square(self.phase[0]),
            Instrument::Triangle => triangle(self.phase[0]),
            Instrument::Bass => 0.7 * sine(self.phase[0]) + 0.3 * triangle(self.phase[1]),
            Instrument::Pad => {
                0.4 * sine(self.phase[0])
                    + 0.3 * triangle(self.phase[1])
                    + 0.3 * triangle(self.phase[2])
            }
            Instrument::Lead => 0.55 * square(self.phase[0]) + 0.45 * triangle(self.phase[1]),
            Instrument::Tom => 0.8 * sine(self.phase[0]) + 0.2 * triangle(self.phase[1]),
            Instrument::Snare => 0.32 * sine(self.phase[0]) + 0.68 * self.next_noise(),
            Instrument::HiHat => {
                let noise = self.next_noise();
                let high_noise = noise - self.previous_noise;
                self.previous_noise = noise;
                0.55 * high_noise + 0.225 * square(self.phase[0]) + 0.225 * square(self.phase[1])
            }
        };

        let ratios = self.phase_ratios();
        for (phase, ratio) in self.phase.iter_mut().zip(ratios) {
            *phase = advance_phase(*phase, frequency * ratio / self.sample_rate);
        }
        self.elapsed_seconds += self.sample_rate.recip();
        if self.released {
            self.release_elapsed_seconds += self.sample_rate.recip();
        }
        (raw * amplitude * self.gain).clamp(-1.0, 1.0)
    }

    fn phase_ratios(&self) -> [f32; 3] {
        match self.instrument {
            Instrument::Bass | Instrument::Lead => [1.0, 2.0, 1.0],
            Instrument::Pad => [1.0, 0.995, 1.005],
            Instrument::HiHat => [1.0, 1.371, 1.617],
            _ => [1.0; 3],
        }
    }

    fn frequency_at(&self, time: f32) -> f32 {
        match self.instrument {
            Instrument::BassDrum => self.frequency_hz * (0.35 + 1.65 * (-30.0 * time).exp()),
            Instrument::Tom => self.frequency_hz * (1.0 + 0.35 * (-18.0 * time).exp()),
            _ => self.frequency_hz,
        }
    }

    fn amplitude(&mut self) -> f32 {
        if self.released {
            let progress = self.release_elapsed_seconds / release_seconds(self.instrument);
            if progress >= 1.0 {
                self.active = false;
                return 0.0;
            }
            return self.release_start * (1.0 - progress);
        }
        let amplitude = self.unreleased_amplitude();
        if is_one_shot(self.instrument) && amplitude <= 0.0 && self.elapsed_seconds > 0.0 {
            self.active = false;
        }
        amplitude
    }

    fn unreleased_amplitude(&self) -> f32 {
        if let Some((attack, duration)) = one_shot_shape(self.instrument) {
            if self.elapsed_seconds >= duration {
                return 0.0;
            }
            if self.elapsed_seconds < attack {
                return self.elapsed_seconds / attack;
            }
            let progress = (self.elapsed_seconds - attack) / (duration - attack);
            return (1.0 - progress) * (1.0 - progress);
        }

        let (attack, decay, sustain) = melodic_shape(self.instrument);
        if self.elapsed_seconds < attack {
            return self.elapsed_seconds / attack;
        }
        if self.elapsed_seconds < attack + decay {
            let progress = (self.elapsed_seconds - attack) / decay;
            return 1.0 + (sustain - 1.0) * progress;
        }
        sustain
    }

    #[allow(clippy::cast_precision_loss)] // Conversion intentionally quantizes PRNG bits to audio.
    fn next_noise(&mut self) -> f32 {
        let mut value = self.noise;
        value ^= value << 13;
        value ^= value >> 17;
        value ^= value << 5;
        self.noise = value;
        (value as f32 / u32::MAX as f32).mul_add(2.0, -1.0)
    }
}

fn sine(phase: f32) -> f32 {
    (TAU * phase).sin()
}

fn square(phase: f32) -> f32 {
    if phase < 0.5 { 1.0 } else { -1.0 }
}

fn triangle(phase: f32) -> f32 {
    1.0 - 4.0 * (phase - 0.5).abs()
}

fn advance_phase(phase: f32, increment: f32) -> f32 {
    (phase + increment).fract()
}

fn melodic_shape(instrument: Instrument) -> (f32, f32, f32) {
    match instrument {
        Instrument::Bass => (0.005, 0.12, 0.65),
        Instrument::Pad => (0.35, 0.4, 0.75),
        Instrument::Lead => (0.01, 0.08, 0.8),
        _ => (0.005, 0.02, 1.0),
    }
}

fn one_shot_shape(instrument: Instrument) -> Option<(f32, f32)> {
    match instrument {
        Instrument::BassDrum => Some((0.002, 0.5)),
        Instrument::Tom => Some((0.002, 0.55)),
        Instrument::Snare => Some((0.001, 0.32)),
        Instrument::HiHat => Some((0.0005, 0.14)),
        _ => None,
    }
}

fn is_one_shot(instrument: Instrument) -> bool {
    one_shot_shape(instrument).is_some()
}

fn release_seconds(instrument: Instrument) -> f32 {
    match instrument {
        Instrument::Bass => 0.08,
        Instrument::Pad => 0.6,
        Instrument::Lead => 0.12,
        Instrument::BassDrum | Instrument::Tom | Instrument::Snare | Instrument::HiHat => 0.01,
        _ => 0.03,
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp)] // Exact equality is required for silence and determinism checks.
mod tests {
    use super::Voice;
    use crate::{Instrument, VoiceId};

    fn voice(instrument: Instrument, frequency: f32) -> Voice {
        let mut voice = Voice::EMPTY;
        voice.start(VoiceId(7), instrument, frequency, 1.0, 48_000.0, 0);
        voice
    }

    #[test]
    fn every_preset_is_finite_and_bounded() {
        let instruments = [
            Instrument::Sine,
            Instrument::Square,
            Instrument::Triangle,
            Instrument::Bass,
            Instrument::Pad,
            Instrument::Lead,
            Instrument::BassDrum,
            Instrument::Tom,
            Instrument::Snare,
            Instrument::HiHat,
        ];
        for instrument in instruments {
            let mut voice = voice(instrument, 440.0);
            for _ in 0..48_000 {
                let sample = voice.next_sample();
                assert!(sample.is_finite());
                assert!((-1.0..=1.0).contains(&sample));
            }
        }
    }

    #[test]
    fn sine_has_expected_pitch() {
        let mut voice = voice(Instrument::Sine, 480.0);
        let mut prior = 0.0;
        let mut rising_crossings = 0;
        for _ in 0..48_000 {
            let sample = voice.next_sample();
            if prior <= 0.0 && sample > 0.0 {
                rising_crossings += 1;
            }
            prior = sample;
        }
        assert!((479..=481).contains(&rising_crossings));
    }

    #[test]
    fn percussion_finishes_without_note_off() {
        for instrument in [
            Instrument::BassDrum,
            Instrument::Tom,
            Instrument::Snare,
            Instrument::HiHat,
        ] {
            let mut voice = voice(instrument, 220.0);
            for _ in 0..30_000 {
                voice.next_sample();
            }
            assert!(!voice.active);
            assert_eq!(voice.next_sample(), 0.0);
        }
    }

    #[test]
    fn noise_is_deterministic_per_identity() {
        let mut first = voice(Instrument::Snare, 180.0);
        let mut second = voice(Instrument::Snare, 180.0);
        for _ in 0..1_000 {
            assert_eq!(first.next_sample(), second.next_sample());
        }
    }

    #[test]
    fn drum_pitch_drops() {
        let kick = voice(Instrument::BassDrum, 60.0);
        let tom = voice(Instrument::Tom, 120.0);
        assert!(kick.frequency_at(0.0) > kick.frequency_at(0.2));
        assert!(tom.frequency_at(0.0) > tom.frequency_at(0.2));
    }

    #[test]
    fn release_reaches_silence() {
        let mut voice = voice(Instrument::Triangle, 220.0);
        for _ in 0..1_000 {
            voice.next_sample();
        }
        voice.release();
        for _ in 0..2_000 {
            voice.next_sample();
        }
        assert!(!voice.active);
        assert_eq!(voice.next_sample(), 0.0);
    }
}
