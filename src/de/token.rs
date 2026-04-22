use nom::{
    branch, bytes::complete as bytes, character::complete as character, combinator, error, multi,
    number::complete as number, sequence, Parser,
};

use crate::syntax::{Integer, Token, Value};
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
    /// There is invalid utf-8 inside a string literal.
    #[error("invalid utf-8")]
    InvalidUtf8,
    /// There is an invalid escape sequence inside a string or bytes literal.
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
    pub fn location(&self) -> &Located<'de, ()> {
        &self.location
    }

    /// Return `true` if there is no more input left.
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

    /// Parse and return the next token in the stream.
    pub fn next(&mut self) -> LocResult<'de, Token<'de>, TokenError> {
        self.parse(Self::token)
    }

    /// Parse and return the next token in the stream, without consuming it.
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
                |s: &[u8]| !s.get(0).map(u8::is_ascii_digit).unwrap_or(true),
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
            sequence::terminated(Self::unit, Self::whitespace0),
            sequence::terminated(Self::symbol, Self::whitespace0),
            sequence::terminated(Self::integer, Self::whitespace0),
            sequence::terminated(Self::float, Self::whitespace0),
            sequence::terminated(Self::character, Self::whitespace0),
            sequence::terminated(Self::string, Self::whitespace0),
            sequence::terminated(Self::bytes, Self::whitespace0),
            sequence::terminated(Self::ident_or_literal, Self::whitespace0),
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

#[cfg(test)]
mod test {
    extern crate std;

    use crate::types::Escaped;

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
                    assert_eq!(*tok, *tokenizer.peek().unwrap());
                    assert_eq!(*tok, *tokenizer.next().unwrap());
                }

                assert!(tokenizer.is_eof());
                assert_eq!(TokenError::Eof, *tokenizer.peek().unwrap_err());
                assert_eq!(TokenError::Eof, *tokenizer.next().unwrap_err());
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
                    tokenizer.next().unwrap();
                }
                assert!(tokenizer.is_eof());
                assert_eq!(TokenError::Eof, *tokenizer.peek().unwrap_err());
                assert_eq!(TokenError::Eof, *tokenizer.next().unwrap_err());
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
        Value(Integer(10)),
        Value(Integer(-20)),
        Value(Integer(30)),
    );
    token_test!(
        val_ints_hex,
        "   \n  0x10 -0x20 +0x30",
        Value(Integer(0x10)),
        Value(Integer(-0x20)),
        Value(Integer(0x30)),
    );
    token_test!(
        val_ints_oct,
        "   \n  0o10 -0o20 +0o30",
        Value(Integer(0o10)),
        Value(Integer(-0o20)),
        Value(Integer(0o30)),
    );
    token_test!(
        val_ints_bin,
        "   \n  0b10 -0b11 +0b111",
        Value(Integer(0b10)),
        Value(Integer(-0b11)),
        Value(Integer(0b111)),
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
        Value(Character('a')),
        Value(Character('A')),
        Value(Character('☃')),
        Value(Character('🄯')),
    );

    token_test!(
        val_string,
        r#"  "hello"    "there"    "#,
        Value(String(Escaped::new("hello").unwrap())),
        Value(String(Escaped::new("there").unwrap())),
    );
    token_test!(
        val_string_escapes,
        r#"  "hel\n\r\tlo"    "the\\\0\"\'re"    "#,
        Value(String(Escaped::new("hel\\n\\r\\tlo").unwrap())),
        Value(String(Escaped::new("the\\\\\\0\\\"\\\'re").unwrap())),
    );
    token_test!(
        val_string_escapes_num,
        r#"  "hel\x42lo"    "the\u{1234}re"    "#,
        Value(String(Escaped::new("hel\\x42lo").unwrap())),
        Value(String(Escaped::new("the\\u{1234}re").unwrap())),
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
        Value(String(Escaped::new("hel\nlo").unwrap())),
        Value(String(Escaped::new("the\nre").unwrap())),
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
