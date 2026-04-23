use core::borrow::Borrow;

use nom::{combinator, multi, Parser};

use crate::de::{token::IResult, TokenError, Tokenizer};

/// Errors encountered by [Escaped::unescape].
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum UnescapeError {
    /// The string or bytes is malformed (usually a bad escape).
    #[error("bad unescaped literal")]
    BadLiteral(#[from] TokenError),
    /// The buffer used to unescape the string or bytes is full.
    #[error("unescape buffer full")]
    BufferFull,
}

/// A fragment of an escaped string or bytes from [Escaped::fragments].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum EscapedFragment<Slice, Item> {
    /// A slice of unescaped data (e.g. `&str` or `&[u8]`).
    Slice(Slice),
    /// A single item (e.g. `char` or `u8`).
    Item(Item),
}

/// A string or bytes value that contains escape sequences.
///
/// This type is used by *smallish* to represent string or bytes
/// literals that may contain escape sequences. It also has methods
/// such as [has_escapes](Escaped::has_escapes) and
/// [unescape](Escaped::unescape) to handle those escapes and convert
/// them into a plain string.
///
/// Most of the methods require that `T: Escapeable`, which
/// essentially means you can [Borrow] `T` as either `&str` or
/// `&[u8]`. This covers almost all string-like and bytes-like types.
///
/// [Escaped] implements [Deref](core::ops::Deref), and can be used in
/// the same places as a reference to the underlying type. To remove
/// the wrapped value entirely, use [Escaped::as_escaped].
///
/// ## Deserialization
///
/// This type modifies how deserialization works for the contained
/// type. String-like or bytes-like types wrapped in [Escaped] will
/// opt-out of the automatic un-escaping, allowing for them to be
/// deserialized without the scratch buffer usually used for
/// un-escaping. This also enables guaranteed zero-copy
/// deserialization, if the underlying type supports it.
///
/// ```
/// # use smallish::{Flavor, from_str, types::Escaped};
/// let r: Escaped<String> = from_str(Flavor::Value, r#""escapes\n\n""#).unwrap();
/// assert_eq!(*r, r#"escapes\n\n"#);
/// ```
///
/// Note that this only works for a string or bytes type directly
/// wrapped in [Escaped], e.g. `Escaped<&[u8]>` or
/// `Escaped<String>`. Types with more deeply nested strings will be
/// handled normally, with the normal automatic un-escaping.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Hash, serde::Deserialize)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[serde(rename = "__smallish_magic_escaped__")]
pub struct Escaped<T>(T);

impl<T> Escaped<T> {
    pub(crate) const SERDE_NAME: &'static str = "__smallish_magic_escaped__";
}

impl<T> Escaped<T> {
    /// Create a new escaped string or bytes from an underlying value.
    ///
    /// This is checked for validity: if the value is malformed
    /// (usually from a bad escape sequence), this will fail.
    ///
    /// To avoid this check, see [new_unchecked](Self::new_unchecked).
    pub fn new<B, I>(s: T) -> Result<Self, TokenError>
    where
        T: Escapeable<B, I>,
        B: ?Sized,
    {
        let escaped = Self(s);
        escaped.check()?;
        Ok(escaped)
    }

    fn check<B, I>(&self) -> Result<(), TokenError>
    where
        T: Escapeable<B, I>,
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

    /// Returns `true` if the contained value needs to be
    /// [unescape](Self::unescape)'d.
    ///
    /// This will also return `true` if the contained value has
    /// errors. Calling [unescape](Self::unescape) in that case will
    /// tell you exactly which error.
    pub fn has_escapes<B, I>(&self) -> bool
    where
        T: Escapeable<B, I>,
        B: ?Sized,
    {
        let bytes = T::as_bytes(self.0.borrow());

        // chunk always consumes some data, so we special-case empty strings
        if bytes.is_empty() {
            false
        } else {
            !matches!(
                T::chunk.parse(bytes),
                // if there is a single slice chunk, it has no escapes
                Ok((b"", EscapedFragment::Slice(_))),
            )
        }
    }

