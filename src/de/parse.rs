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
        }
    }

    /// Return the location of the event to be parsed next.
    pub fn location(&self) -> &Located<'de, ()> {
        self.tokens.location()
    }

    /// Returns `true` if there is no input left.
    pub fn is_eof(&self) -> bool {
        self.tokens.is_eof()
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
