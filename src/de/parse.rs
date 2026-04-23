use super::{TokenError, Tokenizer};
use crate::fmt::FormatIter;
use crate::syntax::{Event, Token, TokenKind};
use crate::types::{LocResult, Located};
use crate::Flavor;

/// Errors produced by [Parser].
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum ParseError {
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
    /// The parser found a token it did not expect in this context.
    #[error("unexpected {0}, expected one of {choices}", choices=FormatIter::new(.1.iter(), ", "))]
    UnexpectedToken(TokenKind, &'static [TokenKind]),
    /// The parser ran out of `state`.
    #[error("maximum recursion limit exceeded")]
    MaxRecursion,
    /// There are unclosed or mismatched braces in the source.
    #[error("unmatched braces")]
    UnmatchedBraces,
    /// There is an incomplete `key=value` pair.
    #[error("incomplete field")]
    IncompleteField,
}

impl From<TokenError> for ParseError {
    fn from(err: TokenError) -> Self {
        match err {
            TokenError::Eof => Self::Eof,
            TokenError::UnknownToken => Self::UnknownToken,
            TokenError::InvalidUtf8 => Self::InvalidUtf8,
            TokenError::UnknownEscape => Self::UnknownEscape,
        }
    }
}

impl<'de> From<Located<'de, TokenError>> for Located<'de, ParseError> {
    fn from(other: Located<'de, TokenError>) -> Self {
        other.map(Into::into)
    }
}

/// Opaque struct to store state for [Parser].
///
/// This is used to allocate storage of the correct size for
/// [Parser]. See [Parser::new] for details.
#[derive(Clone, Copy, Default)]
pub struct ParserState(State);

impl ParserState {
    /// Return a zeroed state, suitable for initializing statics.
    pub const fn zero() -> Self {
        Self(
            // Use all zeros so this can be placed in bss if needed.
            // This is never used without being initialized first.
            State::Value,
        )
    }
}

#[derive(Clone, Copy, Debug, Default)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[repr(u8)]
enum State {
    #[default]
    // should have value 0 so it can fit in bss
    Value = 0,
    ValueClose,
    FieldEquals,
    FieldValue,
    BareEnum,
    ListItem,
    ListSep,
    MapItem,
    MapSep,
    Enum,
}

/// Turn source into a stream of [Events](Event).
///
/// This type implements [Iterator], and can be used in a `for` loop.
#[derive(Clone, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Parser<'de, S> {
    tokens: Tokenizer<'de>,
    initial_state: State,
    state: S,
    state_top: usize,
    unused_token: Option<Located<'de, Token<'de>>>,
    initial_state_sent: bool,
    unused_event: Option<LocResult<'de, Event<'de>, ParseError>>,
}

