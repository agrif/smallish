use super::{LocResult, Located, TokenError, Tokenizer};
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

// put this in a dedicated module so there is no accidentally using
// the private fields.
mod parser_state {
    // safety: don't derive anything that can peek inside besides Copy
    #[derive(Clone, Copy, Default)]
    // safety: transparent is important, see constructors for Parser
    #[repr(transparent)]
    pub struct ParserState {
        // safety: This is private to be inaccessible outside this module,
        // since this *will* contain lifetimes shorter than 'static in reality.
        // Use 'static here, this is transmuted to 'de inside Parser.
        // We just want it to be possible to use static buffers for this.
        state: super::Located<'static, super::State>,
    }

    impl ParserState {
        pub const fn new() -> Self {
            Self {
                // Use all zeros so this can be placed in bss if needed.
                // This is never used without being initialized first.
                state: super::Located {
                    source: None,
                    line: 0,
                    column: 0,
                    offset: 0,
                    value: super::State::Value,
                },
            }
        }
    }
}

pub use parser_state::ParserState;

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
    // safety: must have same repr as ParserState
    state: &'state mut [Located<'de, State>],
    state_top: usize,
    unused_token: Option<LocResult<'de, Token<'de>, TokenError>>,
    initial_state_sent: bool,
}

impl<'de, 'state> Drop for Parser<'de, 'state> {
    fn drop(&mut self) {
        // safety: drop all dangling pointers, just in case
        for state in self.state.iter_mut() {
            state.source = None;
        }
    }
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
            // safety: ParserState is repr(transparent), this only changes
            // a 'static into 'de. We are careful to never read anything
            // but that which we ourselves write to, and 'de outlives self.
            state: unsafe { core::mem::transmute(state.as_mut()) },
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

    fn state(&self) -> State {
        self.state_top
            .checked_sub(1)
            .and_then(|i| self.state.get(i))
            .map(|s| **s)
            .unwrap_or(self.initial_state)
    }

    fn transition(&mut self, state: State) {
        if let Some(dest) = self
            .state_top
            .checked_sub(1)
            .and_then(|i| self.state.get_mut(i))
        {
            *dest = dest.wrap(state);
        } else {
            self.initial_state = state;
        }
    }

    fn push(&mut self, loc: &Located<'de, ()>, state: State) -> Result<(), ParseError> {
        if self.state_top < self.state.len() {
            self.state[self.state_top] = loc.wrap(state);
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
                        let (loc, val) = match self
                            .state_top
                            .checked_sub(1)
                            .and_then(|i| self.state.get(i))
                        {
                            Some(state) if matches!(**state, State::Enum) => {
                                let state = state.pure();
                                let _ = self.pop();
                                (state, Ok(Event::EnumClose))
                            }
                            Some(state)
                                if matches!(
                                    **state,
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
