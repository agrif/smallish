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
    let mut state = [Default::default(); 64];
    de::Deserializer::from_str(input, &mut state).deserialize()
}

pub fn list_from_str<'de, T>(input: &'de str) -> Result<T, de::Located<'de, de::Error>>
where
    T: serde::de::Deserialize<'de>,
{
    let mut state = [Default::default(); 64];
    de::Deserializer::list_from_str(input, &mut state).deserialize()
}

pub fn map_from_str<'de, T>(input: &'de str) -> Result<T, de::Located<'de, de::Error>>
where
    T: serde::de::Deserialize<'de>,
{
    let mut state = [Default::default(); 64];
    de::Deserializer::map_from_str(input, &mut state).deserialize()
}
