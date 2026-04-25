use nom::{
    branch, bytes::complete as bytes, character::complete as character, combinator, error, multi,
    number::complete as number, sequence, Parser,
};

use crate::syntax::{Int, Token, Value};
use crate::types::{Escaped, EscapedFragment, LocResult, Located};

/// Errors produced by [Tokenizer].
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum TokenError {
    /// End of file (there is no more input to consume).
    #[error("end of file")]
    Eof,
    /// The tokenizer found something it didn't recognize.
    #[error("unknown token")]
    UnknownToken,
    /// An integer literal was too big to fit into [Int].
    #[error("integer out of range")]
    IntRange,
    /// There is invalid utf-8 inside a string literal.
    #[error("invalid utf-8")]
    InvalidUtf8,
    /// There is an invalid escape sequence inside a string or bytes literal.
    #[error("unknown escape sequence")]
    UnknownEscape,
    /// There is an invalid character literal.
    #[error("invalid character literal")]
    InvalidCharLiteral,
}

#[derive(Clone, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub(crate) struct NomError<I> {
    pub(crate) input: I,
    pub(crate) error: TokenError,
}

impl<I> error::ParseError<I> for NomError<I> {
    fn from_error_kind(input: I, _kind: error::ErrorKind) -> Self {
        Self {
            input,
            error: TokenError::UnknownToken,
        }
    }

    fn append(_input: I, _kind: error::ErrorKind, other: Self) -> Self {
        other
    }
}

impl<I> error::FromExternalError<I, TokenError> for NomError<I> {
    fn from_external_error(input: I, _kind: error::ErrorKind, err: TokenError) -> Self {
        NomError { input, error: err }
    }
}

impl<I> error::FromExternalError<I, NomError<I>> for NomError<I> {
    fn from_external_error(_input: I, _kind: error::ErrorKind, err: NomError<I>) -> Self {
        err
    }
}

pub(crate) type IResult<I, O> = nom::IResult<I, O, NomError<I>>;

/// Turn source into a stream of [Tokens](Token).
///
/// This type implements [Iterator], and can be used in a `for` loop.
#[derive(Clone, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Tokenizer<'de> {
    input: &'de [u8],
    location: Located<'de, ()>,
}

impl<'de> Tokenizer<'de> {
    /// Create a tokenizer operating on the given `input`.
    pub fn new(input: &'de [u8]) -> Self {
        let mut tokenizer = Self {
            input,
            location: Located::new().with_source(Some(input)),
        };
        let _ = tokenizer.parse(Self::whitespace_and_newlines0);
        tokenizer
    }

