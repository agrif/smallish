use super::{LocResult, Located, Location, TokenError, Tokenizer};
use crate::syntax::{Event, Token, TokenKind};
use crate::Flavor;

#[derive(Clone, Debug, thiserror::Error)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum ParseError {
    #[error("end of file")]
    Eof,
    #[error("unknown token")]
    UnknownToken,
    #[error("unknown escape sequence")]
    UnknownEscape,
    #[error("unexpected {0:?}, expected one of {1:?}")]
    UnexpectedToken(TokenKind, &'static [TokenKind]),
    #[error("maximum recursion limit exceeded")]
    MaxRecursion,
    #[error("unmatched braces")]
    UnmatchedBraces,
    #[error("incomplete field")]
    IncompleteField,
}

impl From<TokenError> for ParseError {
    fn from(err: TokenError) -> Self {
        match err {
            TokenError::Eof => Self::Eof,
            TokenError::UnknownToken => Self::UnknownToken,
            TokenError::UnknownEscape => Self::UnknownEscape,
        }
    }
}

impl<'de> From<Located<'de, TokenError>> for Located<'de, ParseError> {
    fn from(other: Located<'de, TokenError>) -> Self {
        other.map(Into::into)
    }
}

#[derive(Clone, Copy, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct ParserState(Location, State);

impl ParserState {
    pub const fn zero() -> Self {
        Self(
            // Use all zeros so this can be placed in bss if needed.
            // This is never used without being initialized first.
            Location {
                line: 0,
                column: 0,
                offset: 0,
            },
            State::Value,
        )
    }
}

impl Default for ParserState {
    fn default() -> Self {
        Self::zero()
    }
}

#[derive(Clone, Copy, Debug, Default)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
enum State {
    #[default]
    // should have value 0 so it can fit in bss
    Value = 0,
    ValueClose,
    FieldEquals,
    FieldValue,
    FieldBareEnum,
    ListItem,
    ListSep,
    MapItem,
    MapSep,
    Enum,
}

#[derive(Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Parser<'de, 'state> {
    tokens: Tokenizer<'de>,
    initial_state: State,
    state: &'state mut [ParserState],
    state_top: usize,
    unused_token: Option<LocResult<'de, Token<'de>, TokenError>>,
    initial_state_sent: bool,
}

impl<'de, 'state> Parser<'de, 'state> {
    pub fn new<S>(flavor: Flavor, input: &'de str, state: &'state mut S) -> Self
    where
        S: AsMut<[ParserState]> + ?Sized,
    {
        let initial_state = match flavor {
            Flavor::Value => State::Value,
            Flavor::List => State::ListItem,
            Flavor::Map => State::MapItem,
        };

        Self {
            tokens: Tokenizer::new(input),
            initial_state,
            state: state.as_mut(),
            state_top: 0,
            unused_token: None,
            initial_state_sent: false,
        }
    }

