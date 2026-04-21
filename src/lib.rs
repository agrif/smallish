#![no_std]
#![cfg_attr(docsrs, feature(doc_cfg))]
// fix up links in README
//! [LICENSE]: https://github.com/agrif/smallish/blob/main/LICENSE
//! [from_str]: from_str
//! [from_slice]: from_slice
//! [Deserializer]: de::Deserializer
//! [syntax]: syntax
//! [Flavor]: Flavor
//! [from_str_escaped]: from_str_escaped
//! [from_slice_escaped]: from_slice_escaped
//! [Escaped]: types::Escaped
//! [Located]: types::Located
#![doc = include_str!("../README.md")]

#[macro_use]
mod macros;

mod fmt;

pub mod de;
pub mod syntax;
pub mod types;

/// Flavor decides what type of value is assumed at the root of the document.
///
/// For example, [List](Flavor::List) flavored *smallish* consists of
/// the body of a list, without the enclosing braces.
///
/// ```
/// # use smallish::{Flavor, from_str};
/// let a: Vec<u8> = from_str(Flavor::Value, "[1, 2, 3]").unwrap();
/// let b: Vec<u8> = from_str(Flavor::List, "1, 2, 3").unwrap();
/// assert_eq!(a, b);
/// ```
///
/// The main purpose of this is to allow writing lists of enumerations
/// as a plain, newline-separated list of values by using the
/// [List](Flavor::List) flavor.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Flavor {
    /// Any value is expected at the root. Braces are required.
    #[default]
    Value,
    /// A list (without braces!) is expected at the root.
    List,
    /// A map (without braces!) is expected at the root.
    Map,
}

// The default state size used in the convenience functions.
// This is a macro so the docs can re-use it and stay in sync.
// This might be overkill.
macro_rules! default_state_size {
    () => {
        64
    };
}

/// Deserialize a value from a slice of bytes.
///
/// This is a convenience function to deserialize a value with some
/// sensible defaults. In particular, this uses a fixed recursion
/// depth of
#[doc = concat!(default_state_size!(), ".")]
/// If you are getting a
/// [MaxRecursion](de::ParseError::MaxRecursion) error, please use
/// [Deserializer](de::Deserializer) directly.
///
/// This function will return [BufferFull](de::Error::BufferFull) if
/// the input contains escaped strings or escaped bytestrings. Use
/// [from_slice_escaped] instead, or opt-out of unescaping with
/// [Escaped](types::Escaped).
///
/// For deserializing from strings, see [from_str].
pub fn from_slice<'de, T>(
    flavor: Flavor,
    input: &'de [u8],
) -> Result<T, types::Located<'de, de::Error>>
where
    T: serde::de::Deserialize<'de>,
{
    let mut state = [Default::default(); default_state_size!()];
    de::Deserializer::new(flavor, input, &mut state, &mut []).deserialize()
}

/// Deserialize a value from a slice of bytes, and handle escaped strings.
///
/// This is a convenience function to deserialize a value with some
/// sensible defaults. In particular, this uses a fixed recursion
/// depth of
#[doc = concat!(default_state_size!(), ".")]
/// If you are getting a
/// [MaxRecursion](de::ParseError::MaxRecursion) error, please use
/// [Deserializer](de::Deserializer) directly.
///
/// This uses `unescape` as a scratch buffer to remove escapes from
/// strings and bytestrings. If this buffer runs out of space, this
/// function returns [BufferFull](de::Error::BufferFull). Consider
/// using a larger buffer, or using [Escaped](types::Escaped).
///
/// For deserializing from strings, see [from_str_escaped].
pub fn from_slice_escaped<'de, T>(
    flavor: Flavor,
    input: &'de [u8],
    unescape: &'de mut [u8],
) -> Result<T, types::Located<'de, de::Error>>
where
    T: serde::de::Deserialize<'de>,
{
    let mut state = [Default::default(); default_state_size!()];
    de::Deserializer::new(flavor, input, &mut state, unescape).deserialize()
}

/// Deserialize a value from a string.
///
/// This is a convenience function to deserialize a value with some
/// sensible defaults. In particular, this uses a fixed recursion
/// depth of
#[doc = concat!(default_state_size!(), ".")]
/// If you are getting a
/// [MaxRecursion](de::ParseError::MaxRecursion) error, please use
/// [Deserializer](de::Deserializer) directly.
///
/// This function will return [BufferFull](de::Error::BufferFull) if
/// the input contains escaped strings or escaped bytestrings. Use
/// [from_str_escaped] instead, or opt-out of unescaping with
/// [Escaped](types::Escaped).
///
/// For deserializing from bytes, see [from_slice].
pub fn from_str<'de, T>(
    flavor: Flavor,
    input: &'de str,
) -> Result<T, types::Located<'de, de::Error>>
where
    T: serde::de::Deserialize<'de>,
{
    from_slice(flavor, input.as_bytes())
}

/// Deserialize a value from a string, and handle escaped strings.
///
/// This is a convenience function to deserialize a value with some
/// sensible defaults. In particular, this uses a fixed recursion
/// depth of
#[doc = concat!(default_state_size!(), ".")]
/// If you are getting a
/// [MaxRecursion](de::ParseError::MaxRecursion) error, please use
/// [Deserializer](de::Deserializer) directly.
///
/// This uses `unescape` as a scratch buffer to remove escapes from
/// strings and bytestrings. If this buffer runs out of space, this
/// function returns [BufferFull](de::Error::BufferFull). Consider
/// using a larger buffer, or using [Escaped](types::Escaped).
///
/// For deserializing from bytes, see [from_slice_escaped].
pub fn from_str_escaped<'de, T>(
    flavor: Flavor,
    input: &'de str,
    unescape: &'de mut [u8],
) -> Result<T, types::Located<'de, de::Error>>
where
    T: serde::de::Deserialize<'de>,
{
    from_slice_escaped(flavor, input.as_bytes(), unescape)
}