    /// Return the location of the token to be parsed next.
    pub fn location(&self) -> Located<'de, ()> {
        self.location
    }

    /// Return `true` if and only if there is no more input left.
    ///
    /// This is `true` if and only if the next event will be
    /// `TokenError::Eof`.
    pub fn is_eof(&self) -> bool {
        self.input.is_empty()
    }

    fn parse<P>(&mut self, mut parser: P) -> LocResult<'de, P::Output, TokenError>
    where
        P: Parser<&'de [u8], Error = NomError<&'de [u8]>>,
    {
        if self.is_eof() {
            return Err(self.location.wrap(TokenError::Eof));
        }

        match parser.parse(self.input) {
            Ok((input, v)) => {
                let r = self.location.wrap(v);
                self.location.advance(self.input, input);
                self.input = input;
                Ok(r)
            }
            // Incomplete should never occur, but we can catch it
            Err(nom::Err::Incomplete(_)) => Err(self.location.wrap(TokenError::UnknownToken)),
            Err(nom::Err::Error(e)) => {
                // errors should point to token start
                let r = self.location.wrap(e.error);
                Err(r)
            }
            Err(nom::Err::Failure(e)) => {
                // failures are specific to exactly where they failed
                let mut loc: Located<()> = self.location;
                loc.advance(self.input, e.input);
                Err(loc.replace(e.error))
            }
        }
    }

    /// Parse and return the next token in the stream.
    pub fn next_token(&mut self) -> LocResult<'de, Token<'de>, TokenError> {
        self.parse(Self::token)
    }

    fn parse_utf8<'a, P>(
        mut parser: P,
    ) -> impl Parser<&'a [u8], Error = NomError<&'a [u8]>, Output = &'a str>
    where
        P: Parser<&'a [u8], Error = NomError<&'a [u8]>, Output = &'a [u8]>,
    {
        move |input| {
            let (rest, s) = parser.parse(input)?;
            match core::str::from_utf8(s) {
                Ok(s) => Ok((rest, s)),
                Err(e) => Err(nom::Err::Failure(NomError {
                    input: input.get(e.valid_up_to()..).unwrap_or(input),
                    error: TokenError::InvalidUtf8,
                })),
            }
        }
    }

    fn whitespace_single(input: &[u8]) -> IResult<&[u8], ()> {
        branch::alt((
            character::space1,
            sequence::preceded(character::char('#'), character::not_line_ending),
        ))
        .map(|_| ())
        .parse(input)
    }

    fn whitespace1(input: &[u8]) -> IResult<&[u8], ()> {
        multi::many1_count(Self::whitespace_single)
            .map(|_| ())
            .parse(input)
    }

    fn whitespace0(input: &[u8]) -> IResult<&[u8], ()> {
        multi::many0_count(Self::whitespace_single)
            .map(|_| ())
            .parse(input)
    }

    fn newlines1(input: &[u8]) -> IResult<&[u8], ()> {
        multi::many1_count((character::line_ending, Self::whitespace0))
            .map(|_| ())
            .parse(input)
    }

    fn newlines0(input: &[u8]) -> IResult<&[u8], ()> {
        multi::many0_count((character::line_ending, Self::whitespace0))
            .map(|_| ())
            .parse(input)
    }

    fn whitespace_and_newlines0(input: &[u8]) -> IResult<&[u8], ()> {
        (Self::whitespace0, Self::newlines0)
            .map(|_| ())
            .parse(input)
    }

    fn newline<'a>(input: &'a [u8]) -> IResult<&'a [u8], Token<'a>> {
        Self::newlines1.map(|_| Token::Newline).parse(input)
    }

    fn comma<'a>(input: &'a [u8]) -> IResult<&'a [u8], Token<'a>> {
        sequence::preceded(character::char(','), Self::newlines0)
            .map(|_| Token::Comma)
            .parse(input)
    }

    fn symbol<'a>(input: &'a [u8]) -> IResult<&'a [u8], Token<'a>> {
        branch::alt((
            character::char(',').map(|_| Token::Comma),
            character::char('=').map(|_| Token::Equals),
            character::char('(').map(|_| Token::ParenOpen),
            character::char(')').map(|_| Token::ParenClose),
            character::char('[').map(|_| Token::ListOpen),
            character::char(']').map(|_| Token::ListClose),
            character::char('{').map(|_| Token::MapOpen),
            character::char('}').map(|_| Token::MapClose),
        ))
        .parse(input)
    }

    fn token_boundary(input: &[u8]) -> IResult<&[u8], ()> {
        combinator::peek(branch::alt((
            combinator::eof.map(|_| ()),
            character::line_ending.map(|_| ()),
            Self::symbol.map(|_| ()),
            Self::whitespace1,
        )))
        .parse(input)
    }

    fn unit<'a>(input: &'a [u8]) -> IResult<&'a [u8], Token<'a>> {
        sequence::delimited(
            character::char('('),
            Self::whitespace0,
            character::char(')'),
        )
        .map(|_| Token::Value(Value::Unit))
        .parse(input)
    }

    fn raw_ident(input: &[u8]) -> IResult<&[u8], &str> {
        sequence::terminated(
            combinator::verify(
                combinator::recognize((
                    // might start with a \
                    combinator::opt(character::char('\\')),
                    // valid characters
                    bytes::take_while1(|c: u8| c.is_ascii_alphanumeric() || c == b'_'),
                )),
                // should not start with a digit
                |s: &[u8]| !s.first().map(u8::is_ascii_digit).unwrap_or(true),
            ),
            Self::token_boundary,
        )
        // safety: the above parser only matches valid ascii
        .map(|s| unsafe { core::str::from_utf8_unchecked(s) })
        .parse(input)
    }

    fn ident_or_literal<'a>(input: &'a [u8]) -> IResult<&'a [u8], Token<'a>> {
        Self::raw_ident
            .map(|id| match id {
                "none" => Token::Value(Value::None),
                "true" => Token::Value(Value::Bool(true)),
                "false" => Token::Value(Value::Bool(false)),
                s if s.starts_with("\\") => Token::Ident(&s[1..]),
                s => Token::Ident(s),
            })
            .parse(input)
    }

    // Int::from_ascii_radix is insufficient for handling Int::MIN
    // however, by all means read that source for why this is this way
    fn from_ascii_radix(is_positive: bool, src: &[u8], radix: u32) -> Result<Int, TokenError> {
        assert!((2..=16).contains(&radix), "invalid radix");
        assert!(!src.is_empty(), "empty integer");

        let rad = radix as Int;
        let mut v = 0;

        // keep this weird phrasing to follow Int::from_ascii_radix source
        #[allow(clippy::int_plus_one)]
        if src.len() <= core::mem::size_of::<Int>() * 2 - 1 {
            // this is guaranteed not to overflow, there's not enough digits
            if is_positive {
                for c in src {
                    v = v * rad + unwrap!((*c as char).to_digit(radix)) as Int;
                }
            } else {
                for c in src {
                    v = v * rad - unwrap!((*c as char).to_digit(radix)) as Int;
                }
            }
        } else {
            // this might overflow
            if is_positive {
                for c in src {
                    let mul = v.checked_mul(rad);
                    let x = unwrap!((*c as char).to_digit(radix)) as Int;
                    v = mul.ok_or(TokenError::IntRange)?;
                    v = v.checked_add(x).ok_or(TokenError::IntRange)?;
                }
            } else {
                for c in src {
                    let mul = v.checked_mul(rad);
                    let x = unwrap!((*c as char).to_digit(radix)) as Int;
                    v = mul.ok_or(TokenError::IntRange)?;
                    v = v.checked_sub(x).ok_or(TokenError::IntRange)?;
                }
            }
        }

        Ok(v)
    }

    fn integer<'a>(input: &'a [u8]) -> IResult<&'a [u8], Token<'a>> {
        let (rest, is_positive) = combinator::opt(character::one_of("-+"))
            .map(|s| s.unwrap_or('+') == '+')
            .parse(input)?;

        let (rest, value) = branch::alt((
            sequence::delimited(
                (character::char('0'), character::one_of("xX")),
                character::hex_digit1.map(|s| Self::from_ascii_radix(is_positive, s, 16)),
                Self::token_boundary,
            ),
            sequence::delimited(
                (character::char('0'), character::one_of("oO")),
                character::oct_digit1.map(|s| Self::from_ascii_radix(is_positive, s, 8)),
                Self::token_boundary,
            ),
            sequence::delimited(
                (character::char('0'), character::one_of("bB")),
                character::bin_digit1.map(|s| Self::from_ascii_radix(is_positive, s, 2)),
                Self::token_boundary,
            ),
            sequence::terminated(
                character::digit1.map(|s| Self::from_ascii_radix(is_positive, s, 10)),
                Self::token_boundary,
            ),
        ))
        .parse(rest)?;

        let value = value.map_err(|e| nom::Err::Failure(NomError { input, error: e }))?;

        Ok((rest, Token::Value(Value::Int(value))))
    }

    fn float<'a>(input: &'a [u8]) -> IResult<&'a [u8], Token<'a>> {
        // number::float works, but also accepts nan, inf, infinity
        // (but not -inf, -infinity??)
        // so, just do our own to only accept honest-to-god numbers and
        // kick the exception can down the road
        sequence::terminated(number::recognize_float, Self::token_boundary)
            .map_opt(|f| {
                // safety: recognize_float only matches valid utf-8
                let f = unsafe { core::str::from_utf8_unchecked(f) };
                let f = f.parse().ok()?;
                Some(Token::Value(Value::Float(f)))
            })
            .parse(input)
    }

    fn string_plain(input: &[u8]) -> IResult<&[u8], EscapedFragment<&str, char>> {
        Self::parse_utf8(combinator::verify(bytes::is_not("\"\\"), |s: &[u8]| {
            !s.is_empty()
        }))
        .map(EscapedFragment::Slice)
        .parse(input)
    }

    fn character_escape(input: &[u8]) -> IResult<&[u8], char> {
        branch::alt((
            character::char('n').map(|_| '\n'),
            character::char('r').map(|_| '\r'),
            character::char('t').map(|_| '\t'),
            character::char('\\').map(|_| '\\'),
            character::char('0').map(|_| '\0'),
            character::char('"').map(|_| '"'),
            character::char('\'').map(|_| '\''),
            // \x7f
            sequence::preceded(
                character::char('x'),
                combinator::recognize((
                    character::satisfy(|c| ('0'..='7').contains(&c)),
                    character::satisfy(|c| c.is_ascii_hexdigit()),
                ))
                .map_opt(|v| {
                    // safety: this escape only recognizes valid ascii
                    // and only up to 7f
                    unsafe {
                        let v = core::str::from_utf8_unchecked(v);
                        let v = u32::from_str_radix(v, 16).ok()?;
                        Some(char::from_u32_unchecked(v))
                    }
                }),
            ),
            // \u{ffffff}
            sequence::delimited(
                bytes::tag("u{"),
                bytes::take_while_m_n(1, 6, |c: u8| c.is_ascii_hexdigit()).map_opt(|v| {
                    // safety: this escape only recognizes valid ascii
                    let v = unsafe { core::str::from_utf8_unchecked(v) };
                    let v = u32::from_str_radix(v, 16).ok()?;
                    char::from_u32(v)
                }),
                character::char('}'),
            ),
            combinator::success(()).map_res(|_| Err(TokenError::UnknownEscape)),
        ))
        .parse(input)
    }

    fn string_escape(input: &[u8]) -> IResult<&[u8], EscapedFragment<&str, char>> {
        Self::character_escape
            .map(EscapedFragment::Item)
            .parse(input)
    }

    pub(crate) fn string_chunk(input: &[u8]) -> IResult<&[u8], EscapedFragment<&str, char>> {
        branch::alt((
            Self::string_plain,
            sequence::preceded(character::char('\\'), combinator::cut(Self::string_escape)),
        ))
        .parse(input)
    }

    fn string<'a>(input: &'a [u8]) -> IResult<&'a [u8], Token<'a>> {
        sequence::delimited(
            character::char('"'),
            combinator::cut(combinator::recognize(multi::many0_count(
                Self::string_chunk,
            ))),
            character::char('"'),
        )
        .map(|s| {
            Token::Value(Value::Str(Escaped::new_unchecked(
                // safety: the string parsers already check for utf-8
                unsafe { core::str::from_utf8_unchecked(s) },
            )))
        })
        .parse(input)
    }

    fn bytes_plain(input: &[u8]) -> IResult<&[u8], EscapedFragment<&[u8], u8>> {
        combinator::verify(bytes::is_not("\"\\"), |s: &[u8]| !s.is_empty())
            .map(EscapedFragment::Slice)
            .parse(input)
    }

    fn single_byte_escape(input: &[u8]) -> IResult<&[u8], u8> {
        branch::alt((
            character::char('n').map(|_| b'\n'),
            character::char('r').map(|_| b'\r'),
            character::char('t').map(|_| b'\t'),
            character::char('\\').map(|_| b'\\'),
            character::char('0').map(|_| b'\0'),
            character::char('"').map(|_| b'"'),
            character::char('\'').map(|_| b'\''),
            // \x7f
            sequence::preceded(
                character::char('x'),
                bytes::take_while_m_n(2, 2, |c: u8| c.is_ascii_hexdigit()).map_opt(|v| {
                    // safety: this escape only recognizes valid ascii
                    // and only up to ff
                    let v = unsafe { core::str::from_utf8_unchecked(v) };
                    u8::from_str_radix(v, 16).ok()
                }),
            ),
            combinator::success(()).map_res(|_| Err(TokenError::UnknownEscape)),
        ))
        .parse(input)
    }

    fn bytes_escape(input: &[u8]) -> IResult<&[u8], EscapedFragment<&[u8], u8>> {
        Self::single_byte_escape
            .map(EscapedFragment::Item)
            .parse(input)
    }

    pub(crate) fn bytes_chunk(input: &[u8]) -> IResult<&[u8], EscapedFragment<&[u8], u8>> {
        branch::alt((
            Self::bytes_plain,
            sequence::preceded(character::char('\\'), combinator::cut(Self::bytes_escape)),
        ))
        .parse(input)
    }

    fn bytes<'a>(input: &'a [u8]) -> IResult<&'a [u8], Token<'a>> {
        sequence::delimited(
            bytes::tag("b\""),
            combinator::cut(combinator::recognize(multi::many0_count(Self::bytes_chunk))),
            character::char('"'),
        )
        .map(|s| Token::Value(Value::Bytes(Escaped::new_unchecked(s))))
        .parse(input)
    }

    fn character<'a>(input: &'a [u8]) -> IResult<&'a [u8], Token<'a>> {
        sequence::delimited(
            character::char('\''),
            combinator::cut(branch::alt((
                sequence::delimited(
                    character::char('\\'),
                    combinator::cut(Self::character_escape),
                    bytes::take_until("'").map_res(|s: &[u8]| {
                        s.is_empty()
                            .then_some(())
                            .ok_or(TokenError::InvalidCharLiteral)
                    }),
                ),
                bytes::take_until("'")
                    .map_res(|s: &[u8]| {
                        // utf-8 'characters' are always at most 4 bytes
                        (s.len() <= 4)
                            .then_some(s)
                            .ok_or(TokenError::InvalidCharLiteral)
                    })
                    .map_res(|s| {
                        core::str::from_utf8(s).map_err(|_| NomError {
                            input: input.get(1..).unwrap_or(&[]),
                            error: TokenError::InvalidUtf8,
                        })
                    })
                    .map_res(|s| {
                        let mut chars = s.chars();
                        let c = chars.next().ok_or(TokenError::InvalidCharLiteral)?;
                        if chars.next().is_some() {
                            return Err(TokenError::InvalidCharLiteral);
                        }
                        Ok(c)
                    }),
            ))),
            character::char('\''),
        )
        .map(|c| Token::Value(Value::Char(c)))
        .parse(input)
    }

    fn token<'a>(input: &'a [u8]) -> IResult<&'a [u8], Token<'a>> {
        let (i, tok) = sequence::terminated(
            branch::alt((
                Self::newline,
                Self::comma,
                Self::unit,
                Self::symbol,
                Self::integer,
                Self::float,
                Self::character,
                Self::string,
                Self::bytes,
                Self::ident_or_literal,
            )),
            Self::whitespace0,
        )
        .parse(input)?;
        assert!(
            i.len() < input.len(),
            "Tokenizer did not make forward progress"
        );
        Ok((i, tok))
    }
}

