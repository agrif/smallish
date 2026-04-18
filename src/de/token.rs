use nom::{
    branch, bytes::complete as bytes, character::complete as character, combinator, error, multi,
    sequence, Parser,
};

use super::{LocResult, Located};
use crate::syntax::{Integer, Token, Value};

#[derive(Clone, Debug, thiserror::Error)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum TokenError {
    #[error("end of file")]
    Eof,
    #[error("unknown token")]
    UnknownToken,
    #[error("unknown escape sequence")]
    UnknownEscape,
}

#[derive(Clone, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
struct NomError<I> {
    input: I,
    error: TokenError,
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

type IResult<I, O> = nom::IResult<I, O, NomError<I>>;

#[derive(Clone, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Tokenizer<'de> {
    input: &'de str,
    location: Located<'de, ()>,
}

impl<'de> Tokenizer<'de> {
    pub fn new(input: &'de str) -> Self {
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
        P: nom::Parser<&'de str, Error = NomError<&'de str>>,
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

    fn whitespace_single(input: &str) -> IResult<&str, ()> {
        branch::alt((
            character::space1,
            sequence::preceded(character::char('#'), character::not_line_ending),
        ))
        .map(|_| ())
        .parse(input)
    }

    fn whitespace1(input: &str) -> IResult<&str, ()> {
        multi::many1_count(Self::whitespace_single)
            .map(|_| ())
            .parse(input)
    }

    fn whitespace0(input: &str) -> IResult<&str, ()> {
        multi::many0_count(Self::whitespace_single)
            .map(|_| ())
            .parse(input)
    }

    fn newlines1(input: &str) -> IResult<&str, ()> {
        multi::many1_count((character::line_ending, Self::whitespace0))
            .map(|_| ())
            .parse(input)
    }

    fn newlines0(input: &str) -> IResult<&str, ()> {
        multi::many0_count((character::line_ending, Self::whitespace0))
            .map(|_| ())
            .parse(input)
    }

    fn whitespace_and_newlines0(input: &str) -> IResult<&str, ()> {
        (Self::whitespace0, Self::newlines0)
            .map(|_| ())
            .parse(input)
    }

    fn newline<'a>(input: &'a str) -> IResult<&'a str, Token<'a>> {
        Self::newlines1.map(|_| Token::Newline).parse(input)
    }

    fn comma<'a>(input: &'a str) -> IResult<&'a str, Token<'a>> {
        sequence::preceded(character::char(','), Self::newlines0)
            .map(|_| Token::Comma)
            .parse(input)
    }

    fn symbol<'a>(input: &'a str) -> IResult<&'a str, Token<'a>> {
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

    fn token_boundary(input: &str) -> IResult<&str, ()> {
        combinator::peek(branch::alt((
            combinator::eof.map(|_| ()),
            character::line_ending.map(|_| ()),
            Self::symbol.map(|_| ()),
            Self::whitespace1,
        )))
        .parse(input)
    }

    fn ident(input: &str) -> IResult<&str, &str> {
        sequence::terminated(
            combinator::verify(
                bytes::take_while1(|c: char| c.is_ascii_alphanumeric() || c == '_'),
                |s: &str| !s.starts_with(|c: char| c.is_ascii_digit()),
            ),
            Self::token_boundary,
        )
        .parse(input)
    }

    fn integer<'a>(input: &'a str) -> IResult<&'a str, Token<'a>> {
        let (input, sign) = combinator::opt(character::one_of("-+")).parse(input)?;

        let (input, mut value) = branch::alt((
            sequence::terminated(
                sequence::preceded(
                    (character::char('0'), character::one_of("xX")),
                    character::hex_digit1.map_res(|s| Integer::from_str_radix(s, 16)),
                ),
                Self::token_boundary,
            ),
            sequence::terminated(
                sequence::preceded(
                    (character::char('0'), character::one_of("oO")),
                    character::oct_digit1.map_res(|s| Integer::from_str_radix(s, 8)),
                ),
                Self::token_boundary,
            ),
            sequence::terminated(
                sequence::preceded(
                    (character::char('0'), character::one_of("bB")),
                    character::bin_digit1.map_res(|s| Integer::from_str_radix(s, 2)),
                ),
                Self::token_boundary,
            ),
            sequence::terminated(
                character::digit1.map_res(|s| Integer::from_str_radix(s, 10)),
                Self::token_boundary,
            ),
        ))
        .parse(input)?;

        if sign.unwrap_or('+') == '-' {
            value = -value;
        }

        Ok((input, Token::Value(Value::Integer(value))))
    }

    fn float<'a>(input: &'a str) -> IResult<&'a str, Token<'a>> {
        sequence::terminated(nom::number::float(), Self::token_boundary)
            .map(|f| Token::Value(Value::Float(f)))
            .parse(input)
    }

    fn string_plain<'a>(input: &'a str) -> IResult<&'a str, ()> {
        combinator::verify(bytes::is_not("\"\\"), |s: &str| !s.is_empty())
            .map(|_| ())
            .parse(input)
    }

    fn string_escape<'a>(input: &'a str) -> IResult<&'a str, ()> {
        branch::alt((
            character::char('n').map(|_| '\n'),
            character::char('r').map(|_| '\r'),
            character::char('t').map(|_| '\t'),
            character::char('\\').map(|_| '\\'),
            character::char('0').map(|_| '\0'),
            character::char('"').map(|_| '"'),
            character::char('\'').map(|_| '\''),
            // todo: \xNN, \u{NNNN}
        ))
        .map(|_| ())
        .parse(input)
        .map_err(|e| e.map(|e: NomError<_>| e.replace(TokenError::UnknownEscape)))
    }

    fn string_chunk<'a>(input: &'a str) -> IResult<&'a str, ()> {
        branch::alt((
            Self::string_plain,
            sequence::preceded(character::char('\\'), combinator::cut(Self::string_escape)),
        ))
        .parse(input)
    }

    fn string<'a>(input: &'a str) -> IResult<&'a str, Token<'a>> {
        sequence::delimited(
            character::char('"'),
            combinator::recognize(multi::many0_count(Self::string_chunk)),
            character::char('"'),
        )
        .map(|s| Token::Value(Value::String(s)))
        .parse(input)
    }

    fn token<'a>(input: &'a str) -> IResult<&'a str, Token<'a>> {
        branch::alt((
            sequence::terminated(Self::newline, Self::whitespace0),
            sequence::terminated(Self::comma, Self::whitespace0),
            sequence::terminated(Self::symbol, Self::whitespace0),
            sequence::terminated(Self::integer, Self::whitespace0),
            sequence::terminated(Self::float, Self::whitespace0),
            sequence::terminated(Self::string, Self::whitespace0),
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
