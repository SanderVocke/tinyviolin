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
    #[id = "pluck"]
    Pluck,
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
    #[id = "percussion-kit"]
    #[name = "Percussion Kit"]
    PercussionKit,
}

impl Preset {
    pub const ALL: [Self; 12] = [
        Self::Sine,
        Self::Square,
        Self::Triangle,
        Self::Pluck,
        Self::Bass,
        Self::Pad,
        Self::Lead,
        Self::BassDrum,
        Self::Tom,
        Self::Snare,
        Self::HiHat,
        Self::PercussionKit,
    ];

    #[must_use]
    pub const fn instrument(self, midi_note: u8) -> Instrument {
        self.core().instrument(midi_note)
    }

    const fn core(self) -> tinyviolin::Preset {
        match self {
            Self::Sine => tinyviolin::Preset::Sine,
            Self::Square => tinyviolin::Preset::Square,
            Self::Triangle => tinyviolin::Preset::Triangle,
            Self::Pluck => tinyviolin::Preset::Pluck,
            Self::Bass => tinyviolin::Preset::Bass,
            Self::Pad => tinyviolin::Preset::Pad,
            Self::Lead => tinyviolin::Preset::Lead,
            Self::BassDrum => tinyviolin::Preset::BassDrum,
            Self::Tom => tinyviolin::Preset::Tom,
            Self::Snare => tinyviolin::Preset::Snare,
            Self::HiHat => tinyviolin::Preset::HiHat,
            Self::PercussionKit => tinyviolin::Preset::PercussionKit,
        }
    }

    #[must_use]
    pub fn frequency_hz(self, midi_note: u8) -> f32 {
        self.core().frequency_hz(midi_note)
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
        assert_eq!(Preset::variants().len(), 12);
        assert_eq!(
            Preset::ids(),
            Some(
                [
                    "sine",
                    "square",
                    "triangle",
                    "pluck",
                    "bass",
                    "pad",
                    "lead",
                    "bass-drum",
                    "tom",
                    "snare",
                    "hi-hat",
                    "percussion-kit",
                ]
                .as_slice()
            )
        );
        assert_eq!(
            Preset::ALL.map(|preset| preset.instrument(60)),
            [
                Instrument::Sine,
                Instrument::Square,
                Instrument::Triangle,
                Instrument::Pluck,
                Instrument::Bass,
                Instrument::Pad,
                Instrument::Lead,
                Instrument::BassDrum,
                Instrument::Tom,
                Instrument::Snare,
                Instrument::HiHat,
                Instrument::Tom,
            ]
        );
    }

    #[test]
    fn melodic_and_percussion_pitches_are_expected() {
        assert!((Preset::Sine.frequency_hz(69) - 440.0).abs() < f32::EPSILON);
        assert!((Preset::Lead.frequency_hz(81) - 880.0).abs() < 0.001);
        assert!(Preset::BassDrum.frequency_hz(48) < Preset::BassDrum.frequency_hz(72));
        assert!(Preset::Tom.frequency_hz(48) < Preset::Tom.frequency_hz(72));
        assert_eq!(Preset::Snare.frequency_hz(60), 180.0);
        assert_eq!(Preset::HiHat.frequency_hz(60), 6_000.0);
        assert_eq!(Preset::PercussionKit.frequency_hz(41), 80.0);
        assert_eq!(Preset::PercussionKit.frequency_hz(43), 95.0);
        assert_eq!(Preset::PercussionKit.frequency_hz(50), 180.0);
        assert_eq!(Preset::PercussionKit.instrument(36), Instrument::BassDrum);
        assert_eq!(Preset::PercussionKit.instrument(38), Instrument::Snare);
        assert_eq!(Preset::PercussionKit.instrument(42), Instrument::HiHat);
    }
}
