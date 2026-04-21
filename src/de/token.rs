use nom::{
    branch, bytes::complete as bytes, character::complete as character, combinator, error, multi,
    sequence, Parser,
};

use crate::syntax::{Integer, Token, Value};
use crate::types::{Escaped, EscapedFragment, LocResult, Located};

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum TokenError {
    #[error("end of file")]
    Eof,
    #[error("unknown token")]
    UnknownToken,
    #[error("invalid utf-8")]
    InvalidUtf8,
    #[error("unknown escape sequence")]
    UnknownEscape,
}

#[derive(Clone, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub(crate) struct NomError<I> {
    input: I,
    pub(crate) error: TokenError,
}

impl<I> NomError<I> {
    fn replace(self, error: TokenError) -> Self {
        Self { error, ..self }
    }
}

impl<I> error::ParseError<I> for NomError<I> {
    fn from_error_kind(input: I, kind: error::ErrorKind) -> Self {
        Self {
            input,
            error: match kind {
                error::ErrorKind::Eof => TokenError::Eof,
                _ => TokenError::UnknownToken,
            },
        }
    }

    fn append(_input: I, _kind: error::ErrorKind, other: Self) -> Self {
        other
    }
}

impl<I, E> error::FromExternalError<I, E> for NomError<I> {
    fn from_external_error(input: I, kind: error::ErrorKind, _err: E) -> Self {
        use error::ParseError;
        Self::from_error_kind(input, kind)
    }
}

pub(crate) type IResult<I, O> = nom::IResult<I, O, NomError<I>>;

#[derive(Clone, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Tokenizer<'de> {
    input: &'de [u8],
    location: Located<'de, ()>,
}

impl<'de> Tokenizer<'de> {
    pub fn new(input: &'de [u8]) -> Self {
        let mut tokenizer = Self {
            input,
            location: Located::new().with_source(Some(input)),
        };
        let _ = tokenizer.parse(Self::whitespace_and_newlines0);
        tokenizer
    }

