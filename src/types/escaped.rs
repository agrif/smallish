use core::borrow::Borrow;

use nom::{combinator, multi, Parser};

use crate::de::{
    token::{IResult, SliceChunk},
    TokenError, Tokenizer,
};

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

trait Stringlike<Slice: ?Sized>: Borrow<Slice>
where
    for<'a> &'a Slice: nom::Input,
{
    fn chunk<'a>(input: &'a [u8]) -> IResult<&'a [u8], SliceChunk<&'a Slice>>;

    fn as_bytes(slice: &Slice) -> &[u8];

    fn item_len(item: <&Slice as nom::Input>::Item) -> usize;

    fn item_write(item: <&Slice as nom::Input>::Item, buf: &mut [u8]);

    fn finalize(slice: &[u8]) -> &Slice;
}

impl<T> Stringlike<str> for T
where
    T: Borrow<str>,
{
    fn chunk<'a>(input: &'a [u8]) -> IResult<&'a [u8], SliceChunk<&'a str>> {
        Tokenizer::string_chunk(input)
    }

    fn as_bytes(slice: &str) -> &[u8] {
        slice.as_bytes()
    }

    fn item_len(item: char) -> usize {
        item.len_utf8()
    }

    fn item_write(item: char, buf: &mut [u8]) {
        item.encode_utf8(buf);
    }

    fn finalize(slice: &[u8]) -> &str {
        // safety: we just produced this directly from valid utf-8 &str
        // and raw characters
        unsafe { core::str::from_utf8_unchecked(slice) }
    }
}

impl<T> Stringlike<[u8]> for T
where
    T: Borrow<[u8]>,
{
    fn chunk<'a>(input: &'a [u8]) -> IResult<&'a [u8], SliceChunk<&'a [u8]>> {
        Tokenizer::bytes_chunk(input)
    }

    fn as_bytes(slice: &[u8]) -> &[u8] {
        slice
    }

    fn item_len(_item: u8) -> usize {
        1
    }

    fn item_write(item: u8, buf: &mut [u8]) {
        buf[0] = item;
    }

    fn finalize(slice: &[u8]) -> &[u8] {
        slice
    }
}

impl<T> Escaped<T>
where
    T: Borrow<str>,
{
    pub fn new_str(s: T) -> Result<Self, TokenError> {
        let s = Self(s);
        s.impl_check().map(|_| s)
    }

    pub fn str_has_escapes(&self) -> bool {
        self.impl_has_escapes()
    }

    pub fn unescape_str<'buf>(
        &self,
        buffer: &'buf mut [u8],
    ) -> Result<(&'buf mut [u8], &'buf str), UnescapeError> {
        self.impl_unescape(buffer)
    }
}

impl<T> Escaped<T>
where
    T: Borrow<[u8]>,
{
    pub fn new_bytes(s: T) -> Result<Self, TokenError> {
        let s = Self(s);
        s.impl_check().map(|_| s)
    }

    pub fn bytes_has_escapes(&self) -> bool {
        self.impl_has_escapes()
    }

    pub fn unescape_bytes<'buf>(
        &self,
        buffer: &'buf mut [u8],
    ) -> Result<(&'buf mut [u8], &'buf [u8]), UnescapeError> {
        self.impl_unescape(buffer)
    }
}

impl<T> Escaped<T> {
    pub fn new_unchecked(s: T) -> Self {
        Self(s)
    }

    pub fn as_escaped(self) -> T {
        self.0
    }

    fn impl_check<B>(&self) -> Result<(), TokenError>
    where
        T: Stringlike<B>,
        B: ?Sized,
        for<'a> &'a B: nom::Input,
    {
        let input = T::as_bytes(self.0.borrow());
        match combinator::recognize(multi::many0_count(T::chunk)).parse(input) {
            Ok((b"", _)) => Ok(()),
            Ok(_) => Err(TokenError::UnknownToken),
            Err(nom::Err::Incomplete(_)) => Err(TokenError::UnknownToken),
            Err(nom::Err::Error(e) | nom::Err::Failure(e)) => Err(e.error),
        }
    }

    fn impl_has_escapes<B>(&self) -> bool
    where
        T: Stringlike<B>,
        B: ?Sized,
        for<'a> &'a B: nom::Input,
    {
        !matches!(
            T::chunk.parse(T::as_bytes(self.0.borrow())),
            // if there is a single slice chunk, it has no escapes
            Ok((b"", SliceChunk::Slice(_))),
        )
    }

    fn impl_unescape<'buf, B, I>(
        &self,
        buffer: &'buf mut [u8],
    ) -> Result<(&'buf mut [u8], &'buf B), UnescapeError>
    where
        T: Stringlike<B>,
        B: ?Sized,
        for<'a> &'a B: nom::Input<Item = I>,
        I: Copy,
    {
        let mut input = T::as_bytes(self.0.borrow());
        let mut i = 0;
        while !input.is_empty() {
            match T::chunk.parse(input) {
                Ok((rest, chunk)) => {
                    assert!(rest.len() < input.len());
                    input = rest;
                    match chunk {
                        SliceChunk::Slice(s) => {
                            let bytes = T::as_bytes(s);
                            let amt = bytes.len();
                            buffer
                                .get_mut(i..i + amt)
                                .ok_or(UnescapeError::BufferFull)?
                                .copy_from_slice(bytes);
                            i += amt;
                        }
                        SliceChunk::Item(c) => {
                            let amt = T::item_len(c);
                            T::item_write(
                                c,
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
        Ok((unused, T::finalize(result)))
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
