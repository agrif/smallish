#![no_std]
#![cfg_attr(docsrs, feature(doc_cfg))]

//! [LICENSE]: https://github.com/agrif/smallish/blob/main/LICENSE
#![doc = include_str!("../README.md")]

#[macro_use]
mod macros;

pub mod de;
pub mod syntax;

#[derive(Clone, Copy, Debug, Default)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Flavor {
    #[default]
    Value,
    List,
    Map,
}

pub fn from_str<'de, T>(flavor: Flavor, input: &'de str) -> Result<T, de::Located<'de, de::Error>>
where
    T: serde::de::Deserialize<'de>,
{
    let mut state = [Default::default(); 64];
    de::Deserializer::new(flavor, input, &mut state).deserialize()
}
