/// A built-in synthesized instrument.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum Instrument {
    /// A sinusoidal oscillator.
    Sine,
    /// A pulse oscillator with equal high and low periods.
    Square,
    /// A symmetric triangle oscillator.
    Triangle,
    /// A low, harmonically soft melodic preset.
    Bass,
    /// A slowly attacking, detuned melodic preset.
    Pad,
    /// A prominent, harmonically bright melodic preset.
    Lead,
    /// A short low tone with a downward pitch sweep.
    BassDrum,
    /// A short pitched drum with a smaller downward sweep.
    Tom,
    /// A short mixture of tone and deterministic noise.
    Snare,
    /// A short bright metallic/noise preset.
    HiHat,
    /// A quickly decaying, harmonically bright plucked-string preset.
    Pluck,
}

impl Instrument {
    /// Return the recommended linear gain for this instrument.
    ///
    /// The values compensate for waveform, spectrum, envelope, and one-shot
    /// duration so the built-in instruments have similar integrated perceived
    /// loudness at their typical pitches. They are calibrated from the EBU R128
    /// loudness of the one-second sections in the `render_wav` example. Multiply
    /// this value by note velocity when constructing custom MIDI mappings or
    /// direct [`crate::Event`]s.
    #[must_use]
    pub const fn default_gain(self) -> f32 {
        match self {
            Self::Sine => 0.25,
            Self::Square => 0.165,
            Self::Triangle => 0.31,
            Self::Pluck => 0.63,
            Self::Bass => 0.55,
            Self::Pad | Self::Tom => 0.65,
            Self::Lead => 0.325,
            Self::BassDrum => 1.0,
            Self::Snare => 0.81,
            Self::HiHat => 0.86,
        }
    }
}
