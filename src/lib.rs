#![no_std]
#![cfg_attr(docsrs, feature(doc_cfg))]

//! [LICENSE]: https://github.com/agrif/smallish/blob/main/LICENSE
#![doc = include_str!("../README.md")]

#[macro_use]
mod macros;

pub mod de;
pub mod syntax;

pub fn from_str<'de, T>(input: &'de str) -> Result<T, de::Located<'de, de::Error>>
where
    T: serde::de::Deserialize<'de>,
{
    de::Deserializer::<64>::from_str(input).deserialize()
}

pub fn list_from_str<'de, T>(input: &'de str) -> Result<T, de::Located<'de, de::Error>>
where
    T: serde::de::Deserialize<'de>,
{
    de::Deserializer::<64>::list_from_str(input).deserialize()
}

pub fn map_from_str<'de, T>(input: &'de str) -> Result<T, de::Located<'de, de::Error>>
where
    T: serde::de::Deserialize<'de>,
{
    de::Deserializer::<64>::list_from_str(input).deserialize()
}
