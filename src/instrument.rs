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
}
