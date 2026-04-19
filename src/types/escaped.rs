use core::borrow::Borrow;

use nom::{combinator, multi, Parser};

use crate::de::{token::IResult, TokenError, Tokenizer};

#[derive(Clone, Debug, Default, serde::Deserialize)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[serde(rename = "__smallish_magic_escaped__")]
pub struct Escaped<T>(T);

impl<T> Escaped<T> {
    pub(crate) const SERDE_NAME: &'static str = "__smallish_magic_escaped__";
}

#[derive(Clone, Debug, thiserror::Error)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum UnescapeError {
    #[error("bad unescaped literal")]
    BadLiteral(#[from] TokenError),
    #[error("unescape buffer full")]
    BufferFull,
}

#[derive(Clone, Copy, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum EscapedFragment<Slice, Item> {
    Slice(Slice),
    Item(Item),
}

trait Stringlike<Slice: ?Sized, Item>: Borrow<Slice> {
    fn chunk<'a>(input: &'a [u8]) -> IResult<&'a [u8], EscapedFragment<&'a Slice, Item>>;

    fn as_bytes(slice: &Slice) -> &[u8];

    fn item_len(item: Item) -> usize;

    fn item_write(item: Item, buf: &mut [u8]);

    fn finalize(slice: &[u8]) -> &Slice;
}

impl<T> Stringlike<str, char> for T
where
    T: Borrow<str>,
{
    fn chunk<'a>(input: &'a [u8]) -> IResult<&'a [u8], EscapedFragment<&'a str, char>> {
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

impl<T> Stringlike<[u8], u8> for T
where
    T: Borrow<[u8]>,
{
    fn chunk<'a>(input: &'a [u8]) -> IResult<&'a [u8], EscapedFragment<&'a [u8], u8>> {
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

    pub fn str_fragments(
        &self,
    ) -> impl Iterator<Item = Result<EscapedFragment<&str, char>, TokenError>> {
        self.impl_fragments()
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

    pub fn bytes_fragments(
        &self,
    ) -> impl Iterator<Item = Result<EscapedFragment<&[u8], u8>, TokenError>> {
        self.impl_fragments()
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

    fn impl_check<B, I>(&self) -> Result<(), TokenError>
    where
        T: Stringlike<B, I>,
        B: ?Sized,
    {
        let input = T::as_bytes(self.0.borrow());
        match combinator::recognize(multi::many0_count(T::chunk)).parse(input) {
            Ok((b"", _)) => Ok(()),
            Ok(_) => Err(TokenError::UnknownToken),
            Err(nom::Err::Incomplete(_)) => Err(TokenError::UnknownToken),
            Err(nom::Err::Error(e) | nom::Err::Failure(e)) => Err(e.error),
        }
    }

    fn impl_has_escapes<B, I>(&self) -> bool
    where
        T: Stringlike<B, I>,
        B: ?Sized,
    {
        !matches!(
            T::chunk.parse(T::as_bytes(self.0.borrow())),
            // if there is a single slice chunk, it has no escapes
            Ok((b"", EscapedFragment::Slice(_))),
        )
    }

    fn impl_fragments<'a, B, I>(
        &'a self,
    ) -> impl Iterator<Item = Result<EscapedFragment<&'a B, I>, TokenError>>
    where
        T: Stringlike<B, I>,
        B: ?Sized + 'a,
    {
        FragmentIterator::<'a, T, B, I> {
            input: T::as_bytes(self.0.borrow()),
            _marker: Default::default(),
        }
    }

    fn impl_unescape<'buf, B, I>(
        &self,
        buffer: &'buf mut [u8],
    ) -> Result<(&'buf mut [u8], &'buf B), UnescapeError>
    where
        T: Stringlike<B, I>,
        B: ?Sized,
        I: Copy,
    {
        let mut i = 0;
        for chunk in self.impl_fragments() {
            match chunk? {
                EscapedFragment::Slice(s) => {
                    let bytes = T::as_bytes(s);
                    let amt = bytes.len();
                    buffer
                        .get_mut(i..i + amt)
                        .ok_or(UnescapeError::BufferFull)?
                        .copy_from_slice(bytes);
                    i += amt;
                }
                EscapedFragment::Item(c) => {
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

        let (result, unused) = buffer.split_at_mut(i);
        Ok((unused, T::finalize(result)))
    }
}

#[derive(Clone, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
struct FragmentIterator<'a, T, B: ?Sized, I> {
    input: &'a [u8],
    _marker: core::marker::PhantomData<(T, &'a B, I)>,
}

impl<'a, T, B: ?Sized, I> core::iter::FusedIterator for FragmentIterator<'a, T, B, I> where
    T: Stringlike<B, I>
{
}

impl<'a, T, B: ?Sized, I> Iterator for FragmentIterator<'a, T, B, I>
where
    T: Stringlike<B, I>,
{
    type Item = Result<EscapedFragment<&'a B, I>, TokenError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.input.is_empty() {
            return None;
        }

        match T::chunk.parse(self.input) {
            Ok((rest, chunk)) => {
                assert!(rest.len() < self.input.len());
                self.input = rest;
                Some(Ok(chunk))
            }
            // can only be caused by a bad use of new_unchecked
            Err(nom::Err::Incomplete(_)) => Some(Err(TokenError::UnknownToken)),
            Err(nom::Err::Error(e) | nom::Err::Failure(e)) => Some(Err(e.error)),
        }
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