    pub fn location(&self) -> &Located<'de, ()> {
        self.tokens.location()
    }

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

    fn located_state(&self) -> Option<Located<'de, State>> {
        self.state_top
            .checked_sub(1)
            .and_then(|i| self.state.get(i))
            .copied()
            .map(|ParserState(location, value)| Located {
                source: self.location().source,
                location,
                value,
            })
    }

    fn state(&self) -> State {
        self.state_top
            .checked_sub(1)
            .and_then(|i| self.state.get(i))
            .map(|s| s.1)
            .unwrap_or(self.initial_state)
    }

    fn transition(&mut self, state: State) {
        if let Some(dest) = self
            .state_top
            .checked_sub(1)
            .and_then(|i| self.state.get_mut(i))
        {
            dest.1 = state;
        } else {
            self.initial_state = state;
        }
    }

    fn push(&mut self, loc: &Located<'de, ()>, state: State) -> Result<(), ParseError> {
        if self.state_top < self.state.len() {
            self.state[self.state_top] = ParserState(loc.location, state);
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
                    self.push(loc, State::Value)?;
                    Ok(None)
                }
                Token::ListOpen => {
                    self.transition(State::ValueClose);
                    self.push(loc, State::ListItem)?;
                    Ok(Some(Event::ListOpen))
                }
                Token::MapOpen => {
                    self.transition(State::ValueClose);
                    self.push(loc, State::MapItem)?;
                    Ok(Some(Event::MapOpen))
                }
                Token::Ident(name) => {
                    self.transition(State::ValueClose);
                    self.push(loc, State::Enum)?;
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
                    self.push(loc, State::Value)?;
                    Ok(None)
                }
                Token::ListOpen => {
                    self.pop()?;
                    self.push(loc, State::ListItem)?;
                    Ok(Some(Event::ListOpen))
                }
                Token::MapOpen => {
                    self.pop()?;
                    self.push(loc, State::MapItem)?;
                    Ok(Some(Event::MapOpen))
                }
                Token::Ident(name) => {
                    self.pop()?;
                    // careful: enum parents can only support bare enums here
                    // but full enums are okay in maps
                    if matches!(self.state(), State::Enum) {
                        self.push(loc, State::FieldBareEnum)?;
                        self.unused_token = Some(Ok(loc.wrap(tok)));
                    } else {
                        self.push(loc, State::Enum)?;
                    }
                    Ok(Some(Event::EnumOpen(name)))
                }
                Token::Value(v) => {
                    self.pop()?;
                    Ok(Some(Event::Value(v)))
                }
                t => self.unexpected(t, &[ParenOpen, ListOpen, MapOpen, Ident, Value]),
            },

            State::FieldBareEnum => match tok {
                Token::Ident(_name) => {
                    self.pop()?;
                    Ok(Some(Event::EnumClose))
                }
                t => self.unexpected(t, &[Ident]),
            },

            State::ListItem => match tok {
                Token::Newline => Ok(None),
                Token::ParenOpen => {
                    self.transition(State::ListSep);
                    self.push(loc, State::Value)?;
                    Ok(None)
                }
                Token::ListOpen => {
                    self.transition(State::ListSep);
                    self.push(loc, State::ListItem)?;
                    Ok(Some(Event::ListOpen))
                }
                Token::ListClose => {
                    self.pop()?;
                    Ok(Some(Event::ListClose))
                }
                Token::MapOpen => {
                    self.transition(State::ListSep);
                    self.push(loc, State::MapItem)?;
                    Ok(Some(Event::MapOpen))
                }
                Token::Ident(name) => {
                    self.transition(State::ListSep);
                    self.push(loc, State::Enum)?;
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
                    self.push(loc, State::FieldEquals)?;
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
                    self.unused_token = Some(Ok(loc.wrap(tok)));
                    Ok(Some(Event::EnumClose))
                }
                Token::ParenOpen => {
                    self.push(loc, State::Value)?;
                    Ok(None)
                }
                Token::ListOpen => {
                    self.push(loc, State::ListItem)?;
                    Ok(Some(Event::ListOpen))
                }
                Token::MapOpen => {
                    self.push(loc, State::MapItem)?;
                    Ok(Some(Event::MapOpen))
                }
                Token::Ident(name) => {
                    self.push(loc, State::FieldEquals)?;
                    Ok(Some(Event::Key(name)))
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
                tok
            } else {
                self.tokens.next()
            };
            let (loc, tok) = Located::from_result(tok).split();
            let tok = match tok {
                Ok(tok) => tok,
                Err(e) => match e.into() {
                    ParseError::Eof => {
                        let (loc, val) = match self.located_state() {
                            Some(state) if matches!(*state, State::Enum) => {
                                let state = state.pure();
                                let _ = self.pop();
                                (state, Ok(Event::EnumClose))
                            }
                            Some(state)
                                if matches!(
                                    *state,
                                    State::FieldEquals | State::FieldValue | State::FieldBareEnum
                                ) =>
                            {
                                (state.pure(), Err(ParseError::IncompleteField))
                            }
                            Some(state) => (state.pure(), Err(ParseError::UnmatchedBraces)),
                            None => {
                                let r = match self.initial_state {
                                    State::ListItem | State::ListSep => {
                                        self.initial_state = State::Value;
                                        Ok(Event::ListClose)
                                    }
                                    State::MapItem | State::MapSep => {
                                        self.initial_state = State::Value;
                                        Ok(Event::MapClose)
                                    }
                                    _ => Err(ParseError::Eof),
                                };

                                (loc.pure(), r)
                            }
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
