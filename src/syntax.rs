//! Syntax documentation and common types to describe syntax.
//!
//! ## Basic Types
//!
//! *smallish* syntax is designed to be simple for humans to write,
//! and to get out of the way of its main purpose: writing lists of
//! simple instructions.
//!
//! This informs the most common syntax:
//!
//! * **Lists** are values enclosed in square brackets `[]`, separated
//!   by either a comma or newlines.
//!
//!   `[0, 1, 2]`
//!
//!   When using list-flavored *smallish*, the brackets around the
//!   root list are omitted.
//!
//! * **Enumerations** are the variant name followed by arguments,
//!   separated by spaces. Tuple variants use the values directly, while
//!   struct variants use key-value pairs.
//!
//!   `VariantName key=42`
//!
//! * **Comments** begin with `#` and continue until the end of the line.
//!
//!   `# this is a comment`
//!
//! For example:
//!
//! ```
//! # use smallish::{Flavor, from_str};
//! #[derive(Debug, PartialEq, Eq, serde::Deserialize)]
//! enum Instr {
//!     UnitVariant,
//!     TupleVariant(u8, u8, u8),
//!     StructVariant { foo: u8, bar: Vec<u8> },
//! }
//!
//! let r: Vec<Instr> = from_str(Flavor::List, r#"
//! UnitVariant # comments!
//! TupleVariant 0 1 2
//! StructVariant foo=42 bar=[10, 20]
//! "#).unwrap();
//!
//! assert_eq!(r, vec![
//!     Instr::UnitVariant,
//!     Instr::TupleVariant(0, 1, 2),
//!     Instr::StructVariant { foo: 42, bar: vec![10, 20] },
//! ]);
//! ```
//!
//! These are the most important types to get started, but *smallish*
//! supports other types as well:
//!
//! * **Maps and Structs** are enclosed in curly brackets `{}`
//!   containing key-value pairs, separated by either a comma or
//!   newlines.
//!
//!   `{foo = 5, bar = 10}`
//!
//!   When using map-flavored *smallish*, the brackets around the
//!   root map are omitted.
//!
//! * **Unit Structs** are written as `()`.
//!
//! * **None** is written `none`. This is used to represent [Option::None].
//!
//! * **Booleans** are written as `true` and `false`.
//!
//! * **Integers** are written as strings of digits in decimal, or
//!   prefixed by `0x` for hexadecimal, `0o` for octal, and `0b` for
//!   binary. They may start with `+` or `-`.
//!
//!   `-42`
//!
//! * **Floats** are written as strings of digits in decimal, with a
//!   decimal point `.` and optionally either a `+` or `-` in front and
//!   an exponent `e` at the end.
//!
//!   `6.28e-2`
//!
//! * **Characters** are written enclosed in single-quotes `'`, and
//!   support the same escapes as Rust.
//!
//!   `'A'`
//!
//!   `'\u{2603}'`
//!
//! * **Strings and Bytes** are written enclosed in double-quotes `"`,
//!   and bytes are prefixed with `b`. These also support the same
//!   escapes as Rust.
//!
//!   `"Hello,\nworld!"`
//!
//!   `b"\x00\x01"`
//!
//! ## Flavors
//!
//! *smallish* comes in [**Flavors**](crate::Flavor), which change how
//! the root value is represented. Value flavored is the default,
//! while map and list flavored *smallish* simply omit the enclosing
//! braces for that root value.
//!
//! ## Reserved Names
//!
//! Some names, like `true`, `false`, and `none`, are used to
//! represent values directly. If you need to use these names for an
//! enumeration variant or a key name, escape it by prefixing it with
//! `\`.
//!
//! ```
//! # use smallish::{Flavor, from_str};
//! #[derive(Debug, PartialEq, Eq, serde::Deserialize)]
//! #[serde(rename_all = "snake_case")]
//! enum MyBool { True, False }
//!
//! assert_eq!(MyBool::True, from_str(Flavor::Value, r#"\true"#).unwrap());
//! ```
//!
//! ## Nested Enumerations and Precedence
//!
//! You can use parenthesis `()` to enclose values. This is sometimes
//! necessary to parse a value correctly. For example, enumerations
//! passed as arguments to another enumeration sometimes need
//! parenthesis to group the arguments correctly.
//!
//! ```
//! # use smallish::{Flavor, from_str};
//! #[derive(Debug, PartialEq, Eq, serde::Deserialize)]
//! enum Arg { Plain, WithValue(u8) }
//! #[derive(Debug, PartialEq, Eq, serde::Deserialize)]
//! enum Instr { Foo { arg: Arg } }
//!
//! let r: Vec<Instr> = from_str(Flavor::List, r#"
//! Foo arg=Plain
//! Foo arg=(WithValue 42)
//! "#).unwrap();
//!
//! assert_eq!(r, vec![
//!     Instr::Foo { arg: Arg::Plain },
//!     Instr::Foo { arg: Arg::WithValue(42) },
//! ]);
//! ```
//!
//! ## Using with `serde`
//!
//! There are a few details to be aware of when using *smallish* with
//! `serde`.
//!
//! ### Options
//!
//! Options are written as `none` for [None], and the value itself for
//! [Some].
//!
//! ```
//! # use smallish::{Flavor, from_str};
//! assert_eq!(from_str::<Option<u8>>(Flavor::Value, "none").unwrap(), None);
//! assert_eq!(from_str::<Option<u8>>(Flavor::Value, "20").unwrap(), Some(20));
//! ```
//!
//! ### Newtype Structs and Variants
//!
//! Newtypes are written exactly the same as if the in-between type
//! was not there. In particular, for newtype enumeration variants,
//! this means that variants containing a sequence look like tuple
//! variants, and those containing a struct look like struct
//! variants. Newtype variants containing any other data are treated
//! like tuple variants with one argument.
//!
//! ```
//! # use smallish::{Flavor, from_str};
//! #[derive(Debug, PartialEq, Eq, serde::Deserialize)]
//! enum E {
//!     NewtypeSeq(Vec<u8>),
//!     NewtypeStruct(std::ops::Range<u8>),
//!     NewtypePlain(u8),
//! }
//!
//! let r: Vec<E> = from_str(Flavor::List, r#"
//! NewtypeSeq 0 1 2
//! NewtypeStruct start=10 end=20
//! NewtypePlain 42
//! "#).unwrap();
//!
//! assert_eq!(r, vec![
//!     E::NewtypeSeq(vec![0, 1, 2]),
//!     E::NewtypeStruct(std::ops::Range { start: 10, end: 20 }),
//!     E::NewtypePlain(42),
//! ]);
//! ```
//!
//! ### Escaped Strings and Bytes
//!
//! Strings and bytes can be borrowed without copy from the document
//! itself, as long as they do not contain any escapes. Strings and
//! bytes with escapes must be first un-escaped into a scratch buffer
//! provided to the deserializer, using
//! e.g. [from_str_escaped](crate::from_str_escaped) or
//! [from_slice_escaped](crate::from_slice_escaped). If this buffer
//! becomes full, the deserialization will fail.
//!
//! You can either increase the size of this buffer, or opt-out of
//! un-escaping by wrapping your string (or bytes) type in
//! [Escaped]. Strings wrapped this way can always be deserialized
//! without a copy, but may contain unhandled escape sequences that
//! you must handle yourself.
//!
//! ```
//! # use smallish::{Flavor, from_str, types::Escaped};
//! let r: Escaped<&str> = from_str(Flavor::Value, r#""hello\n""#).unwrap();
//! assert_eq!(*r, r#"hello\n"#)
//! ```
//!
//! ### Enum Representations and Untagged Variants
//!
//! *smallish* is designed to work with the default, externally-tagged
//! enum representation. However, it is possible to use the other
//! representations by using the bare variant name as the tag.
//!
//! ```
//! # use smallish::{Flavor, from_str};
//! #[derive(Debug, PartialEq, Eq, serde::Deserialize)]
//! #[serde(tag = "type")]
//! enum E {
//!     VariantName { a: u8 },
//! }
//!
//! let r: E = from_str(Flavor::Value, "{type=VariantName, a=42}").unwrap();
//! assert_eq!(r, E::VariantName { a: 42 });
//! ```
//!
//! However, due to ambiguity in the grammar, **alternately-tagged
//! enums, or enums with untagged variants, need special care**. If
//! such enums directly contain other enums, the contained enums will
//! be parsed slightly differently. Specifically,
//!
//! * `Variant` is always parsed as a unit variant.
//!
//! * `Variant k=v [...]` is always parsed as a struct variant, or a
//!   newtype variant containing a map or struct.
//!
//! * `Variant v [...]` is always parsed as a tuple variant, or a
//!   newtype variant containing a sequence.
//!
//! This means non-container newtype variants, or newtype variants
//! containing an empty container, cannot be parsed with the usual
//! syntax when inside an externally-tagged or untagged enum. This
//! restriction is only for the first level of parsing inside a
//! non-standard enum; deeper levels are not affected.
//!
//! Adjacently-tagged enums do not suffer from this restriction if the
//! tag precedes the content, but you should not rely on this
//! behavior.
//!
//! As a workaround, you can instead represent the contained enums
//! using map syntax.
//!
//! ```
//! # use smallish::{Flavor, from_str};
//! #[derive(Debug, PartialEq, Eq, serde::Deserialize)]
//! enum SubEnum {
//!     Sub(u8),
//! }
//!
//! #[derive(Debug, PartialEq, Eq, serde::Deserialize)]
//! #[serde(tag = "type")]
//! enum E {
//!     Tricky { a: SubEnum },
//! }
//!
//! // this fails
//! from_str::<E>(Flavor::Value, "{type=Tricky, a=(Sub 42)}").unwrap_err();
//!
//! // this works
//! let r: E = from_str(Flavor::Value, "{type=Tricky, a={Sub=42}}").unwrap();
//! assert_eq!(r, E::Tricky { a: SubEnum::Sub(42) });
//! ```

