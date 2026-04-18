use core::borrow::Borrow;

use nom::{combinator, multi, Parser};

use crate::de::{token::SliceChunk, TokenError, Tokenizer};

#[derive(Clone, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Escaped<T>(T);

#[derive(Clone, Debug, thiserror::Error)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum UnescapeError {
    #[error("bad unescaped literal")]
    BadLiteral(#[from] TokenError),
    #[error("unescape buffer full")]
    BufferFull,
}

impl<T> Escaped<T>
where
    T: Borrow<str>,
{
    pub fn new(s: T) -> Result<Self, TokenError> {
        let s = Self(s);
        s.check().map(|_| s)
    }

    fn check(&self) -> Result<(), TokenError> {
        match combinator::recognize(multi::many0_count(Tokenizer::string_chunk))
            .parse(self.0.borrow())
        {
            Ok(("", _)) => Ok(()),
            Ok(_) => Err(TokenError::UnknownToken),
            Err(nom::Err::Incomplete(_)) => Err(TokenError::UnknownToken),
            Err(nom::Err::Error(e) | nom::Err::Failure(e)) => Err(e.error),
        }
    }

    pub fn has_escapes(&self) -> bool {
        !matches!(
            Tokenizer::string_chunk(self.0.borrow()),
            // if there is a single slice chunk, it has no escapes
            Ok(("", SliceChunk::Slice(_))),
        )
    }

    pub fn unescape<'a>(
        &self,
        buffer: &'a mut [u8],
    ) -> Result<(&'a mut [u8], &'a str), UnescapeError> {
        let mut input = self.0.borrow();
        let mut i = 0;
        while !input.is_empty() {
            match Tokenizer::string_chunk(input) {
                Ok((rest, chunk)) => {
                    assert!(rest.len() < input.len());
                    input = rest;
                    match chunk {
                        SliceChunk::Slice(s) => {
                            let bytes = s.as_bytes();
                            let amt = bytes.len();
                            buffer
                                .get_mut(i..i + amt)
                                .ok_or(UnescapeError::BufferFull)?
                                .copy_from_slice(bytes);
                            i += amt;
                        }
                        SliceChunk::Item(c) => {
                            let amt = c.len_utf8();
                            c.encode_utf8(
                                buffer
                                    .get_mut(i..i + amt)
                                    .ok_or(UnescapeError::BufferFull)?,
                            );
                            i += amt;
                        }
                    }
                }
                // can only be caused by a bad use of new_unchecked
                Err(nom::Err::Incomplete(_)) => Err(TokenError::UnknownToken)?,
                Err(nom::Err::Error(e) | nom::Err::Failure(e)) => Err(e.error)?,
            }
        }

        let (result, unused) = buffer.split_at_mut(i);

        // safety: we just produced this directly from valid utf-8 &str
        // and raw characters
        let result = unsafe { core::str::from_utf8_unchecked(result) };

        Ok((unused, result))
    }
}

impl<T> Escaped<T> {
    pub fn new_unchecked(s: T) -> Self {
        Self(s)
    }

    pub fn as_escaped(self) -> T {
        self.0
    }
}

impl<T> core::ops::Deref for Escaped<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> core::fmt::Display for Escaped<T>
where
    T: core::fmt::Display,
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}
