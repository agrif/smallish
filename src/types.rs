//! Special types to control behavior.
//!
//! This module contains a few special types to control how values are
//! deserialized. They also often have a concrete meaning that makes
//! them useful elsewhere.
//!
//! ## Escaping
//!
//! *smallish* strings and bytes support the same escapes as
//! Rust. However, it needs a scratch buffer to parse strings with
//! escapes. This can be done with the [from_slice_escaped][] and
//! [from_str_escaped][] functions, or more directly with
//! [Deserializer][].
//!
//!  [from_str_escaped]: crate::from_str_escaped
//!  [from_slice_escaped]: crate::from_slice_escaped
//!  [Deserializer]: crate::de::Deserializer
//!
//! It is also possible to opt-out of unescaping by wrapping a string
//! type in [Escaped]. This deserializes the string unmodified, with
//! escapes still intact, at which point you can choose to unescape it
//! manually or use it as-is.
//!
//! ## Locating Values and Errors
//!
//! You can have *smallish* attach a source location to any value in
//! your type by wrapping it in [Located]. This can be helpful to
//! point humans to where an error occurred, for example.
//!
//! Errors produced by *smallish* itself are always wrapped in
//! [Located]. Some effort has gone into making them useful to humans
//! even in an embedded context.

mod escaped;
mod location;

pub use escaped::{Escapable, Escaped, EscapedFragment, UnescapeError};
pub use location::{LocResult, Located};