impl<'de, S> Parser<'de, S>
where
    S: AsRef<[ParserState]> + AsMut<[ParserState]>,
{
    /// Create a `flavor`-flavored parser operating on the given `input`.
    ///
    /// The `state` argument should be pointer to an array of
    /// [ParserState], for example a mutable slice or non-empty
    /// `Vec`. The size of this array correlates with how
    /// deeply-nested this parser can go before it returns
    /// [ParseError::MaxRecursion]. On average, it needs two elements
    /// per nested value.
    ///
    /// It is safe to re-use this buffer without zeroing it before use.
    pub fn new(flavor: Flavor, input: &'de [u8], state: S) -> Self {
        let initial_state = match flavor {
            Flavor::Value => State::Value,
            Flavor::List => State::ListItem,
            Flavor::Map => State::MapItem,
        };

        Self {
            tokens: Tokenizer::new(input),
            initial_state,
            state: state,
            state_top: 0,
            unused_token: None,
            initial_state_sent: false,
            unused_event: None,
        }
    }

    /// Return the location of the event to be parsed next.
    pub fn location(&self) -> Located<'de, ()> {
        if let Some(ev) = self.unused_event {
            match ev {
                Ok(v) => v.wrap(()),
                Err(e) => e.wrap(()),
            }
        } else {
            self.tokens.location()
        }
    }

    /// Returns `true` if and only if there is no input left.
    ///
    /// This is `true` if and only if the next event will be
    /// `ParseError::Eof`.
    pub fn is_eof(&mut self) -> bool {
        let decide = |ev: &LocResult<Event, ParseError>| match ev {
            Ok(_) => false,
            Err(e) => matches!(**e, ParseError::Eof),
        };

        if let Some(ev) = self.unused_event.as_ref() {
            return decide(ev);
        }

        let ev = self.next();
        let eof = decide(&ev);
        self.unused_event = Some(ev);
        eof
    }

    fn unexpected(
        &self,
        t: Token<'de>,
        expected: &'static [TokenKind],
    ) -> Result<Option<Event<'de>>, ParseError> {
        Err(ParseError::UnexpectedToken(t.kind(), expected))
    }

    fn only_stack_state(&self) -> Option<State> {
        self.state_top
            .checked_sub(1)
            .and_then(|i| self.state.as_ref().get(i))
            .copied()
            .map(|s| s.0)
    }

    fn state(&self) -> State {
        self.state_top
            .checked_sub(1)
            .and_then(|i| self.state.as_ref().get(i))
            .map(|s| s.0)
            .unwrap_or(self.initial_state)
    }

    fn transition(&mut self, state: State) {
        if let Some(dest) = self
            .state_top
            .checked_sub(1)
            .and_then(|i| self.state.as_mut().get_mut(i))
        {
            dest.0 = state;
        } else {
            self.initial_state = state;
        }
    }

    fn push(&mut self, state: State) -> Result<(), ParseError> {
        if self.state_top < self.state.as_ref().len() {
            self.state.as_mut()[self.state_top] = ParserState(state);
            self.state_top += 1;
            Ok(())
        } else {
            Err(ParseError::MaxRecursion)
        }
    }

    fn pop(&mut self) -> Result<(), ParseError> {
        if self.state_top > 0 {
            self.state_top -= 1;
            Ok(())
        } else {
            Err(ParseError::UnmatchedBraces)
        }
    }

    fn step(
        &mut self,
        loc: &Located<'de, ()>,
        tok: Token<'de>,
    ) -> Result<Option<Event<'de>>, ParseError> {
        use TokenKind::*;

        match self.state() {
            State::Value => match tok {
                Token::Newline => Ok(None),
                Token::ParenOpen => {
                    self.transition(State::ValueClose);
                    self.push(State::Value)?;
                    Ok(None)
                }
                Token::ListOpen => {
                    self.transition(State::ValueClose);
                    self.push(State::ListItem)?;
                    Ok(Some(Event::ListOpen))
                }
                Token::MapOpen => {
                    self.transition(State::ValueClose);
                    self.push(State::MapItem)?;
                    Ok(Some(Event::MapOpen))
                }
                Token::Ident(name) => {
                    self.transition(State::ValueClose);
                    self.push(State::Enum)?;
                    Ok(Some(Event::EnumOpen(name)))
                }
                Token::Value(v) => {
                    self.transition(State::ValueClose);
                    Ok(Some(Event::Value(v)))
                }
                t => self.unexpected(t, &[Newline, ParenOpen, ListOpen, MapOpen, Ident, Value]),
            },

            State::ValueClose => match tok {
                Token::Newline => Ok(None),
                Token::ParenClose => {
                    self.pop()?;
                    Ok(None)
                }
                t => self.unexpected(t, &[Newline, ParenClose]),
            },

            State::FieldEquals => match tok {
                Token::Equals => {
                    self.transition(State::FieldValue);
                    Ok(None)
                }
                t => self.unexpected(t, &[Equals]),
            },

            State::FieldValue => match tok {
                Token::ParenOpen => {
                    self.pop()?;
                    self.push(State::Value)?;
                    Ok(None)
                }
                Token::ListOpen => {
                    self.pop()?;
                    self.push(State::ListItem)?;
                    Ok(Some(Event::ListOpen))
                }
                Token::MapOpen => {
                    self.pop()?;
                    self.push(State::MapItem)?;
                    Ok(Some(Event::MapOpen))
                }
                Token::Ident(name) => {
                    self.pop()?;
                    // careful: enum parents can only support bare enums here
                    // but full enums are okay in maps
                    if matches!(self.state(), State::Enum) {
                        self.push(State::BareEnum)?;
                    } else {
                        self.push(State::Enum)?;
                    }
                    Ok(Some(Event::EnumOpen(name)))
                }
                Token::Value(v) => {
                    self.pop()?;
                    Ok(Some(Event::Value(v)))
                }
                t => self.unexpected(t, &[ParenOpen, ListOpen, MapOpen, Ident, Value]),
            },

            State::BareEnum => match tok {
                _ => {
                    self.pop()?;
                    self.unused_token = Some(loc.wrap(tok));
                    Ok(Some(Event::EnumClose))
                }
            },

            State::ListItem => match tok {
                Token::Newline => Ok(None),
                Token::ParenOpen => {
                    self.transition(State::ListSep);
                    self.push(State::Value)?;
                    Ok(None)
                }
                Token::ListOpen => {
                    self.transition(State::ListSep);
                    self.push(State::ListItem)?;
                    Ok(Some(Event::ListOpen))
                }
                Token::ListClose => {
                    self.pop()?;
                    Ok(Some(Event::ListClose))
                }
                Token::MapOpen => {
                    self.transition(State::ListSep);
                    self.push(State::MapItem)?;
                    Ok(Some(Event::MapOpen))
                }
                Token::Ident(name) => {
                    self.transition(State::ListSep);
                    self.push(State::Enum)?;
                    Ok(Some(Event::EnumOpen(name)))
                }
                Token::Value(v) => {
                    self.transition(State::ListSep);
                    Ok(Some(Event::Value(v)))
                }
                t => self.unexpected(
                    t,
                    &[
                        Newline, ParenOpen, ListOpen, ListClose, MapOpen, Ident, Value,
                    ],
                ),
            },

            State::ListSep => match tok {
                Token::Newline | Token::Comma => {
                    self.transition(State::ListItem);
                    Ok(None)
                }
                Token::ListClose => {
                    self.pop()?;
                    Ok(Some(Event::ListClose))
                }
                t => self.unexpected(t, &[Newline, Comma, ListClose]),
            },

            State::MapItem => match tok {
                Token::Newline => Ok(None),
                Token::MapClose => {
                    self.pop()?;
                    Ok(Some(Event::MapClose))
                }
                Token::Ident(name) => {
                    self.transition(State::MapSep);
                    self.push(State::FieldEquals)?;
                    Ok(Some(Event::Key(name)))
                }
                t => self.unexpected(t, &[Newline, MapClose, Ident]),
            },

            State::MapSep => match tok {
                Token::Newline | Token::Comma => {
                    self.transition(State::MapItem);
                    Ok(None)
                }
                Token::MapClose => {
                    self.pop()?;
                    Ok(Some(Event::MapClose))
                }
                t => self.unexpected(t, &[Newline, Comma, MapClose]),
            },

            State::Enum => match tok {
                Token::Newline
                | Token::Comma
                | Token::ParenClose
                | Token::ListClose
                | Token::MapClose => {
                    self.pop()?;
                    self.unused_token = Some(loc.wrap(tok));
                    Ok(Some(Event::EnumClose))
                }
                Token::ParenOpen => {
                    self.push(State::Value)?;
                    Ok(None)
                }
                Token::ListOpen => {
                    self.push(State::ListItem)?;
                    Ok(Some(Event::ListOpen))
                }
                Token::MapOpen => {
                    self.push(State::MapItem)?;
                    Ok(Some(Event::MapOpen))
                }
                Token::Ident(name) => {
                    let (subloc, subtok) = Located::from_result(self.tokens.next()).split();
                    match subtok {
                        // careful: ident might be a bare enum in enum context,
                        // so look for equals
                        Ok(subtok @ Token::Equals) => {
                            self.push(State::FieldEquals)?;
                            self.unused_token = Some(subloc.wrap(subtok));
                            Ok(Some(Event::Key(name)))
                        }
                        Ok(subtok) => {
                            self.push(State::BareEnum)?;
                            self.unused_token = Some(subloc.wrap(subtok));
                            Ok(Some(Event::EnumOpen(name)))
                        }
                        Err(TokenError::Eof) => {
                            self.push(State::BareEnum)?;
                            Ok(Some(Event::EnumOpen(name)))
                        }
                        Err(e) => Err(e)?,
                    }
                }
                Token::Value(v) => Ok(Some(Event::Value(v))),
                t => self.unexpected(
                    t,
                    &[
                        Newline, Comma, ParenClose, ListClose, MapClose, ParenOpen, ListOpen,
                        MapOpen, Ident, Value,
                    ],
                ),
            },
        }
    }

    /// Parses and returns the next [Event].
    pub fn next(&mut self) -> LocResult<'de, Event<'de>, ParseError> {
        let ev = self
            .unused_event
            .take()
            .unwrap_or_else(|| self.next_inner());

        // keep errors around and constantly return them once they happen
        if ev.is_err() {
            self.unused_event = Some(ev.clone());
        }

        ev
    }

    pub fn next_inner(&mut self) -> LocResult<'de, Event<'de>, ParseError> {
        if !self.initial_state_sent {
            self.initial_state_sent = true;
            match self.initial_state {
                State::ListItem | State::ListSep => {
                    return Ok(self.location().wrap(Event::ListOpen));
                }
                State::MapItem | State::MapSep => {
                    return Ok(self.location().wrap(Event::MapOpen));
                }
                _ => (),
            }
        }

        loop {
            let tok = if let Some(tok) = self.unused_token.take() {
                Ok(tok)
            } else {
                self.tokens.next()
            };
            let (loc, tok) = Located::from_result(tok).split();
            let tok = match tok {
                Ok(tok) => tok,
                Err(e) => match e.into() {
                    ParseError::Eof => {
                        let val = match self.only_stack_state() {
                            Some(state) if matches!(state, State::Enum | State::BareEnum) => {
                                let _ = self.pop();
                                Ok(Event::EnumClose)
                            }
                            Some(state)
                                if matches!(state, State::FieldEquals | State::FieldValue) =>
                            {
                                Err(ParseError::IncompleteField)
                            }
                            Some(_) => Err(ParseError::UnmatchedBraces),
                            None => match self.initial_state {
                                State::ListItem | State::ListSep => {
                                    self.initial_state = State::Value;
                                    Ok(Event::ListClose)
                                }
                                State::MapItem | State::MapSep => {
                                    self.initial_state = State::Value;
                                    Ok(Event::MapClose)
                                }
                                _ => Err(ParseError::Eof),
                            },
                        };
                        return loc.replace(val).to_result();
                    }
                    e => return loc.replace(Err(e)).to_result(),
                },
            };

            match self.step(&loc, tok) {
                Ok(None) => continue,
                Ok(Some(ev)) => return loc.replace(Ok(ev)).to_result(),
                Err(e) => return loc.replace(Err(e)).to_result(),
            }
        }
    }
}

