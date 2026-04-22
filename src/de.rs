//! Low-level parsing and deserialization support.
//!
//! This module contains implementations of the [Tokenizer], [Parser],
//! and [Deserializer], and their associated error types.
//!
//! Apart from the error types, you probably do not need to use
//! anything in this module unless you run into problems with the
//! convenience functions in the root of this crate, or are doing
//! something special.

mod deserialize;
mod parse;
pub(crate) mod token;

pub use deserialize::{Deserializer, Error};
pub use parse::{ParseError, Parser, ParserState};
pub use token::{TokenError, Tokenizer};
