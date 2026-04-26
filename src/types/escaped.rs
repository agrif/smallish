use core::ops::Deref;

use nom::{combinator, multi, Parser};

use crate::de::token::{IResult, NomError};
use crate::de::{TokenError, Tokenizer};
use crate::types::Located;

/// Errors encountered by [Escaped].
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum UnescapeError {
    /// The string or bytes is has an unknown escape sequence.
    #[error("unknown escape sequence")]
    UnknownEscape,
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
/// Most of the methods require that `T: Escapable`, which
/// essentially means you can [Deref] `T` as either `&str` or
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
    /// This is checked for validity: if the value has a bad escape
    /// sequence, this will fail.
    ///
    /// Note that due to lifetime requirements, the returned error is
    /// [Located] but does not have an attached source. If you want to
    /// attach the source yourself, use [Located::with_source] to
    /// attach the same value you passed in here.
    ///
    /// To avoid this check, see [new_unchecked](Self::new_unchecked).
    pub fn new<I>(s: T) -> Result<Self, Located<'static, UnescapeError>>
    where
        T: Escapable<I>,
    {
        let escaped = Self(s);
        escaped.check().map_err(|loc| loc.without_source())?;
        Ok(escaped)
    }

    fn convert_nom_error<'a, I>(
        &'a self,
        error: nom::Err<NomError<&'a [u8]>>,
    ) -> Located<'a, UnescapeError>
    where
        T: Escapable<I>,
    {
        let src = T::as_bytes(&self.0);

        // this can only produce UnknownEscape, but check when we can
        let (rest, err) = match error {
            nom::Err::Incomplete(_) => (src, UnescapeError::UnknownEscape),
            nom::Err::Error(e) | nom::Err::Failure(e) => {
                assert_eq!(
                    e.error,
                    TokenError::UnknownEscape,
                    "string/bytes tokenizer error other than UnknownEscape"
                );
                (e.input, UnescapeError::UnknownEscape)
            }
        };

        let mut loc = Located::new().with_source(Some(src)).replace(err);
        loc.advance(src, rest);
        loc
    }

    fn check<'a, I>(&'a self) -> Result<(), Located<'a, UnescapeError>>
    where
        T: Escapable<I>,
    {
        let input = T::as_bytes(&self.0);
        match combinator::recognize(multi::many0_count(T::chunk)).parse(input) {
            Ok((b"", _)) => Ok(()),
            Ok(_) => Err(Located::new()
                .with_source(Some(input))
                .replace(UnescapeError::UnknownEscape)),
            Err(e) => Err(self.convert_nom_error(e)),
        }
    }

    /// Returns `true` if the contained value needs to be
    /// [unescape](Self::unescape)'d.
    ///
    /// This will also return `true` if the contained value has
    /// errors. Calling [unescape](Self::unescape) in that case will
    /// tell you exactly which error.
    pub fn has_escapes<I>(&self) -> bool
    where
        T: Escapable<I>,
    {
        let bytes = T::as_bytes(&self.0);

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
    /// also yield an error due to an unknown escape.
    pub fn fragments<'a, I>(
        &'a self,
    ) -> impl Iterator<Item = Result<EscapedFragment<&'a T::Target, I>, Located<'a, UnescapeError>>>
    where
        T: Escapable<I>,
    {
        FragmentIterator::<'a, T, I> {
            input: T::as_bytes(&self.0),
            escaped: self,
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
    pub fn unescape<'a, 'buf, I>(
        &'a self,
        buffer: &'buf mut [u8],
    ) -> Result<(&'buf mut [u8], &'buf T::Target), Located<'a, UnescapeError>>
    where
        T: Escapable<I>,
        I: Copy,
    {
        let mut i = 0;
        let src = T::as_bytes(&self.0);
        for chunk in self.fragments() {
            match chunk? {
                EscapedFragment::Slice(s) => {
                    let bytes = T::as_bytes(s);
                    let amt = bytes.len();
                    buffer
                        .get_mut(i..i + amt)
                        .ok_or(
                            Located::new()
                                .with_source(Some(src))
                                .replace(UnescapeError::BufferFull),
                        )?
                        .copy_from_slice(bytes);
                    i += amt;
                }
                EscapedFragment::Item(c) => {
                    let amt = T::item_len(c);
                    T::item_write(
                        c,
                        buffer.get_mut(i..i + amt).ok_or(
                            Located::new()
                                .with_source(Some(src))
                                .replace(UnescapeError::BufferFull),
                        )?,
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
struct FragmentIterator<'a, T, I> {
    escaped: &'a Escaped<T>,
    input: &'a [u8],
    _marker: core::marker::PhantomData<I>,
}

impl<'a, T, I> core::iter::FusedIterator for FragmentIterator<'a, T, I>
where
    T: Escapable<I>,
    T::Target: 'a,
{
}

impl<'a, T, I> Iterator for FragmentIterator<'a, T, I>
where
    T: Escapable<I>,
    T::Target: 'a,
{
    type Item = Result<EscapedFragment<&'a T::Target, I>, Located<'a, UnescapeError>>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.input.is_empty() {
            return None;
        }

        match T::chunk
            .parse(self.input)
            .map_err(|e| self.escaped.convert_nom_error(e))
        {
            Ok((rest, chunk)) => {
                assert!(
                    rest.len() < self.input.len(),
                    "FragmentIterator did not make forward progress"
                );
                self.input = rest;
                Some(Ok(chunk))
            }
            // can only be caused by a bad use of new_unchecked
            Err(e) => Some(Err(e)),
        }
    }
}

impl<T> Deref for Escaped<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Types that are suitable to be wrapped in [Escaped].
///
/// This trait is sealed, meaning it can only be implemented by this
/// crate. However, it comes with implementations for any type that
/// implements the [Deref] trait and yields `&str` or `&[u8]`.
#[allow(private_bounds)]
pub trait Escapable<Item>: SealedEscapable<Item> + Deref {}

trait SealedEscapable<Item>: Deref {
    fn chunk(input: &[u8]) -> IResult<&[u8], EscapedFragment<&Self::Target, Item>>;

    fn as_bytes(slice: &Self::Target) -> &[u8];

    fn item_len(item: Item) -> usize;

    fn item_write(item: Item, buf: &mut [u8]);

    fn finalize(slice: &[u8]) -> &Self::Target;
}

impl<T> Escapable<char> for T where T: Deref<Target = str> {}

impl<T> SealedEscapable<char> for T
where
    T: Deref<Target = str>,
{
    fn chunk(input: &[u8]) -> IResult<&[u8], EscapedFragment<&str, char>> {
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

impl<T> Escapable<u8> for T where T: Deref<Target = [u8]> {}

impl<T> SealedEscapable<u8> for T
where
    T: Deref<Target = [u8]>,
{
    fn chunk(input: &[u8]) -> IResult<&[u8], EscapedFragment<&[u8], u8>> {
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
        use super::{Escaped, Located, UnescapeError};
        let err = Located {
            source: None,
            line: 1,
            column: 6,
            offset: 6,
            value: UnescapeError::UnknownEscape,
        };
        assert_eq!(Escaped::new(r#"hello\?"#), Err(err));
    }

    #[test]
    fn bytes_bad_escape() {
        use super::{Escaped, Located, UnescapeError};
        let err = Located {
            source: None,
            line: 1,
            column: 6,
            offset: 6,
            value: UnescapeError::UnknownEscape,
        };
        assert_eq!(Escaped::new(br#"hello\?"#.as_ref()), Err(err));
    }

    #[test]
    fn str_bad_escape_unchecked() {
        use super::{Escaped, Located, UnescapeError};
        let e = Escaped::new_unchecked(r#"\?"#);
        let mut buf = [0; 128];
        let err = Located {
            source: None,
            line: 1,
            column: 1,
            offset: 1,
            value: UnescapeError::UnknownEscape,
        };
        assert_eq!(Some(Err(err)), e.fragments().next());
        assert_eq!(Err(err), e.unescape(&mut buf));
    }

    #[test]
    fn bytes_bad_escape_unchecked() {
        use super::{Escaped, Located, UnescapeError};
        let e = Escaped::new_unchecked(br#"\?"#.as_ref());
        let mut buf = [0; 128];
        let err = Located {
            source: None,
            line: 1,
            column: 1,
            offset: 1,
            value: UnescapeError::UnknownEscape,
        };
        assert_eq!(Some(Err(err)), e.fragments().next());
        assert_eq!(Err(err), e.unescape(&mut buf));
    }

    #[test]
    fn str_buffer_full() {
        use super::{Escaped, Located, UnescapeError};
        let mut buf = [0; 0];
        let err = Located {
            source: None,
            line: 1,
            column: 0,
            offset: 0,
            value: UnescapeError::BufferFull,
        };
        let e = Escaped::new_unchecked(r#"hello\n"#);
        assert_eq!(Err(err), e.unescape(&mut buf));
        let e = Escaped::new_unchecked(r#"\n"#);
        assert_eq!(Err(err), e.unescape(&mut buf));
    }

    #[test]
    fn bytes_buffer_full() {
        use super::{Escaped, Located, UnescapeError};
        let mut buf = [0; 0];
        let err = Located {
            source: None,
            line: 1,
            column: 0,
            offset: 0,
            value: UnescapeError::BufferFull,
        };
        let e = Escaped::new_unchecked(br#"hello\n"#.as_ref());
        assert_eq!(Err(err), e.unescape(&mut buf));
        let e = Escaped::new_unchecked(br#"\n"#.as_ref());
        assert_eq!(Err(err), e.unescape(&mut buf));
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
