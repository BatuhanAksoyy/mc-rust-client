use super::NbtError;

/// A Java string, preserving even unpaired UTF-16 surrogates without replacement.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NbtString(Vec<u16>);

impl NbtString {
    /// Access the original decoded UTF-16 code units.
    #[must_use]
    pub fn as_utf16(&self) -> &[u16] {
        &self.0
    }

    /// Convert to Rust UTF-8; isolated surrogates are an explicit error.
    pub fn to_utf8(&self) -> Result<String, std::string::FromUtf16Error> {
        String::from_utf16(&self.0)
    }

    // Follow DataInput.readUTF's byte grammar, including legacy overlong forms.
    // Charge one allocation unit before each push; encoded bytes are pre-bounded.
    pub(super) fn decode(bytes: &[u8], budget: &mut usize) -> Result<Self, NbtError> {
        let mut units = Vec::new();
        let mut input = bytes.iter().copied();
        while let Some(first) = input.next() {
            let unit = match first {
                0x00..=0x7f => u16::from(first),
                0xc0..=0xdf => (u16::from(first & 0x1f) << 6) | continuation(&mut input)?,
                0xe0..=0xef => {
                    (u16::from(first & 0x0f) << 12)
                        | (continuation(&mut input)? << 6)
                        | continuation(&mut input)?
                }
                _ => return Err(NbtError::InvalidString),
            };
            *budget = budget.checked_sub(1).ok_or(NbtError::ElementLimit)?;
            units.push(unit);
        }
        Ok(Self(units))
    }
}

fn continuation(input: &mut impl Iterator<Item = u8>) -> Result<u16, NbtError> {
    match input.next() {
        Some(byte @ 0x80..=0xbf) => Ok(u16::from(byte & 0x3f)),
        _ => Err(NbtError::InvalidString),
    }
}