    /// Iterate over the fragments inside this string or bytes.
    ///
    /// This yields valid slices (`&str` or `&[u8]`) and un-escaped
    /// items (`char` or `u8`) from the wrapped value.
    ///
    /// If you have used [new_unchecked](Self::new_unchecked), it may
    /// also yield an error, usually due to an unknown escape.
    pub fn fragments<'a, B, I>(
        &'a self,
    ) -> impl Iterator<Item = Result<EscapedFragment<&'a B, I>, TokenError>>
    where
        T: Escapeable<B, I>,
        B: ?Sized + 'a,
    {
        FragmentIterator::<'a, T, B, I> {
            input: T::as_bytes(self.0.borrow()),
            _marker: Default::default(),
        }
    }

    /// Un-escape this string or bytes, using the provided scratch buffer.
    ///
    /// This un-escapes the contained value, returning a tuple
    /// containing the unused portion of the scratch buffer and the
    /// un-escaped value (either `&str` or `&[u8]`) itself.
    ///
    /// This will fail if the value contains a bad escape, or if the
    /// scratch buffer provided is too small.
    ///
    /// Note that due to lifetime requirements, this function will
    /// *always* perform a copy, even if the contained value has no
    /// escapes. If you need zero-copy behavior, check
    /// [has_escapes](Self::has_escapes) first to see if it is even
    /// necessary to call this function.
    pub fn unescape<'buf, B, I>(
        &self,
        buffer: &'buf mut [u8],
    ) -> Result<(&'buf mut [u8], &'buf B), UnescapeError>
    where
        T: Escapeable<B, I>,
        B: ?Sized,
        I: Copy,
    {
        let mut i = 0;
        for chunk in self.fragments() {
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

impl<T> Escaped<T> {
    /// Create a new escaped string or bytes from an underlying value,
    /// without checking it.
    ///
    /// This wraps the value directly, even if it contains invalid
    /// escapes. This is not unsafe, but it might result in errors
    /// when trying to [unescape](Self::unescape) it. To check the
    /// value for validity up-front, see [new](Self::new).
    pub fn new_unchecked(s: T) -> Self {
        Self(s)
    }

    /// Extract the wrapped value.
    ///
    /// This removes the [Escaped] wrapper and returns the wrapped
    /// value. If this value was deserialized, it may contain escapes!
    pub fn as_escaped(self) -> T {
        self.0
    }
}

#[derive(Clone, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
struct FragmentIterator<'a, T, B: ?Sized, I> {
    input: &'a [u8],
    _marker: core::marker::PhantomData<(T, &'a B, I)>,
}

impl<'a, T, B: ?Sized, I> core::iter::FusedIterator for FragmentIterator<'a, T, B, I> where
    T: Escapeable<B, I>
{
}

impl<'a, T, B: ?Sized, I> Iterator for FragmentIterator<'a, T, B, I>
where
    T: Escapeable<B, I>,
{
    type Item = Result<EscapedFragment<&'a B, I>, TokenError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.input.is_empty() {
            return None;
        }

        match T::chunk.parse(self.input) {
            Ok((rest, chunk)) => {
                assert!(
                    rest.len() < self.input.len(),
                    "FragmentIterator did not make forward progress"
                );
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

/// Types that are suitable to be wrapped in [Escaped].
///
/// This trait is sealed, meaning it can only be implemented by this
/// crate. However, it comes with implementations for any type that
/// implements the [Borrow] trait and yields `&str` or `&[u8]`.
#[allow(private_bounds)]
pub trait Escapeable<Slice: ?Sized, Item>: SealedEscapeable<Slice, Item> {}

trait SealedEscapeable<Slice: ?Sized, Item>: Borrow<Slice> {
    fn chunk<'a>(input: &'a [u8]) -> IResult<&'a [u8], EscapedFragment<&'a Slice, Item>>;

    fn as_bytes(slice: &Slice) -> &[u8];

    fn item_len(item: Item) -> usize;

    fn item_write(item: Item, buf: &mut [u8]);

    fn finalize(slice: &[u8]) -> &Slice;
}

impl<T> Escapeable<str, char> for T where T: Borrow<str> {}

impl<T> SealedEscapeable<str, char> for T
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

impl<T> Escapeable<[u8], u8> for T where T: Borrow<[u8]> {}

impl<T> SealedEscapeable<[u8], u8> for T
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

#[cfg(test)]
mod test {
    #[test]
    fn str_bad_escape() {
        use super::{Escaped, TokenError};
        assert_eq!(Escaped::new(r#"hello\?"#), Err(TokenError::UnknownEscape));
    }

    #[test]
    fn bytes_bad_escape() {
        use super::{Escaped, TokenError};
        assert_eq!(
            Escaped::new(br#"hello\?"#.as_ref()),
            Err(TokenError::UnknownEscape)
        );
    }

    #[test]
    fn str_bad_escape_unchecked() {
        use super::{Escaped, TokenError, UnescapeError};
        let e = Escaped::new_unchecked(r#"\?"#);
        let mut buf = [0; 128];
        assert_eq!(Some(Err(TokenError::UnknownEscape)), e.fragments().next());
        assert_eq!(
            Err(UnescapeError::BadLiteral(TokenError::UnknownEscape)),
            e.unescape(&mut buf)
        );
    }

    #[test]
    fn bytes_bad_escape_unchecked() {
        use super::{Escaped, TokenError, UnescapeError};
        let e = Escaped::new_unchecked(br#"\?"#.as_ref());
        let mut buf = [0; 128];
        assert_eq!(Some(Err(TokenError::UnknownEscape)), e.fragments().next());
        assert_eq!(
            Err(UnescapeError::BadLiteral(TokenError::UnknownEscape)),
            e.unescape(&mut buf)
        );
    }

    #[test]
    fn str_buffer_full() {
        use super::{Escaped, UnescapeError};
        let mut buf = [0; 0];
        let e = Escaped::new_unchecked(r#"hello\n"#);
        assert_eq!(Err(UnescapeError::BufferFull), e.unescape(&mut buf));
        let e = Escaped::new_unchecked(r#"\n"#);
        assert_eq!(Err(UnescapeError::BufferFull), e.unescape(&mut buf));
    }

    #[test]
    fn bytes_buffer_full() {
        use super::{Escaped, UnescapeError};
        let mut buf = [0; 0];
        let e = Escaped::new_unchecked(br#"hello\n"#.as_ref());
        assert_eq!(Err(UnescapeError::BufferFull), e.unescape(&mut buf));
        let e = Escaped::new_unchecked(br#"\n"#.as_ref());
        assert_eq!(Err(UnescapeError::BufferFull), e.unescape(&mut buf));
    }

    #[test]
    fn as_escaped() {
        use super::Escaped;
        assert_eq!(42, Escaped::new_unchecked(42).as_escaped());
    }

    #[test]
    fn deref() {
        use super::Escaped;
        let e = Escaped::new("hello").unwrap();
        assert_eq!("hello", *e);
    }

    macro_rules! test_escape {
        ($(#[$attr:meta])* $name:ident, $str:expr, $unescaped:literal $(,$frag:expr)* $(,)?) => {
            #[test]
            $(#[$attr])*
            #[allow(unused_assignments)]
            fn $name() {
                #[allow(unused)]
                use super::{Escaped, EscapedFragment::*};
                let e = Escaped::new($str).unwrap();
                let mut fragments = &[$($frag),*][..];
                if false {
                    // funny business to force the right type on fragments
                    fragments = &[e.fragments().next().unwrap().unwrap()][..];
                    fragments = &[];
                }
                let has_escapes = fragments.iter().any(|f| matches!(f, Item(_)));
                assert_eq!(has_escapes, e.has_escapes());

                let mut iter = e.fragments();
                for frag in fragments {
                    assert_eq!(Some(Ok(*frag)), iter.next());
                }
                assert_eq!(None, iter.next());

                let mut buf = [0; 128];
                let (_, s) = e.unescape(&mut buf).unwrap();
                assert_eq!(s, $unescaped);
            }
        };
    }

    test_escape!(str_empty, r#""#, "");
    test_escape!(str_plain, r#"hello"#, "hello", Slice("hello"));
    test_escape!(str_escape, r#"\n"#, "\n", Item('\n'));
    test_escape!(
        str_mixed,
        r#"hel\nlo"#,
        "hel\nlo",
        Slice("hel"),
        Item('\n'),
        Slice("lo")
    );

    test_escape!(bytes_empty, br#""#.as_ref(), b"");
    test_escape!(
        bytes_plain,
        br#"hello"#.as_ref(),
        b"hello",
        Slice(b"hello".as_ref())
    );
    test_escape!(bytes_escape, br#"\n"#.as_ref(), b"\n", Item(b'\n'));
    test_escape!(
        bytes_mixed,
        br#"hel\nlo"#.as_ref(),
        b"hel\nlo",
        Slice(b"hel".as_ref()),
        Item(b'\n'),
        Slice(b"lo".as_ref())
    );
}
