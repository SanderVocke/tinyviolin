use nice_plug::prelude::Enum;
use tinyviolin::Instrument;

#[derive(Clone, Copy, Debug, Enum, Eq, PartialEq)]
pub enum Preset {
    #[id = "sine"]
    Sine,
    #[id = "square"]
    Square,
    #[id = "triangle"]
    Triangle,
    #[id = "bass"]
    Bass,
    #[id = "pad"]
    Pad,
    #[id = "lead"]
    Lead,
    #[id = "bass-drum"]
    #[name = "Bass Drum"]
    BassDrum,
    #[id = "tom"]
    Tom,
    #[id = "snare"]
    Snare,
    #[id = "hi-hat"]
    #[name = "Hi-Hat"]
    HiHat,
}

impl Preset {
    pub const ALL: [Self; 10] = [
        Self::Sine,
        Self::Square,
        Self::Triangle,
        Self::Bass,
        Self::Pad,
        Self::Lead,
        Self::BassDrum,
        Self::Tom,
        Self::Snare,
        Self::HiHat,
    ];

    #[must_use]
    pub const fn instrument(self) -> Instrument {
        match self {
            Self::Sine => Instrument::Sine,
            Self::Square => Instrument::Square,
            Self::Triangle => Instrument::Triangle,
            Self::Bass => Instrument::Bass,
            Self::Pad => Instrument::Pad,
            Self::Lead => Instrument::Lead,
            Self::BassDrum => Instrument::BassDrum,
            Self::Tom => Instrument::Tom,
            Self::Snare => Instrument::Snare,
            Self::HiHat => Instrument::HiHat,
        }
    }

    #[must_use]
    pub fn frequency_hz(self, midi_note: u8) -> f32 {
        match self {
            Self::BassDrum => 60.0,
            Self::Tom => 130.0,
            Self::Snare => 180.0,
            Self::HiHat => 6_000.0,
            _ => 440.0 * 2.0_f32.powf((f32::from(midi_note) - 69.0) / 12.0),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::float_cmp)] // Fixed preset frequencies should be represented exactly.
    use super::Preset;
    use nice_plug::prelude::Enum;
    use tinyviolin::Instrument;

    #[test]
    fn all_presets_have_stable_ids_and_instrument_mappings() {
        assert_eq!(Preset::variants().len(), 10);
        assert_eq!(
            Preset::ids(),
            Some(
                [
                    "sine",
                    "square",
                    "triangle",
                    "bass",
                    "pad",
                    "lead",
                    "bass-drum",
                    "tom",
                    "snare",
                    "hi-hat",
                ]
                .as_slice()
            )
        );
        assert_eq!(
            Preset::ALL.map(Preset::instrument),
            [
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
            ]
        );
    }

    #[test]
    fn melodic_and_percussion_pitches_are_expected() {
        assert!((Preset::Sine.frequency_hz(69) - 440.0).abs() < f32::EPSILON);
        assert!((Preset::Lead.frequency_hz(81) - 880.0).abs() < 0.001);
        assert_eq!(Preset::BassDrum.frequency_hz(0), 60.0);
        assert_eq!(Preset::Tom.frequency_hz(127), 130.0);
        assert_eq!(Preset::Snare.frequency_hz(60), 180.0);
        assert_eq!(Preset::HiHat.frequency_hz(60), 6_000.0);
    }
}
