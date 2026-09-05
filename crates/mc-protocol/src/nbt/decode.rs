use super::{Limits, MAX_DEPTH, NamedTag, NbtError, NbtString, Tag};
use crate::{CodecError, types::Reader};

/// Decode one unnamed network root, including its type byte and consumed length.
/// Trailing bytes are left for the enclosing packet decoder.
pub fn decode_network(input: &[u8], limits: Limits) -> Result<(Tag<'_>, usize), NbtError> {
    let mut decoder = Decoder::new(input, limits)?;
    let id = decoder.id()?;
    let value = decoder.payload(id, 0)?;
    Ok((value, decoder.consumed))
}

/// Decode one named, uncompressed root. End has an empty name and no name bytes.
/// This does not enforce the compound-root convention or decompress disk files.
pub fn decode_named(input: &[u8], limits: Limits) -> Result<(NamedTag<'_>, usize), NbtError> {
    let mut decoder = Decoder::new(input, limits)?;
    let id = decoder.id()?;
    let name = if id == 0 { NbtString::default() } else { decoder.string()? };
    let tag = decoder.payload(id, 0)?;
    Ok((NamedTag { name, tag }, decoder.consumed))
}

struct Decoder<'a> {
    reader: Reader<'a>,
    limits: Limits,
    consumed: usize,
    elements_left: usize,
}

impl<'a> Decoder<'a> {
    const fn new(input: &'a [u8], limits: Limits) -> Result<Self, NbtError> {
        if limits.max_depth > MAX_DEPTH {
            return Err(NbtError::DepthLimit);
        }
        Ok(Self {
            reader: Reader::new(input),
            limits,
            consumed: 0,
            elements_left: limits.max_elements,
        })
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], NbtError> {
        if count > self.limits.max_bytes - self.consumed {
            return Err(NbtError::ByteLimit);
        }
        let bytes = self.reader.take(count)?;
        self.consumed += count;
        Ok(bytes)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], NbtError> {
        self.take(N)?.try_into().map_err(|_| CodecError::UnexpectedEof.into())
    }

    fn id(&mut self) -> Result<u8, NbtError> {
        let id = self.take(1)?[0];
        if id > 12 { Err(NbtError::InvalidTag(id)) } else { Ok(id) }
    }

    fn charge(&mut self, count: usize) -> Result<(), NbtError> {
        self.elements_left = self.elements_left.checked_sub(count).ok_or(NbtError::ElementLimit)?;
        Ok(())
    }

    fn length(&mut self) -> Result<usize, NbtError> {
        usize::try_from(i32::from_be_bytes(self.array()?))
            .map_err(|_| CodecError::InvalidLength.into())
    }

    fn string(&mut self) -> Result<NbtString, NbtError> {
        let length = usize::from(u16::from_be_bytes(self.array()?));
        let bytes = self.take(length)?;
        NbtString::decode(bytes, &mut self.elements_left)
    }

    fn numeric_array<const N: usize, T>(
        &mut self,
        convert: impl Fn([u8; N]) -> T,
    ) -> Result<Vec<T>, NbtError> {
        let count = self.length()?;
        self.charge(count)?;
        let length = count.checked_mul(N).ok_or(NbtError::ByteLimit)?;
        let bytes = self.take(length)?;
        Ok(bytes.as_chunks::<N>().0.iter().copied().map(convert).collect())
    }

    fn payload(&mut self, id: u8, depth: usize) -> Result<Tag<'a>, NbtError> {
        if depth > self.limits.max_depth {
            return Err(NbtError::DepthLimit);
        }
        self.charge(1)?;
        Ok(match id {
            0 => Tag::End,
            1 => Tag::Byte(i8::from_be_bytes(self.array()?)),
            2 => Tag::Short(i16::from_be_bytes(self.array()?)),
            3 => Tag::Int(i32::from_be_bytes(self.array()?)),
            4 => Tag::Long(i64::from_be_bytes(self.array()?)),
            5 => Tag::Float(f32::from_be_bytes(self.array()?)),
            6 => Tag::Double(f64::from_be_bytes(self.array()?)),
            7 => {
                let length = self.length()?;
                Tag::ByteArray(self.take(length)?)
            }
            8 => Tag::String(self.string()?),
            9 => self.list(depth)?,
            10 => self.compound(depth)?,
            11 => Tag::IntArray(self.numeric_array(i32::from_be_bytes)?),
            12 => Tag::LongArray(self.numeric_array(i64::from_be_bytes)?),
            other => return Err(NbtError::InvalidTag(other)),
        })
    }

    fn list(&mut self, depth: usize) -> Result<Tag<'a>, NbtError> {
        let element_id = self.id()?;
        let count = self.length()?;
        if element_id == 0 && count != 0 {
            return Err(NbtError::EndList);
        }
        if count > self.elements_left {
            return Err(NbtError::ElementLimit);
        }
        let mut elements = Vec::new();
        for _ in 0..count {
            elements.push(self.payload(element_id, depth + 1)?);
        }
        Ok(Tag::List { element_id, elements })
    }

    fn compound(&mut self, depth: usize) -> Result<Tag<'a>, NbtError> {
        let mut entries = Vec::new();
        loop {
            let id = self.id()?;
            if id == 0 {
                break;
            }
            // Check child budgets before allocating its name.
            if depth == self.limits.max_depth {
                return Err(NbtError::DepthLimit);
            }
            if self.elements_left == 0 {
                return Err(NbtError::ElementLimit);
            }
            let name = self.string()?;
            let tag = self.payload(id, depth + 1)?;
            entries.push(NamedTag { name, tag });
        }
        Ok(Tag::Compound(entries))
    }
}