impl<'de> Iterator for Tokenizer<'de> {
    type Item = LocResult<'de, Token<'de>, TokenError>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.next_token() {
            Ok(ev) => Some(Ok(ev)),
            Err(e) if matches!(*e, TokenError::Eof) => None,
            Err(e) => Some(Err(e)),
        }
    }
}

#[cfg(test)]
mod test {
    use crate::types::Escaped;

    #[test]
    fn token_does_not_match_empty() {
        assert!(super::Tokenizer::token(&[]).is_err());
    }

    #[test]
    fn token_iterator() {
        use super::{Token, Tokenizer};
        let tokenizer = Tokenizer::new(" [ ]   ".as_ref());
        for (i, tok) in tokenizer.enumerate() {
            assert!(i < 2);
            let tok = tok.unwrap();
            if i == 0 {
                assert_eq!(*tok, Token::ListOpen);
            } else {
                assert_eq!(*tok, Token::ListClose);
            }
        }
    }

    #[test]
    fn token_iterator_error() {
        use super::{Token, TokenError, Tokenizer};
        let tokenizer = Tokenizer::new(" [ ?   ".as_ref());
        for (i, tok) in tokenizer.enumerate() {
            if i == 0 {
                assert_eq!(*tok.unwrap(), Token::ListOpen);
            } else {
                assert_eq!(*tok.unwrap_err(), TokenError::UnknownToken);
                if i > 5 {
                    break;
                }
            }
        }
    }

