use crate::ctype::CtypeProfile;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextUnit<'a> {
    Char { ch: char, bytes: &'a [u8] },
    Invalid { bytes: &'a [u8] },
}

impl TextUnit<'_> {
    pub fn is_slash(self) -> bool {
        matches!(self, Self::Char { ch: '/', .. })
    }

    pub fn as_char(self) -> Option<char> {
        match self {
            Self::Char { ch, .. } => Some(ch),
            Self::Invalid { .. } => None,
        }
    }
}

pub fn decode_units<'a>(
    profile: &'a CtypeProfile,
    bytes: &'a [u8],
) -> Box<dyn Iterator<Item = TextUnit<'a>> + 'a> {
    match profile {
        CtypeProfile::ByteC | CtypeProfile::Unknown(_) => Box::new(ByteUnits { bytes, index: 0 }),
        CtypeProfile::Encoded(_) => match profile.encoding() {
            Some(encoding) if encoding == encoding_rs::UTF_8 => {
                Box::new(Utf8Units { bytes, index: 0 })
            }
            Some(encoding) => Box::new(EncodingUnits {
                encoding,
                bytes,
                index: 0,
            }),
            None => Box::new(ByteUnits { bytes, index: 0 }),
        },
    }
}

pub(crate) fn decodes_without_errors(profile: &CtypeProfile, bytes: &[u8]) -> bool {
    decode_units(profile, bytes).all(|unit| unit.as_char().is_some())
}

struct ByteUnits<'a> {
    bytes: &'a [u8],
    index: usize,
}

impl<'a> Iterator for ByteUnits<'a> {
    type Item = TextUnit<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let byte = *self.bytes.get(self.index)?;
        let start = self.index;
        self.index += 1;
        if byte.is_ascii() {
            Some(TextUnit::Char {
                ch: byte as char,
                bytes: &self.bytes[start..self.index],
            })
        } else {
            Some(TextUnit::Invalid {
                bytes: &self.bytes[start..self.index],
            })
        }
    }
}

struct Utf8Units<'a> {
    bytes: &'a [u8],
    index: usize,
}

impl<'a> Iterator for Utf8Units<'a> {
    type Item = TextUnit<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.bytes.len() {
            return None;
        }

        let tail = &self.bytes[self.index..];
        match std::str::from_utf8(tail) {
            Ok(text) => {
                let ch = text.chars().next().unwrap();
                let start = self.index;
                self.index += ch.len_utf8();
                Some(TextUnit::Char {
                    ch,
                    bytes: &self.bytes[start..self.index],
                })
            }
            Err(error) if error.valid_up_to() > 0 => {
                let valid = &tail[..error.valid_up_to()];
                let text = std::str::from_utf8(valid).unwrap();
                let ch = text.chars().next().unwrap();
                let start = self.index;
                self.index += ch.len_utf8();
                Some(TextUnit::Char {
                    ch,
                    bytes: &self.bytes[start..self.index],
                })
            }
            Err(_) => {
                let start = self.index;
                self.index += 1;
                Some(TextUnit::Invalid {
                    bytes: &self.bytes[start..self.index],
                })
            }
        }
    }
}

struct EncodingUnits<'a> {
    encoding: &'static encoding_rs::Encoding,
    bytes: &'a [u8],
    index: usize,
}

impl<'a> Iterator for EncodingUnits<'a> {
    type Item = TextUnit<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.bytes.len() {
            return None;
        }

        let max_end = self.bytes.len().min(self.index + 4);
        for end in self.index + 1..=max_end {
            let slice = &self.bytes[self.index..end];
            let Some(decoded) = self
                .encoding
                .decode_without_bom_handling_and_without_replacement(slice)
            else {
                continue;
            };
            let mut chars = decoded.chars();
            let Some(ch) = chars.next() else {
                continue;
            };
            if chars.next().is_none() {
                let start = self.index;
                self.index = end;
                return Some(TextUnit::Char {
                    ch,
                    bytes: &self.bytes[start..end],
                });
            }
        }

        let start = self.index;
        self.index += 1;
        Some(TextUnit::Invalid {
            bytes: &self.bytes[start..self.index],
        })
    }
}
