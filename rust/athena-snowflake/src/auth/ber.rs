const CONSTRUCTED: u8 = 0x20;
const HIGH_TAG_NUMBER: u8 = 0x1f;
const MORE_TAG_OCTETS: u8 = 0x80;
const LONG_LENGTH: u8 = 0x80;
const RESERVED_LENGTH: u8 = 0xff;
const OCTET_STRING: &[u8] = &[0x04];
const CONSTRUCTED_OCTET_STRING: &[u8] = &[0x24];
const END_OF_CONTENTS: &[u8] = &[0, 0];
const MAX_LENGTH_OCTETS: usize = 4;
const MAX_TAG_OCTETS: usize = 5;
const MAX_DEPTH: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum BerError {
    #[error("corrupted stream - out of bounds length found")]
    Truncated,
    #[error("DER length more than 4 bytes")]
    LengthTooLong,
    #[error("corrupted stream - negative length found")]
    NegativeLength,
    #[error("invalid length encoding")]
    ReservedLength,
    #[error("indefinite-length primitive encoding encountered")]
    IndefinitePrimitive,
    #[error("unknown object in constructed OCTET STRING")]
    ConstructedOctetString,
    #[error("tag number too large")]
    TagTooLong,
    #[error("nesting deeper than {MAX_DEPTH} levels")]
    TooDeep,
    #[error("Extra data detected in stream")]
    TrailingData,
}

struct Element<'a> {
    tag: &'a [u8],
    content: Vec<u8>,
}

impl Element<'_> {
    fn encode_into(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(self.tag);
        let len = self.content.len();
        if let Ok(short) = u8::try_from(len) {
            if short < LONG_LENGTH {
                out.push(short);
                out.extend_from_slice(&self.content);
                return;
            }
        }
        let bytes = len.to_be_bytes();
        let start = bytes
            .iter()
            .position(|byte| *byte != 0)
            .unwrap_or(bytes.len());
        let octets = &bytes[start..];
        #[allow(
            clippy::cast_possible_truncation,
            reason = "a usize has at most 16 length octets"
        )]
        out.push(LONG_LENGTH | octets.len() as u8);
        out.extend_from_slice(octets);
        out.extend_from_slice(&self.content);
    }
}

struct Reader<'a> {
    input: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn is_finished(&self) -> bool {
        self.pos == self.input.len()
    }

    fn take(&mut self, len: usize) -> Result<&'a [u8], BerError> {
        let end = self.pos.checked_add(len).ok_or(BerError::Truncated)?;
        let bytes = self.input.get(self.pos..end).ok_or(BerError::Truncated)?;
        self.pos = end;
        Ok(bytes)
    }

    fn byte(&mut self) -> Result<u8, BerError> {
        Ok(self.take(1)?[0])
    }

    fn tag(&mut self) -> Result<&'a [u8], BerError> {
        let start = self.pos;
        if self.byte()? & HIGH_TAG_NUMBER == HIGH_TAG_NUMBER {
            while self.byte()? & MORE_TAG_OCTETS != 0 {
                if self.pos - start >= MAX_TAG_OCTETS {
                    return Err(BerError::TagTooLong);
                }
            }
        }
        Ok(&self.input[start..self.pos])
    }

    fn length(&mut self) -> Result<Option<usize>, BerError> {
        let first = self.byte()?;
        if first < LONG_LENGTH {
            return Ok(Some(usize::from(first)));
        }
        if first == LONG_LENGTH {
            return Ok(None);
        }
        if first == RESERVED_LENGTH {
            return Err(BerError::ReservedLength);
        }
        let octets = usize::from(first & !LONG_LENGTH);
        if octets > MAX_LENGTH_OCTETS {
            return Err(BerError::LengthTooLong);
        }
        let mut word = [0u8; MAX_LENGTH_OCTETS];
        word[MAX_LENGTH_OCTETS - octets..].copy_from_slice(self.take(octets)?);
        let len = i32::from_be_bytes(word);
        usize::try_from(len)
            .map(Some)
            .map_err(|_| BerError::NegativeLength)
    }

    fn at_end_of_contents(&mut self) -> Result<bool, BerError> {
        if self.input.len() - self.pos < END_OF_CONTENTS.len() {
            return Err(BerError::Truncated);
        }
        if self.input[self.pos..].starts_with(END_OF_CONTENTS) {
            self.pos += END_OF_CONTENTS.len();
            return Ok(true);
        }
        Ok(false)
    }

    fn element(&mut self, depth: usize) -> Result<Element<'a>, BerError> {
        if depth > MAX_DEPTH {
            return Err(BerError::TooDeep);
        }
        let tag = self.tag()?;
        let length = self.length()?;
        if tag[0] & CONSTRUCTED == 0 {
            let len = length.ok_or(BerError::IndefinitePrimitive)?;
            return Ok(Element {
                tag,
                content: self.take(len)?.to_vec(),
            });
        }
        let mut children = Vec::new();
        match length {
            Some(len) => {
                let mut inner = Reader {
                    input: self.take(len)?,
                    pos: 0,
                };
                while !inner.is_finished() {
                    children.push(inner.element(depth + 1)?);
                }
            }
            None => {
                while !self.at_end_of_contents()? {
                    children.push(self.element(depth + 1)?);
                }
            }
        }
        let mut content = Vec::new();
        if tag == CONSTRUCTED_OCTET_STRING {
            for child in children {
                if child.tag != OCTET_STRING {
                    return Err(BerError::ConstructedOctetString);
                }
                content.extend_from_slice(&child.content);
            }
            return Ok(Element {
                tag: OCTET_STRING,
                content,
            });
        }
        for child in &children {
            child.encode_into(&mut content);
        }
        Ok(Element { tag, content })
    }
}