    // parse and check expected tokens
    macro_rules! token_test {
        ($(#[$attr:meta])* $name:ident, $src:literal $(,$tok:expr)* $(,)?) => {
            #[test]
            $(#[$attr])*
            fn $name() {
                #[allow(unused)]
                use super::{Value::*, Token, Token::*, Tokenizer, TokenError};
                let tokens: &[Token] = &[$($tok,)*];
                let mut tokenizer = Tokenizer::new($src.as_ref());
                for tok in tokens {
                    assert!(!tokenizer.is_eof());
                    assert_eq!(*tok, *tokenizer.next_token().unwrap());
                }

                assert!(tokenizer.is_eof());
                assert_eq!(TokenError::Eof, *tokenizer.next_token().unwrap_err());
            }
        }
    }

    // parse all tokens, don't check them (but check errors)
    macro_rules! any_tokens_test {
        ($(#[$attr:meta])* $name:ident, $src:literal $(,)?) => {
            #[test]
            $(#[$attr])*
            fn $name() {
                #[allow(unused)]
                use super::{Value::*, Token, Token::*, Tokenizer, TokenError};
                let mut tokenizer = Tokenizer::new($src.as_ref());
                while !tokenizer.is_eof() {
                    tokenizer.next_token().unwrap();
                }
                assert!(tokenizer.is_eof());
                assert_eq!(TokenError::Eof, *tokenizer.next_token().unwrap_err());
            }
        }
    }

    // just newlines get eaten at start
    token_test!(newline, "\n\n\n");

    any_tokens_test!(
        #[should_panic(expected = "UnknownToken")]
        invalid_utf8,
        // invalid unicode outside a string is a bad token
        b"\n\n\xf0\n"
    );
    any_tokens_test!(
        #[should_panic(expected = "InvalidUtf8")]
        invalid_utf8_in_string,
        // invalid unicode inside a string is bad utf-8
        b"\n\n\"\xf0\"\n"
    );
    any_tokens_test!(
        #[should_panic(expected = "InvalidUtf8")]
        invalid_utf8_in_char,
        // invalid unicode inside a character is bad utf-8
        b"\n\n\'\xf0\'\n"
    );
    // invalid unicode in a bytes literal is fine
    any_tokens_test!(invalid_utf8_in_bytes, b"\n\nb\"\xf0\"\n");

    token_test!(comma_newline, "\n\n  \n    ,   \n \n", Comma, Newline);
    token_test!(equals, "    =   ", Equals);
    token_test!(paren_open, "  \n  (      ", ParenOpen);
    token_test!(paren_close, "  \n   )   ", ParenClose);
    token_test!(list_open, "  \n  [      ", ListOpen);
    token_test!(list_close, "  \n   ]   ", ListClose);
    token_test!(list_empty, "  \n [  ]   ", ListOpen, ListClose);
    token_test!(map_open, "  \n  {      ", MapOpen);
    token_test!(map_close, "  \n   }   ", MapClose);
    token_test!(map_empty, "  \n {  }   ", MapOpen, MapClose);

    token_test!(comment, "  #  this is a comment \n ");
    token_test!(comment_comma, "  #  this is a comment \n ,  ", Comma);
    token_test!(
        comment_tok_newline_tok,
        " [ # com \n ] ",
        ListOpen,
        Newline,
        ListClose,
    );

    token_test!(ident, "   \n   ident", Ident("ident"));
    token_test!(ident_true, "   \n   \\true  ", Ident("true"));
    token_test!(ident_false, "   \n   \\false  ", Ident("false"));
    token_test!(ident_none, "   \n   \\none  ", Ident("none"));
    token_test!(
        #[should_panic(expected = "UnknownToken")]
        ident_number,
        // idents should not start with numbers
        "   \n   0ident  ",
        Ident("0ident")
    );
    token_test!(
        ident_number_escaped,
        // escaped is fine to start with number
        "   \n   \\0ident  ",
        Ident("0ident")
    );

    token_test!(val_unit, "   \n   ()   ", Value(Unit));
    token_test!(val_unit_space, "   \n   (      )   ", Value(Unit));
    token_test!(
        paren_newline_paren,
        // anything but spaces between parens makes it not a unit
        "   \n   (   \n  )   ",
        ParenOpen,
        Newline,
        ParenClose,
    );

    token_test!(val_true, "   \n   true   ", Value(Bool(true)));
    token_test!(val_false, "   \n   false   ", Value(Bool(false)));
    token_test!(val_none, "   \n   none   ", Value(None));

    token_test!(
        val_ints,
        "   \n  10 -20 +30",
        Value(Int(10)),
        Value(Int(-20)),
        Value(Int(30)),
    );
    token_test!(
        val_ints_hex,
        "   \n  0x10 -0x20 +0x30",
        Value(Int(0x10)),
        Value(Int(-0x20)),
        Value(Int(0x30)),
    );
    token_test!(
        val_ints_oct,
        "   \n  0o10 -0o20 +0o30",
        Value(Int(0o10)),
        Value(Int(-0o20)),
        Value(Int(0o30)),
    );
    token_test!(
        val_ints_bin,
        "   \n  0b10 -0b11 +0b111",
        Value(Int(0b10)),
        Value(Int(-0b11)),
        Value(Int(0b111)),
    );
    token_test!(
        val_ints_limit,
        // max and min for Int
        "   \n  0x7fffffffffffffff -0x8000000000000000",
        Value(Int(crate::syntax::Int::MAX)),
        Value(Int(crate::syntax::Int::MIN)),
    );
    any_tokens_test!(
        #[should_panic(expected = "IntRange")]
        val_int_huge,
        // too big to fit (96 bits)
        "   \n  0xffffffffffffffffffffffff ",
    );

    token_test!(
        val_floats,
        "  \n  1.2 -1.3 +1.4",
        Value(Float(1.2)),
        Value(Float(-1.3)),
        Value(Float(1.4)),
    );

    token_test!(
        val_floats_exp,
        "  \n  1.2e1 -1.3E+3 +1.4e-2",
        Value(Float(1.2e1)),
        Value(Float(-1.3e+3)),
        Value(Float(1.4e-2)),
    );

    token_test!(
        val_chars,
        r#"      'a' 'A' '\u{2603}' '🄯'"#,
        Value(Char('a')),
        Value(Char('A')),
        Value(Char('☃')),
        Value(Char('🄯')),
    );
    token_test!(
        #[should_panic(expected = "UnknownEscape")]
        val_chars_escapes_invalid,
        // \xf0 is not valid utf-8
        r#"      'a' 'A' '\xf0' '🄯'"#,
        Value(Char('a')),
        Value(Char('A')),
        Value(Char('?')),
        Value(Char('🄯')),
    );
    any_tokens_test!(
        #[should_panic(expected = "InvalidCharLiteral")]
        val_char_empty,
        "      '' "
    );
    any_tokens_test!(
        #[should_panic(expected = "InvalidCharLiteral")]
        val_char_too_many,
        "      'aa' "
    );
    any_tokens_test!(
        #[should_panic(expected = "InvalidCharLiteral")]
        val_char_way_too_many,
        "      'aaaaa' "
    );

    token_test!(
        val_string,
        r#"  "hello"    "there"    "#,
        Value(Str(Escaped::new("hello").unwrap())),
        Value(Str(Escaped::new("there").unwrap())),
    );
    token_test!(
        val_string_escapes,
        r#"  "hel\n\r\tlo"    "the\\\0\"\'re"    "#,
        Value(Str(Escaped::new("hel\\n\\r\\tlo").unwrap())),
        Value(Str(Escaped::new("the\\\\\\0\\\"\\\'re").unwrap())),
    );
    token_test!(
        val_string_escapes_num,
        r#"  "hel\x42lo"    "the\u{1234}re"    "#,
        Value(Str(Escaped::new("hel\\x42lo").unwrap())),
        Value(Str(Escaped::new("the\\u{1234}re").unwrap())),
    );
    token_test!(
        #[should_panic(expected = "UnknownEscape")]
        val_string_escapes_invalid,
        // \xf2 is not valid utf-8
        r#"  b"hel\xf2lo"    b"the\u{1234}re"    "#,
        Value(Bytes(Escaped::new(&b"hel\\xf2lo"[..]).unwrap())),
        Value(Bytes(Escaped::new(&b"the\\u{1234}12re"[..]).unwrap())),
    );
    token_test!(
        val_string_multiline,
        r#"  "hel
lo"    "the
re"    "#,
        Value(Str(Escaped::new("hel\nlo").unwrap())),
        Value(Str(Escaped::new("the\nre").unwrap())),
    );

    token_test!(
        val_bytes,
        r#"  b"hello"    b"there"    "#,
        Value(Bytes(Escaped::new(&b"hello"[..]).unwrap())),
        Value(Bytes(Escaped::new(&b"there"[..]).unwrap())),
    );
    token_test!(
        val_bytes_escapes,
        r#"  b"hel\n\r\tlo"    b"the\\\0\"\'re"    "#,
        Value(Bytes(Escaped::new(&b"hel\\n\\r\\tlo"[..]).unwrap())),
        Value(Bytes(Escaped::new(&b"the\\\\\\0\\\"\\\'re"[..]).unwrap())),
    );
    token_test!(
        val_bytes_escapes_num,
        r#"  b"hel\x42lo"    b"the\x12re"    "#,
        Value(Bytes(Escaped::new(&b"hel\\x42lo"[..]).unwrap())),
        Value(Bytes(Escaped::new(&b"the\\x12re"[..]).unwrap())),
    );
    token_test!(
        #[should_panic(expected = "UnknownEscape")]
        val_bytes_escapes_no_unicode,
        // \u{...} not allowed in bytes
        r#"  b"hel\x42lo"    b"the\u{1234}re"    "#,
        Value(Bytes(Escaped::new(&b"hel\\x42lo"[..]).unwrap())),
        Value(Bytes(Escaped::new(&b"the\\u{1234}12re"[..]).unwrap())),
    );
    token_test!(
        val_bytes_multiline,
        r#"  b"hel
lo"    b"the
re"    "#,
        Value(Bytes(Escaped::new(&b"hel\nlo"[..]).unwrap())),
        Value(Bytes(Escaped::new(&b"the\nre"[..]).unwrap())),
    );
}
