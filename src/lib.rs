#![no_std]
#![cfg_attr(docsrs, feature(doc_cfg))]

//! [LICENSE]: https://github.com/agrif/smallish/blob/main/LICENSE
#![doc = include_str!("../README.md")]

#[macro_use]
mod macros;

mod fmt;

pub mod de;
pub mod syntax;
pub mod types;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Flavor {
    #[default]
    Value,
    List,
    Map,
}

pub fn from_slice<'de, T>(
    flavor: Flavor,
    input: &'de [u8],
) -> Result<T, types::Located<'de, de::Error>>
where
    T: serde::de::Deserialize<'de>,
{
    let mut state = [Default::default(); 64];
    de::Deserializer::new(flavor, input, &mut state, &mut []).deserialize()
}

pub fn from_slice_escaped<'de, T>(
    flavor: Flavor,
    input: &'de [u8],
    unescape: &'de mut [u8],
) -> Result<T, types::Located<'de, de::Error>>
where
    T: serde::de::Deserialize<'de>,
{
    let mut state = [Default::default(); 64];
    de::Deserializer::new(flavor, input, &mut state, unescape).deserialize()
}

pub fn from_str<'de, T>(
    flavor: Flavor,
    input: &'de str,
) -> Result<T, types::Located<'de, de::Error>>
where
    T: serde::de::Deserialize<'de>,
{
    from_slice(flavor, input.as_bytes())
}

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