impl<'de, S> Iterator for Parser<'de, S>
where
    S: AsRef<[ParserState]> + AsMut<[ParserState]>,
{
    type Item = LocResult<'de, Event<'de>, ParseError>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.next() {
            Ok(ev) => Some(Ok(ev)),
            Err(e) if matches!(*e, ParseError::Eof) => None,
            Err(e) => Some(Err(e)),
        }
    }
}

#[cfg(test)]
mod test {
    use crate::types::Escaped;

    // parse and check expected events
    macro_rules! parse_test {
        ($(#[$attr:meta])* $name:ident, $flavor:ident, $src:literal $(,$ev:expr)* $(,)?) => {
            #[test]
            $(#[$attr])*
            fn $name() {
                #[allow(unused)]
                use super::{Event, Event::*, Parser, ParseError, Flavor, ParserState};
                #[allow(unused)]
                use crate::syntax::Value::*;
                let events: &[Event] = &[$($ev,)*];
                let mut state = [ParserState::zero(); 64];
                let mut parser = Parser::new(Flavor::$flavor, $src.as_ref(), &mut state);
                for ev in events {
                    assert!(!parser.is_eof());
                    assert_eq!(*ev, *parser.next().unwrap());
                }

                assert!(parser.is_eof());
                assert_eq!(ParseError::Eof, *parser.next().unwrap_err());
            }
        }
    }

    // parse all events, don't check them (but check errors)
    macro_rules! parse_all_test {
        ($(#[$attr:meta])* $name:ident, $flavor:ident, $src:literal $(,)?) => {
            #[test]
            $(#[$attr])*
            fn $name() {
                #[allow(unused)]
                use super::{Event, Event::*, Parser, ParseError, Flavor, ParserState};
                #[allow(unused)]
                use crate::syntax::Value::*;
                let mut state = [ParserState::zero(); 64];
                let mut parser = Parser::new(Flavor::$flavor, $src.as_ref(), &mut state);
                while !parser.is_eof() {
                    parser.next().unwrap();
                }
                assert!(parser.is_eof());
                assert_eq!(ParseError::Eof, *parser.next().unwrap_err());
            }
        }
    }

    parse_all_test!(
        #[should_panic(expected = "UnexpectedToken")]
        bare_key,
        Value,
        "    key=0    ",
    );
    parse_all_test!(
        #[should_panic(expected = "UnexpectedToken")]
        bare_comma,
        Value,
        "    ,    ",
    );
    // newline is ok though
    parse_all_test!(bare_newline, Value, "    \n    ");

    // make sure newlines at end and beginning are ignored
    parse_test!(newline_at_beginning, Value, " \n   0  ", Value(Integer(0)));
    parse_test!(newline_at_end, Value, "   0  \n  ", Value(Integer(0)));

    parse_test!(val_unit, Value, "   ()   ", Value(Unit));
    parse_test!(val_none, Value, "   none   ", Value(None));
    parse_test!(val_true, Value, "   true   ", Value(Bool(true)));
    parse_test!(val_false, Value, "   false   ", Value(Bool(false)));
    parse_test!(val_int, Value, "   42   ", Value(Integer(42)));
    parse_test!(val_float, Value, "   42.1   ", Value(Float(42.1)));
    parse_test!(val_char, Value, "   'A'   ", Value(Character('A')));
    parse_test!(
        val_string,
        Value,
        "   \"hello\"   ",
        Value(String(Escaped::new("hello").unwrap()))
    );
    parse_test!(
        val_bytes,
        Value,
        "   b\"hello\"   ",
        Value(Bytes(Escaped::new(&b"hello"[..]).unwrap()))
    );

    parse_test!(list_empty, Value, "  []  ", ListOpen, ListClose);
    parse_test!(list_empty_space, Value, "  [   ]  ", ListOpen, ListClose);
    parse_test!(list_empty_newline, Value, "  [ \n ] ", ListOpen, ListClose);
    parse_test!(
        #[should_panic(expected = "UnexpectedToken")]
        list_empty_comma,
        Value,
        "  [ , ] ",
        ListOpen,
        ListClose, // dummy
    );
    parse_test!(
        list_simple,
        Value,
        " [1, 2, 3] ",
        ListOpen,
        Value(Integer(1)),
        Value(Integer(2)),
        Value(Integer(3)),
        ListClose,
    );
    parse_test!(
        list_trailing_comma,
        Value,
        " [1 , ] ",
        ListOpen,
        Value(Integer(1)),
        ListClose,
    );
    parse_test!(
        list_newlines,
        Value,
        " [1, \n 2 \n 3] ",
        ListOpen,
        Value(Integer(1)),
        Value(Integer(2)),
        Value(Integer(3)),
        ListClose,
    );
    parse_test!(
        list_compound,
        Value,
        " [[], {}, variant] ",
        ListOpen,
        ListOpen,
        ListClose,
        MapOpen,
        MapClose,
        EnumOpen("variant"),
        EnumClose,
        ListClose,
    );
    parse_test!(
        list_flavor_simple,
        List,
        " 1, 2, 3 ",
        ListOpen,
        Value(Integer(1)),
        Value(Integer(2)),
        Value(Integer(3)),
        ListClose,
    );
    parse_test!(
        list_flavor_trailing_comma,
        List,
        " 1 ,  ",
        ListOpen,
        Value(Integer(1)),
        ListClose,
    );
    parse_test!(
        list_flavor_newlines,
        List,
        " 1, \n 2 \n 3 ",
        ListOpen,
        Value(Integer(1)),
        Value(Integer(2)),
        Value(Integer(3)),
        ListClose,
    );
    parse_test!(
        list_flavor_compound,
        List,
        " [], {}, variant  ",
        ListOpen,
        ListOpen,
        ListClose,
        MapOpen,
        MapClose,
        EnumOpen("variant"),
        EnumClose,
        ListClose,
    );

    parse_test!(map_empty, Value, "  {}  ", MapOpen, MapClose);
    parse_test!(map_empty_space, Value, "  {   }  ", MapOpen, MapClose);
    parse_test!(map_empty_newline, Value, "  { \n } ", MapOpen, MapClose);
    parse_test!(
        #[should_panic(expected = "UnexpectedToken")]
        map_empty_comma,
        Value,
        "  { , } ",
        MapOpen,
        MapClose, // dummy
    );
    parse_test!(
        map_simple,
        Value,
        " {a=1, b =  2} ",
        MapOpen,
        Key("a"),
        Value(Integer(1)),
        Key("b"),
        Value(Integer(2)),
        MapClose,
    );
    parse_test!(
        map_trailing_comma,
        Value,
        " {a=1 , } ",
        MapOpen,
        Key("a"),
        Value(Integer(1)),
        MapClose,
    );
    parse_test!(
        #[should_panic(expected = "UnexpectedToken")]
        map_field_newline1,
        Value,
        " { a \n = 1 } ",
        MapOpen,
        Key("a"),
        Value(None), // dummy
    );
    parse_test!(
        #[should_panic(expected = "UnexpectedToken")]
        map_field_newline2,
        Value,
        " { a = \n 1 } ",
        MapOpen,
        Key("a"),
        Value(None), // dummy
    );
    parse_test!(
        map_newlines,
        Value,
        " {a = 1, \n b = 2\n c = 3 } ",
        MapOpen,
        Key("a"),
        Value(Integer(1)),
        Key("b"),
        Value(Integer(2)),
        Key("c"),
        Value(Integer(3)),
        MapClose,
    );
    parse_test!(
        map_compound,
        Value,
        " {a=[], b ={}, c= variant} ",
        MapOpen,
        Key("a"),
        ListOpen,
        ListClose,
        Key("b"),
        MapOpen,
        MapClose,
        Key("c"),
        EnumOpen("variant"),
        EnumClose,
        MapClose,
    );
    parse_test!(
        map_bare_enum,
        Value,
        " {a = enum} ",
        MapOpen,
        Key("a"),
        EnumOpen("enum"),
        EnumClose,
        MapClose,
    );
    parse_test!(
        map_bare_enum_arg,
        Value,
        " {a = enum 0} ",
        MapOpen,
        Key("a"),
        EnumOpen("enum"),
        Value(Integer(0)),
        EnumClose,
        MapClose,
    );
    parse_test!(
        map_paren_enum_arg,
        Value,
        " {a = (enum 0)} ",
        MapOpen,
        Key("a"),
        EnumOpen("enum"),
        Value(Integer(0)),
        EnumClose,
        MapClose,
    );
    parse_test!(
        map_flavor_simple,
        Map,
        " a=1, b =  2 ",
        MapOpen,
        Key("a"),
        Value(Integer(1)),
        Key("b"),
        Value(Integer(2)),
        MapClose,
    );
    parse_test!(
        map_flavor_trailing_comma,
        Map,
        " a=1 ,  ",
        MapOpen,
        Key("a"),
        Value(Integer(1)),
        MapClose,
    );
    parse_test!(
        map_flavor_newlines,
        Map,
        " a = 1, \n b = 2 \n c = 3  ",
        MapOpen,
        Key("a"),
        Value(Integer(1)),
        Key("b"),
        Value(Integer(2)),
        Key("c"),
        Value(Integer(3)),
        MapClose,
    );
    parse_test!(
        map_flavor_compound,
        Map,
        " a = [], b={}, c =variant  ",
        MapOpen,
        Key("a"),
        ListOpen,
        ListClose,
        Key("b"),
        MapOpen,
        MapClose,
        Key("c"),
        EnumOpen("variant"),
        EnumClose,
        MapClose,
    );

    parse_test!(enum_empty, Value, "  var  ", EnumOpen("var"), EnumClose);
    parse_test!(
        #[should_panic(expected = "UnexpectedToken")]
        enum_empty_comma,
        Value,
        "  var , ",
        EnumOpen("var"),
        EnumClose,
        Value(None), // dummy
    );
    parse_test!(
        enum_simple_seq,
        Value,
        " var 1 2  ",
        EnumOpen("var"),
        Value(Integer(1)),
        Value(Integer(2)),
        EnumClose,
    );
    parse_test!(
        enum_simple_map,
        Value,
        " var a=1 b=2  ",
        EnumOpen("var"),
        Key("a"),
        Value(Integer(1)),
        Key("b"),
        Value(Integer(2)),
        EnumClose,
    );
    parse_test!(
        enum_simple_mixed,
        Value,
        " var 1 b=2  ",
        EnumOpen("var"),
        Value(Integer(1)),
        Key("b"),
        Value(Integer(2)),
        EnumClose,
    );
    parse_test!(
        #[should_panic(expected = "UnexpectedToken")]
        enum_field_newline1,
        Value,
        " var a \n = 1  ",
        EnumOpen("var"),
        EnumOpen("a"),
        EnumClose,
        EnumClose,
        Value(None), // dummy
    );
    parse_test!(
        #[should_panic(expected = "UnexpectedToken")]
        enum_field_newline2,
        Value,
        " var a = \n 1  ",
        EnumOpen("var"),
        Key("a"),
        Value(None), // dummy
    );
    parse_test!(
        enum_newlines,
        Value,
        " var 1 \n ",
        EnumOpen("var"),
        Value(Integer(1)),
        EnumClose,
    );
    parse_test!(
        enum_compound,
        Value,
        " var [] {} variant ",
        EnumOpen("var"),
        ListOpen,
        ListClose,
        MapOpen,
        MapClose,
        EnumOpen("variant"),
        EnumClose,
        EnumClose,
    );
    parse_test!(
        enum_map_bare_enum,
        Value,
        " var a = enum ",
        EnumOpen("var"),
        Key("a"),
        EnumOpen("enum"),
        EnumClose,
        EnumClose,
    );
    parse_test!(
        enum_map_bare_enum_arg,
        Value,
        " var a = enum 0 ",
        EnumOpen("var"),
        Key("a"),
        EnumOpen("enum"),
        EnumClose,
        Value(Integer(0)),
        EnumClose,
    );
    parse_test!(
        enum_map_paren_enum_arg,
        Value,
        " var a = (enum 0) ",
        EnumOpen("var"),
        Key("a"),
        EnumOpen("enum"),
        Value(Integer(0)),
        EnumClose,
        EnumClose,
    );
    parse_test!(
        enum_seq_bare_enum,
        Value,
        " var enum ",
        EnumOpen("var"),
        EnumOpen("enum"),
        EnumClose,
        EnumClose,
    );
    parse_test!(
        enum_seq_bare_enum_arg,
        Value,
        " var enum 0 ",
        EnumOpen("var"),
        EnumOpen("enum"),
        EnumClose,
        Value(Integer(0)),
        EnumClose,
    );
    parse_test!(
        enum_seq_paren_enum_arg,
        Value,
        " var (enum 0) ",
        EnumOpen("var"),
        EnumOpen("enum"),
        Value(Integer(0)),
        EnumClose,
        EnumClose,
    );
    parse_test!(
        enum_variant_none,
        Value,
        " \\none ",
        EnumOpen("none"),
        EnumClose,
    );
    parse_test!(
        enum_variant_true,
        Value,
        " \\true ",
        EnumOpen("true"),
        EnumClose,
    );
    parse_test!(
        enum_variant_false,
        Value,
        " \\false ",
        EnumOpen("false"),
        EnumClose,
    );
}
