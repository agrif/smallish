use core::borrow::Borrow;

use nom::{combinator, multi, sequence, Parser};

use crate::de::{TokenError, Tokenizer};

#[derive(Clone, Debug, thiserror::Error)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct EscapedStr<T>(T);

impl<T> EscapedStr<T>
where
    T: Borrow<str>,
{
    pub fn new(s: T) -> Result<Self, TokenError> {
        let s = Self(s);
        s.check().map(|_| s)
    }

    fn check(&self) -> Result<(), TokenError> {
        match combinator::recognize(sequence::terminated(
            multi::many0_count(Tokenizer::string_chunk),
            combinator::eof,
        ))
        .parse(self.0.borrow())
        {
            Ok(("", _)) => Ok(()),
            Ok(_) => Err(TokenError::UnknownToken),
            Err(nom::Err::Incomplete(_)) => Err(TokenError::UnknownToken),
            Err(nom::Err::Error(e) | nom::Err::Failure(e)) => Err(e.error),
        }
    }
}

impl<T> EscapedStr<T> {
    pub fn new_unchecked(s: T) -> Self {
        Self(s)
    }

    pub fn as_escaped(self) -> T {
        self.0
    }
}

impl<T> core::ops::Deref for EscapedStr<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> core::fmt::Display for EscapedStr<T>
where
    T: core::fmt::Display,
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}
