use crate::types::Escaped;

/// The internal integer type. Integers outside this range will fail to parse.
pub type Integer = i64;

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
    /// `null`
    Null,
    /// `true` or `false`
    Bool(bool),
    /// integers
    Integer(Integer),
    /// floats
    Float(Float),
    /// characters
    Character(char),
    /// strings, stored *with* escapes
    String(Escaped<&'de str>),
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
