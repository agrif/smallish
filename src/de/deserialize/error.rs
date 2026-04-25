use serde::de;

use crate::de::ParseError;
use crate::fmt::FormatIter;
use crate::syntax::{Float, Integer};
use crate::types::UnescapeError;

/// Errors produced by [Deserializer](super::Deserializer).
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Error {
    /// The parser found an error.
    #[error("parse error: {0}")]
    Parse(#[from] ParseError),
    /// There was unconsumed data at the end of the input.
    #[error("unused input at end")]
    UnusedInput,
    /// An integer did not fit into the deserialized type.
    #[error("integer out of range: {0}")]
    IntegerRange(Integer),
    /// A float did not fit into the deserialized type.
    #[error("float out of range: {0}")]
    FloatRange(Float),
    /// There is not enough room in the unescape buffer.
    #[error("unescape buffer full")]
    UnescapeBufferFull,

    /// Custom `serde` error (via [serde::de::Error::custom]).
    ///
    /// If the `custom-error-messages` feature is enabled, this
    /// variant carries a small buffer to store the
    /// message. Otherwise, the message is lost.
    #[cfg(feature = "custom-error-messages")]
    #[error("{0}")]
    Custom(heapless::String<64>),

    /// Custom `serde` error (via [serde::de::Error::custom]).
    ///
    /// If the `custom-error-messages` feature is enabled, this
    /// variant carries a small buffer to store the
    /// message. Otherwise, the message is lost.
    #[cfg(not(feature = "custom-error-messages"))]
    #[error("serde error")]
    Custom,

    /// This value has the wrong type.
    #[error("invalid type")]
    InvalidType,
    /// This value has the wrong... value.
    #[error("invalid value")]
    InvalidValue,
    /// This value has the wrong length. Holds the length *found*.
    #[error("invalid length: {0}")]
    InvalidLength(usize),
    /// Found a variant name that should be in this list but isn't.
    #[error("unknown variant: expected one of {choices}", choices=FormatIter::new(.0.iter(), ", "))]
    UnknownVariant(&'static [&'static str]),
    /// Found a field name that should be in this list but isn't.
    #[error("unknown field: expected one of {choices}", choices=FormatIter::new(.0.iter(), ", "))]
    UnknownField(&'static [&'static str]),
    /// This field is required but not present.
    #[error("missing field: {0}")]
    MissingField(&'static str),
    /// This field is repeated more than once.
    #[error("duplicate field: {0}")]
    DuplicateField(&'static str),
}

impl From<UnescapeError> for Error {
    fn from(other: UnescapeError) -> Self {
        match other {
            UnescapeError::UnknownEscape => Error::Parse(ParseError::UnknownEscape),
            UnescapeError::BufferFull => Error::UnescapeBufferFull,
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

#[cfg(test)]
mod test {
    #[test]
    fn invalid_value() {
        use crate::{de::Error, from_slice_escaped, Flavor};
        use core::num::NonZero;
        let r = from_slice_escaped::<NonZero<u8>>(Flavor::Value, b" 0 ", &mut []).unwrap_err();
        assert_eq!(Error::InvalidValue, *r)
    }

    #[test]
    fn invalid_length() {
        use crate::{de::Error, from_slice_escaped, Flavor};
        let r = from_slice_escaped::<(u8, u8)>(Flavor::Value, b" [0] ", &mut []).unwrap_err();
        assert_eq!(Error::InvalidLength(1), *r)
    }

    #[derive(Debug, PartialEq, Eq, serde::Deserialize)]
    enum Enum {
        Variant,
    }
    #[test]
    fn unknown_variant() {
        use crate::{de::Error, from_slice_escaped, Flavor};
        let r = from_slice_escaped::<Enum>(Flavor::Value, b" No ", &mut []).unwrap_err();
        assert_eq!(Error::UnknownVariant(&["Variant"]), *r)
    }

    #[derive(Debug, PartialEq, Eq, serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Struct {
        a: u8,
        b: u8,
    }
    #[test]
    fn unknown_field() {
        use crate::{de::Error, from_slice_escaped, Flavor};
        let r = from_slice_escaped::<Struct>(Flavor::Value, b" {foo=2} ", &mut []).unwrap_err();
        assert_eq!(Error::UnknownField(&["a", "b"]), *r)
    }
    #[test]
    fn missing_field() {
        use crate::{de::Error, from_slice_escaped, Flavor};
        let r = from_slice_escaped::<Struct>(Flavor::Value, b" {a=2} ", &mut []).unwrap_err();
        assert_eq!(Error::MissingField("b"), *r)
    }
    #[test]
    fn duplicate_field() {
        use crate::{de::Error, from_slice_escaped, Flavor};
        let r = from_slice_escaped::<Struct>(Flavor::Value, b" {a=2, a=3} ", &mut []).unwrap_err();
        assert_eq!(Error::DuplicateField("a"), *r)
    }
}
