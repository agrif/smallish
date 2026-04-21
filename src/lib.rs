#![no_std]
#![cfg_attr(docsrs, feature(doc_cfg))]

//! [LICENSE]: https://github.com/agrif/smallish/blob/main/LICENSE
#![doc = include_str!("../README.md")]

#[macro_use]
mod macros;

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

// helper to format strings nicely in error message
struct FormatIter<'a, I> {
    iter: core::cell::Cell<Option<I>>,
    sep: &'a str,
}

impl<'a, I> FormatIter<'a, I> {
    fn new(iter: I, sep: &'a str) -> Self {
        Self {
            iter: core::cell::Cell::new(Some(iter)),
            sep,
        }
    }
}

impl<'a, I> core::fmt::Display for FormatIter<'a, I>
where
    I: Iterator,
    I::Item: core::fmt::Display,
{
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        let iter = self.iter.take().expect("FormatIter used more than once");
        let mut first = true;
        for part in iter {
            let sep = if !first { self.sep } else { "" };
            write!(f, "{}{}", sep, part)?;
            first = false;
        }
        Ok(())
    }
}
