use crate::types::Escaped;

pub type Integer = i64;
pub type Float = f32;

define_enum_with_kind! {
    #[derive(Clone, Copy, Debug, PartialEq)]
    #[cfg_attr(feature = "defmt", derive(defmt::Format))]
    pub enum Token<'de> {
        Newline,
        Comma,
        Equals,
        ParenOpen,
        ParenClose,
        ListOpen,
        ListClose,
        MapOpen,
        MapClose,
        Ident(&'de str),
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

#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Value<'de> {
    Null,
    Bool(bool),
    Integer(Integer),
    Float(Float),
    Character(char),
    String(Escaped<&'de str>),
    Bytes(Escaped<&'de [u8]>),
}

#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Event<'de> {
    ListOpen,
    ListClose,
    MapOpen,
    MapClose,
    EnumOpen(&'de str),
    EnumClose,
    Key(&'de str),
    Value(Value<'de>),
}
