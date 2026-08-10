use crate::Instrument;

/// A built-in MIDI preset.
///
/// Preset IDs are stable session-facing identifiers. Hosts should enumerate
/// [`Preset::available`] instead of hard-coding the set of variants so newly
/// added presets can appear without host changes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum Preset {
    /// A sinusoidal oscillator across the MIDI note range.
    Sine,
    /// A square oscillator across the MIDI note range.
    Square,
    /// A triangle oscillator across the MIDI note range.
    Triangle,
    /// A low melodic instrument across the MIDI note range.
    Bass,
    /// A slowly attacking melodic instrument across the MIDI note range.
    Pad,
    /// A bright melodic instrument across the MIDI note range.
    Lead,
    /// A bass drum on every MIDI key.
    BassDrum,
    /// A tom on every MIDI key.
    Tom,
    /// A snare on every MIDI key.
    Snare,
    /// A hi-hat on every MIDI key.
    HiHat,
    /// A General MIDI-inspired kit containing all percussion instruments.
    PercussionKit,
}

const AVAILABLE: [Preset; 11] = [
    Preset::Sine,
    Preset::Square,
    Preset::Triangle,
    Preset::Bass,
    Preset::Pad,
    Preset::Lead,
    Preset::BassDrum,
    Preset::Tom,
    Preset::Snare,
    Preset::HiHat,
    Preset::PercussionKit,
];

impl Preset {
    /// Return every preset available in this version of the library.
    #[must_use]
    pub const fn available() -> &'static [Self] {
        &AVAILABLE
    }

    /// Return the stable machine-readable ID used in serialized state.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::Sine => "sine",
            Self::Square => "square",
            Self::Triangle => "triangle",
            Self::Bass => "bass",
            Self::Pad => "pad",
            Self::Lead => "lead",
            Self::BassDrum => "bass-drum",
            Self::Tom => "tom",
            Self::Snare => "snare",
            Self::HiHat => "hi-hat",
            Self::PercussionKit => "percussion-kit",
        }
    }

    /// Return a human-readable preset name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Sine => "Sine",
            Self::Square => "Square",
            Self::Triangle => "Triangle",
            Self::Bass => "Bass",
            Self::Pad => "Pad",
            Self::Lead => "Lead",
            Self::BassDrum => "Bass Drum",
            Self::Tom => "Tom",
            Self::Snare => "Snare",
            Self::HiHat => "Hi-Hat",
            Self::PercussionKit => "Percussion Kit",
        }
    }

    /// Find a currently available preset by its stable ID.
    #[must_use]
    pub fn from_id(id: &str) -> Option<Self> {
        Self::available()
            .iter()
            .copied()
            .find(|preset| preset.id() == id)
    }

    /// Return the instrument assigned to a MIDI note by this preset.
    #[must_use]
    pub const fn instrument(self, midi_note: u8) -> Instrument {
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
            Self::PercussionKit => percussion_instrument(midi_note),
        }
    }

    pub(crate) const fn uses_midi_pitch(self) -> bool {
        matches!(
            self,
            Self::Sine | Self::Square | Self::Triangle | Self::Bass | Self::Pad | Self::Lead
        )
    }
}

// General MIDI assigns bass drums to 35/36, snares to 38/40, toms to
// 41/43/45/47/48/50, and hi-hats to 42/44/46. Unsupported keys carry the most
// recent assignment forward; keys below the first assignment use bass drum.
const fn percussion_instrument(note: u8) -> Instrument {
    match note {
        0..=37 => Instrument::BassDrum,
        38..=40 => Instrument::Snare,
        42 | 44 | 46 => Instrument::HiHat,
        41 | 43 | 45 | 47..=u8::MAX => Instrument::Tom,
    }
}

#[cfg(test)]
mod tests {
    use super::Preset;
    use crate::Instrument;

    #[test]
    fn runtime_catalog_has_stable_unique_ids() {
        let presets = Preset::available();
        assert_eq!(presets.len(), 11);
        for (index, preset) in presets.iter().copied().enumerate() {
            assert_eq!(Preset::from_id(preset.id()), Some(preset));
            assert!(!preset.name().is_empty());
            assert!(
                presets[..index]
                    .iter()
                    .all(|other| other.id() != preset.id())
            );
        }
        assert_eq!(Preset::from_id("future-preset"), None);
    }

    #[test]
    fn percussion_kit_uses_general_midi_keys_and_fills_gaps() {
        let kit = Preset::PercussionKit;
        assert_eq!(kit.instrument(35), Instrument::BassDrum);
        assert_eq!(kit.instrument(36), Instrument::BassDrum);
        assert_eq!(kit.instrument(37), Instrument::BassDrum);
        assert_eq!(kit.instrument(38), Instrument::Snare);
        assert_eq!(kit.instrument(39), Instrument::Snare);
        assert_eq!(kit.instrument(40), Instrument::Snare);
        assert_eq!(kit.instrument(41), Instrument::Tom);
        assert_eq!(kit.instrument(42), Instrument::HiHat);
        assert_eq!(kit.instrument(43), Instrument::Tom);
        assert_eq!(kit.instrument(44), Instrument::HiHat);
        assert_eq!(kit.instrument(45), Instrument::Tom);
        assert_eq!(kit.instrument(46), Instrument::HiHat);
        assert_eq!(kit.instrument(50), Instrument::Tom);
        assert_eq!(kit.instrument(127), Instrument::Tom);
    }
}