pub(crate) fn to_der(input: &[u8]) -> Result<Vec<u8>, BerError> {
    let mut reader = Reader { input, pos: 0 };
    let element = reader.element(0)?;
    if !reader.is_finished() {
        return Err(BerError::TrailingData);
    }
    let mut out = Vec::with_capacity(input.len());
    element.encode_into(&mut out);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn der_input_is_unchanged() {
        let der = [0x30, 0x06, 0x02, 0x01, 0x05, 0x04, 0x01, 0xaa];
        assert_eq!(der.to_vec(), to_der(&der).unwrap());
    }

    #[test]
    fn indefinite_lengths_become_definite() {
        let ber = [
            0x30, 0x80, 0x30, 0x80, 0x02, 0x01, 0x05, 0x00, 0x00, 0x04, 0x01, 0xaa, 0x00, 0x00,
        ];
        assert_eq!(
            vec![0x30, 0x08, 0x30, 0x03, 0x02, 0x01, 0x05, 0x04, 0x01, 0xaa],
            to_der(&ber).unwrap()
        );
    }

    #[test]
    fn long_form_lengths_become_minimal() {
        let ber = [0x30, 0x84, 0, 0, 0, 0x04, 0x04, 0x81, 0x01, 0xaa];
        assert_eq!(vec![0x30, 0x03, 0x04, 0x01, 0xaa], to_der(&ber).unwrap());
    }

    #[test]
    fn constructed_octet_strings_are_joined() {
        let ber = [
            0x30, 0x80, 0x24, 0x80, 0x04, 0x01, 0xaa, 0x24, 0x03, 0x04, 0x01, 0xbb, 0x00, 0x00,
            0x00, 0x00,
        ];
        assert_eq!(
            vec![0x30, 0x04, 0x04, 0x02, 0xaa, 0xbb],
            to_der(&ber).unwrap()
        );
    }

    #[test]
    fn long_contents_get_long_form_lengths() {
        let mut ber = vec![0x30, 0x80, 0x04, 0x81, 0xc8];
        ber.extend([7u8; 200]);
        ber.extend([0, 0]);
        let mut der = vec![0x30, 0x81, 0xcb, 0x04, 0x81, 0xc8];
        der.extend([7u8; 200]);
        assert_eq!(der, to_der(&ber).unwrap());
    }

    #[test]
    fn malformed_encodings_are_rejected() {
        assert_eq!(Err(BerError::Truncated), to_der(&[0x30, 0x80, 0x05, 0x00]));
        assert_eq!(
            Err(BerError::TrailingData),
            to_der(&[0x30, 0x80, 0x05, 0x00, 0x00, 0x00, 0x00])
        );
        assert_eq!(
            Err(BerError::IndefinitePrimitive),
            to_der(&[0x04, 0x80, 0x01, 0x00, 0x00])
        );
        assert_eq!(
            Err(BerError::LengthTooLong),
            to_der(&[0x04, 0x85, 0, 0, 0, 0, 1, 0xaa])
        );
        assert_eq!(
            Err(BerError::NegativeLength),
            to_der(&[0x04, 0x84, 0xff, 0xff, 0xff, 0xff])
        );
        assert_eq!(Err(BerError::ReservedLength), to_der(&[0x04, 0xff]));
        assert_eq!(Err(BerError::Truncated), to_der(&[0x30, 0x05, 0x05, 0x00]));
        assert_eq!(
            Err(BerError::ConstructedOctetString),
            to_der(&[0x24, 0x02, 0x05, 0x00])
        );
        let mut deep = Vec::new();
        for _ in 0..=MAX_DEPTH + 1 {
            deep.extend([0x30, 0x80]);
        }
        assert_eq!(Err(BerError::TooDeep), to_der(&deep));
    }
}
