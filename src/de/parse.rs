use super::token::{self, Token, TokenKind, Tokenizer};
use super::{LocResult, Located};
use crate::types::Value;

#[derive(Clone, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Event<'de> {
    ListOpen,
    ListClose,
    MapOpen,
    MapClose,
    EnumOpen(&'de str),
    EnumClose,
    Key(&'de str),
    Value(Value),
}

#[derive(Clone, Debug, thiserror::Error)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Error {
    #[error("end of file")]
    Eof,
    #[error("unknown token")]
    UnknownToken,
    #[error("unexpected {0:?}, expected one of {1:?}")]
    UnexpectedToken(TokenKind, &'static [TokenKind]),
    #[error("maximum recursion limit exceeded")]
    MaxRecursion,
    #[error("unmatched braces")]
    UnmatchedBraces,
    #[error("incomplete field")]
    IncompleteField,
}

impl From<token::Error> for Error {
    fn from(err: token::Error) -> Self {
        match err {
            token::Error::Eof => Self::Eof,
            token::Error::UnknownToken => Self::UnknownToken,
        }
    }
}

#[derive(Clone, Copy, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
enum State {
    Value,
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

#[derive(Clone, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Parser<'de, const STACK: usize> {
    tokens: Tokenizer<'de>,
    initial_state: State,
    state: heapless::Vec<Located<'de, State>, STACK>,
    unused_token: Option<LocResult<'de, Token<'de>, token::Error>>,
    initial_state_sent: bool,
}

impl<'de, const STACK: usize> Parser<'de, STACK> {
    pub fn from_str(input: &'de str) -> Self {
        Self {
            tokens: Tokenizer::new(input),
            initial_state: State::Value,
            state: heapless::Vec::new(),
            unused_token: None,
            initial_state_sent: false,
        }
    }

    pub fn list_from_str(input: &'de str) -> Self {
        Self {
            initial_state: State::ListItem,
            ..Self::from_str(input)
        }
    }

    pub fn map_from_str(input: &'de str) -> Self {
        Self {
            initial_state: State::MapItem,
            ..Self::from_str(input)
        }
    }

    pub fn location(&self) -> &Located<'de, ()> {
        self.tokens.location()
    }

    pub fn is_eof(&self) -> bool {
        self.tokens.is_eof()
    }

    fn state(&self) -> State {
        self.state.last().map(|s| **s).unwrap_or(self.initial_state)
    }

    fn unexpected(
        &self,
        t: Token<'de>,
        expected: &'static [TokenKind],
    ) -> Result<Option<Event<'de>>, Error> {
        Err(Error::UnexpectedToken(t.kind(), expected))
    }

    fn transition(&mut self, state: State) {
        if let Some(dest) = self.state.last_mut() {
            *dest = dest.clone().replace(state).0;
        } else {
            self.initial_state = state;
        }
    }

    fn push(&mut self, loc: &Located<'de, ()>, state: State) -> Result<(), Error> {
        if self.state.push(loc.clone().replace(state).0).is_err() {
            Err(Error::MaxRecursion)
        } else {
            Ok(())
        }
    }

    fn pop(&mut self) -> Result<(), Error> {
        if self.state.pop().is_none() {
            Err(Error::UnmatchedBraces)
        } else {
            Ok(())
        }
    }

    fn step(
        &mut self,
        loc: &Located<'de, ()>,
        tok: Token<'de>,
    ) -> Result<Option<Event<'de>>, Error> {
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
                        self.unused_token = Some(Ok(loc.clone().replace(tok).0));
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
                t => self.unexpected(t, &[Newline, ParenOpen, ListClose, MapOpen, Ident, Value]),
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
                    self.unused_token = Some(Ok(loc.clone().replace(tok).0));
                    // FIXME emit EnumClose
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

    pub fn next(&mut self) -> LocResult<'de, Event<'de>, Error> {
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
            let (loc, tok) = Located::from_result(tok).replace(());
            let tok = match tok {
                Ok(tok) => tok,
                Err(e) => match e.into() {
                    Error::Eof => {
                        let (loc, val) = match self.state.last() {
                            Some(state) if matches!(**state, State::Enum) => {
                                let state = state.wrap(());
                                let _ = self.pop();
                                (state, Ok(Event::EnumClose))
                            }
                            Some(state)
                                if matches!(
                                    **state,
                                    State::FieldEquals | State::FieldValue | State::FieldBareEnum
                                ) =>
                            {
                                (state.wrap(()), Err(Error::IncompleteField))
                            }
                            Some(state) => (state.wrap(()), Err(Error::UnmatchedBraces)),
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
                                    _ => Err(Error::Eof),
                                };

                                (loc.wrap(()), r)
                            }
                        };
                        return loc.replace(val).0.to_result();
                    }
                    e => return loc.replace(Err(e)).0.to_result(),
                },
            };

            match self.step(&loc, tok) {
                Ok(None) => continue,
                Ok(Some(ev)) => return loc.replace(Ok(ev)).0.to_result(),
                Err(e) => return loc.replace(Err(e)).0.to_result(),
            }
        }
    }
}
