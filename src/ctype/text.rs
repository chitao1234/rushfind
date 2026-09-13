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
        let start = self.index;
        let first = *self.bytes.get(start)?;

        // Validate only the sequence at this offset; checking the whole tail
        // per character made decoding a candidate quadratic.
        let end = (start + utf8_sequence_len(first)).min(self.bytes.len());
        let decoded = std::str::from_utf8(&self.bytes[start..end])
            .ok()
            .and_then(|text| text.chars().next())
            .filter(|ch| ch.len_utf8() == end - start);

        if let Some(ch) = decoded {
            self.index = end;
            return Some(TextUnit::Char {
                ch,
                bytes: &self.bytes[start..end],
            });
        }

        self.index = start + 1;
        Some(TextUnit::Invalid {
            bytes: &self.bytes[start..start + 1],
        })
    }
}

/// Sequence length for this lead byte; anything that cannot lead a sequence
/// reports one byte, so the caller records a single invalid unit.
fn utf8_sequence_len(lead: u8) -> usize {
    match lead {
        0x00..=0x7f => 1,
        0xc0..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf7 => 4,
        _ => 1,
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

#[cfg(test)]
mod tests {
    use super::{TextUnit, decode_units};
    use crate::ctype::{CtypeProfile, resolve_ctype_profile_from};

    fn shape(profile: &CtypeProfile, bytes: &[u8]) -> Vec<String> {
        decode_units(profile, bytes)
            .map(|unit| match unit {
                TextUnit::Char { ch, .. } => ch.to_string(),
                TextUnit::Invalid { bytes } => format!("invalid({})", bytes.len()),
            })
            .collect()
    }

    /// The shapes that validating per-offset rather than per-tail can get wrong.
    #[test]
    fn utf8_decoding_splits_valid_and_invalid_sequences_the_same_way() {
        let profile = resolve_ctype_profile_from([("LC_CTYPE", "en_US.UTF-8")]);
        assert!(!profile.is_byte_c(), "the test needs an encoded profile");

        for (bytes, expected) in [
            (&b"ab"[..], vec!["a", "b"]),
            ("\u{e9}".as_bytes(), vec!["\u{e9}"]),
            ("\u{65e5}".as_bytes(), vec!["\u{65e5}"]),
            ("\u{1f600}".as_bytes(), vec!["\u{1f600}"]),
            ("a\u{e9}b".as_bytes(), vec!["a", "\u{e9}", "b"]),
            // Truncated three-byte sequence: the lead and the stray
            // continuation byte are each invalid on their own.
            (&[0xe6, 0x97][..], vec!["invalid(1)", "invalid(1)"]),
            // Overlong encoding and a surrogate are both rejected.
            (&[0xc0, 0x80][..], vec!["invalid(1)", "invalid(1)"]),
            (
                &[0xed, 0xa0, 0x80][..],
                vec!["invalid(1)", "invalid(1)", "invalid(1)"],
            ),
            (&[0x41, 0xff, 0x42][..], vec!["A", "invalid(1)", "B"]),
            (&[0xf8, 0x88][..], vec!["invalid(1)", "invalid(1)"]),
        ] {
            assert_eq!(shape(&profile, bytes), expected, "{bytes:02x?}");
        }
    }
}
