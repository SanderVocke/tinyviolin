/// An error while loading serialized processor configuration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StateError {
    /// The data is truncated, malformed, or is not tinyviolin state.
    InvalidData,
    /// The state format version is newer or otherwise unsupported.
    UnsupportedVersion,
    /// The saved fixed MIDI layer capacity differs from this processor.
    IncompatibleMidiLayers,
    /// A saved mapping or effect setting is outside its valid range.
    InvalidConfiguration,
}

impl core::fmt::Display for StateError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidData => "invalid serialized tinyviolin state",
            Self::UnsupportedVersion => "unsupported tinyviolin state version",
            Self::IncompatibleMidiLayers => "serialized state has a different MIDI layer capacity",
            Self::InvalidConfiguration => "serialized state contains invalid configuration",
        })
    }
}

impl std::error::Error for StateError {}

pub(crate) struct Reader<'a> {
    bytes: &'a [u8],
    cursor: usize,
}

impl<'a> Reader<'a> {
    pub(crate) const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, cursor: 0 }
    }

    pub(crate) fn read_exact(&mut self, len: usize) -> Result<&'a [u8], StateError> {
        let end = self
            .cursor
            .checked_add(len)
            .ok_or(StateError::InvalidData)?;
        let value = self
            .bytes
            .get(self.cursor..end)
            .ok_or(StateError::InvalidData)?;
        self.cursor = end;
        Ok(value)
    }

    pub(crate) fn u8(&mut self) -> Result<u8, StateError> {
        Ok(self.read_exact(1)?[0])
    }

    pub(crate) fn u16(&mut self) -> Result<u16, StateError> {
        let bytes = self.read_exact(2)?;
        Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    pub(crate) fn u32(&mut self) -> Result<u32, StateError> {
        let bytes = self.read_exact(4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    pub(crate) fn f32(&mut self) -> Result<f32, StateError> {
        Ok(f32::from_bits(self.u32()?))
    }

    pub(crate) const fn is_finished(&self) -> bool {
        self.cursor == self.bytes.len()
    }
}

pub(crate) fn push_u16(output: &mut Vec<u8>, value: u16) {
    output.extend_from_slice(&value.to_le_bytes());
}

pub(crate) fn push_u32(output: &mut Vec<u8>, value: u32) {
    output.extend_from_slice(&value.to_le_bytes());
}

pub(crate) fn push_f32(output: &mut Vec<u8>, value: f32) {
    push_u32(output, value.to_bits());
}
