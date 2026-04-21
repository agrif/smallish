use serde::de;

use crate::de::ParseError;
use crate::fmt::FormatIter;
use crate::syntax::{Float, Integer};
use crate::types::{Located, UnescapeError};

#[derive(Clone, Debug, PartialEq, thiserror::Error)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Error {
    #[error("parse error: {0}")]
    Parse(#[from] ParseError),
    #[error("unused input at end")]
    UnusedInput,
    #[error("integer out of range: {0}")]
    IntegerRange(Integer),
    #[error("float out of range: {0}")]
    FloatRange(Float),
    #[error("unescape buffer full")]
    BufferFull,

    #[cfg(feature = "custom-error-messages")]
    #[error("{0}")]
    Custom(heapless::String<64>),

    #[cfg(not(feature = "custom-error-messages"))]
    #[error("serde error")]
    Custom,

    #[error("invalid type")]
    InvalidType,
    #[error("invalid value")]
    InvalidValue,
    #[error("invalid length: {0}")]
    InvalidLength(usize),
    #[error("unknown variant: expected one of {choices}", choices=FormatIter::new(.0.iter(), ", "))]
    UnknownVariant(&'static [&'static str]),
    #[error("unknown field: expected one of {choices}", choices=FormatIter::new(.0.iter(), ", "))]
    UnknownField(&'static [&'static str]),
    #[error("missing field: {0}")]
    MissingField(&'static str),
    #[error("duplicate field: {0}")]
    DuplicateField(&'static str),
}

impl<'de> From<Located<'de, ParseError>> for Located<'de, Error> {
    fn from(other: Located<'de, ParseError>) -> Self {
        other.map(Into::into)
    }
}

impl From<UnescapeError> for Error {
    fn from(other: UnescapeError) -> Self {
        match other {
            UnescapeError::BadLiteral(e) => Error::Parse(e.into()),
            UnescapeError::BufferFull => Error::BufferFull,
        }
    }
}

impl de::Error for Error {
    #[cfg(feature = "custom-error-messages")]
    fn custom<T>(msg: T) -> Self
    where
        T: core::fmt::Display,
    {
        use core::fmt::Write;
        let mut s = heapless::String::new();
        if write!(&mut s, "{}", msg).is_err() {
            s.clear();
            let _ = s.push_str("<too large for buffer>");
        }
        Self::Custom(s)
    }

    #[cfg(not(feature = "custom-error-messages"))]
    fn custom<T>(_msg: T) -> Self
    where
        T: core::fmt::Display,
    {
        Self::Custom
    }

    fn invalid_type(_unexp: de::Unexpected<'_>, _exp: &dyn de::Expected) -> Self {
        Self::InvalidType
    }

    fn invalid_value(_unexp: de::Unexpected<'_>, _exp: &dyn de::Expected) -> Self {
        Self::InvalidValue
    }

    fn invalid_length(len: usize, _exp: &dyn de::Expected) -> Self {
        Self::InvalidLength(len)
    }

    fn unknown_variant(_variant: &str, expected: &'static [&'static str]) -> Self {
        Self::UnknownVariant(expected)
    }

    fn unknown_field(_field: &str, expected: &'static [&'static str]) -> Self {
        Self::UnknownField(expected)
    }

    fn missing_field(field: &'static str) -> Self {
        Self::MissingField(field)
    }

    fn duplicate_field(field: &'static str) -> Self {
        Self::DuplicateField(field)
    }
}