use crate::types::Escaped;

/// The internal integer type. Integers outside this range will fail to parse.
pub type Int = i64;

/// The internal float type. Floats outside this range will fail to parse.
pub type Float = f32;

define_enum_with_kind! {
    /// All possible tokens.
    #[derive(Clone, Copy, Debug, PartialEq)]
    #[cfg_attr(feature = "defmt", derive(defmt::Format))]
    pub enum Token<'de> {
        /// newline '\n'
        Newline,
        /// comma ','
        Comma,
        /// equals '='
        Equals,
        /// open parenthesis '('
        ParenOpen,
        /// close parenthesis ')'
        ParenClose,
        /// list open '['
        ListOpen,
        /// list close ']'
        ListClose,
        /// map open '{'
        MapOpen,
        /// map close '}'
        MapClose,
        /// identifier
        Ident(&'de str),
        /// simple value
        Value(Value<'de>),
    }
}

// to support error messages
impl core::fmt::Display for TokenKind {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let s = match self {
            Self::Newline => "'\\n'",
            Self::Comma => "','",
            Self::Equals => "'-'",
            Self::ParenOpen => "'('",
            Self::ParenClose => "')'",
            Self::ListOpen => "'['",
            Self::ListClose => "']'",
            Self::MapOpen => "'{'",
            Self::MapClose => "'}'",
            Self::Ident => "identifier",
            Self::Value => "value",
        };
        write!(f, "{}", s)
    }
}

/// Simple (non-compound) values.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Value<'de> {
    /// unit `()`
    Unit,
    /// `none`
    None,
    /// `true` or `false`
    Bool(bool),
    /// integers
    Int(Int),
    /// floats
    Float(Float),
    /// characters
    Char(char),
    /// strings, stored *with* escapes
    Str(Escaped<&'de str>),
    /// bytes, stored *with* escapes
    Bytes(Escaped<&'de [u8]>),
}

/// Parser events.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Event<'de> {
    /// A list has started. Expect values, and then [Self::ListClose].
    ListOpen,
    /// A list has ended.
    ListClose,
    /// A map has started. Expect alternating [Self::Key] and values,
    /// and then [Self::MapClose].
    MapOpen,
    /// A map has ended.
    MapClose,
    /// An enum has started with this name. Expect either values, or
    /// alternating [Self::Key] and values, terminated by
    /// [Self::EnumClose].
    EnumOpen(&'de str),
    /// An enum has ended.
    EnumClose,
    /// The parser found a key with this name.
    Key(&'de str),
    /// The parser found a simple value.
    Value(Value<'de>),
}