    pub fn location(&self) -> &Located<'de, ()> {
        &self.location
    }

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
            Err(nom::Err::Incomplete(_)) => Err(self.location.wrap(TokenError::UnknownToken)),
            Err(nom::Err::Error(e)) => {
                // errors are recoverable and should point to token start
                let r = self.location.wrap(e.error);
                self.location.advance(self.input, e.input);
                Err(r)
            }
            Err(nom::Err::Failure(e)) => {
                // failures are specific to exactly where they failed
                self.location.advance(self.input, e.input);
                Err(self.location.wrap(e.error))
            }
        }
    }

    pub fn next(&mut self) -> LocResult<'de, Token<'de>, TokenError> {
        self.parse(Self::token)
    }

    pub fn peek(&mut self) -> LocResult<'de, Token<'de>, TokenError> {
        self.parse(combinator::peek(Self::token))
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

    fn ident(input: &[u8]) -> IResult<&[u8], &str> {
        sequence::terminated(
            combinator::verify(
                bytes::take_while1(|c: u8| c.is_ascii_alphanumeric() || c == b'_'),
                |s: &[u8]| !s.get(0).map(u8::is_ascii_digit).unwrap_or(true),
            ),
            Self::token_boundary,
        )
        // safety: the above parser only matches valid ascii
        .map(|s| unsafe { core::str::from_utf8_unchecked(s) })
        .parse(input)
    }

    fn integer<'a>(input: &'a [u8]) -> IResult<&'a [u8], Token<'a>> {
        let (input, sign) = combinator::opt(character::one_of("-+")).parse(input)?;

        let (input, mut value) = branch::alt((
            sequence::terminated(
                sequence::preceded(
                    (character::char('0'), character::one_of("xX")),
                    character::hex_digit1.map(|s| (s, 16)),
                ),
                Self::token_boundary,
            ),
            sequence::terminated(
                sequence::preceded(
                    (character::char('0'), character::one_of("oO")),
                    character::oct_digit1.map(|s| (s, 8)),
                ),
                Self::token_boundary,
            ),
            sequence::terminated(
                sequence::preceded(
                    (character::char('0'), character::one_of("bB")),
                    character::bin_digit1.map(|s| (s, 2)),
                ),
                Self::token_boundary,
            ),
            sequence::terminated(character::digit1.map(|s| (s, 10)), Self::token_boundary),
        ))
        .map_res(|(s, radix)| {
            // safety: the above only matches valid ascii
            let s = unsafe { core::str::from_utf8_unchecked(s) };
            Integer::from_str_radix(s, radix)
        })
        .parse(input)?;

        if sign.unwrap_or('+') == '-' {
            value = -value;
        }

        Ok((input, Token::Value(Value::Integer(value))))
    }

    fn float<'a>(input: &'a [u8]) -> IResult<&'a [u8], Token<'a>> {
        sequence::terminated(nom::number::float(), Self::token_boundary)
            .map(|f| Token::Value(Value::Float(f)))
            .parse(input)
    }

    fn string_plain<'a>(input: &'a [u8]) -> IResult<&'a [u8], EscapedFragment<&'a str, char>> {
        Self::parse_utf8(combinator::verify(bytes::is_not("\"\\"), |s: &[u8]| {
            !s.is_empty()
        }))
        .map(EscapedFragment::Slice)
        .parse(input)
    }

    fn character_escape<'a>(input: &'a [u8]) -> IResult<&'a [u8], char> {
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
                    character::satisfy(|c| c >= '0' && c <= '7'),
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
        ))
        .parse(input)
    }

    fn string_escape<'a>(input: &'a [u8]) -> IResult<&'a [u8], EscapedFragment<&'a str, char>> {
        Self::character_escape
            .map(EscapedFragment::Item)
            .parse(input)
            .map_err(|e| e.map(|e: NomError<_>| e.replace(TokenError::UnknownEscape)))
    }

    pub(crate) fn string_chunk<'a>(
        input: &'a [u8],
    ) -> IResult<&'a [u8], EscapedFragment<&'a str, char>> {
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
            Token::Value(Value::String(Escaped::new_unchecked(
                // safety: the string parsers already check for utf-8
                unsafe { core::str::from_utf8_unchecked(s) },
            )))
        })
        .parse(input)
    }

    fn bytes_plain<'a>(input: &'a [u8]) -> IResult<&'a [u8], EscapedFragment<&'a [u8], u8>> {
        combinator::verify(bytes::is_not("\"\\"), |s: &[u8]| !s.is_empty())
            .map(EscapedFragment::Slice)
            .parse(input)
    }

    fn single_byte_escape<'a>(input: &'a [u8]) -> IResult<&'a [u8], u8> {
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
        ))
        .parse(input)
    }

    fn bytes_escape<'a>(input: &'a [u8]) -> IResult<&'a [u8], EscapedFragment<&'a [u8], u8>> {
        Self::single_byte_escape
            .map(EscapedFragment::Item)
            .parse(input)
            .map_err(|e| e.map(|e: NomError<_>| e.replace(TokenError::UnknownEscape)))
    }

    pub(crate) fn bytes_chunk<'a>(
        input: &'a [u8],
    ) -> IResult<&'a [u8], EscapedFragment<&'a [u8], u8>> {
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
                bytes::is_not("'\\").map_opt(|s: &[u8]| {
                    if s.len() > 4 {
                        return None;
                    }
                    let s = core::str::from_utf8(s).ok()?;
                    if s.chars().count() != 1 {
                        return None;
                    }
                    s.chars().next()
                }),
                sequence::preceded(character::char('\\'), Self::character_escape),
            ))),
            character::char('\''),
        )
        .map(|c| Token::Value(Value::Character(c)))
        .parse(input)
    }

    fn token<'a>(input: &'a [u8]) -> IResult<&'a [u8], Token<'a>> {
        branch::alt((
            sequence::terminated(Self::newline, Self::whitespace0),
            sequence::terminated(Self::comma, Self::whitespace0),
            sequence::terminated(Self::symbol, Self::whitespace0),
            sequence::terminated(Self::integer, Self::whitespace0),
            sequence::terminated(Self::float, Self::whitespace0),
            sequence::terminated(Self::character, Self::whitespace0),
            sequence::terminated(Self::string, Self::whitespace0),
            sequence::terminated(Self::bytes, Self::whitespace0),
            sequence::terminated(Self::ident, Self::whitespace0).map(|id| match id {
                "null" => Token::Value(Value::Null),
                "true" => Token::Value(Value::Bool(true)),
                "false" => Token::Value(Value::Bool(false)),
                _ => Token::Ident(id),
            }),
        ))
        .parse(input)
    }
}

impl<'de> Iterator for Tokenizer<'de> {
    type Item = LocResult<'de, Token<'de>, TokenError>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.next() {
            Ok(ev) => Some(Ok(ev)),
            Err(e) if matches!(*e, TokenError::Eof) => None,
            Err(e) => Some(Err(e)),
        }
    }
}
