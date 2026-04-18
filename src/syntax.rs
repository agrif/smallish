use crate::types::Escaped;

pub type Integer = i64;
pub type Float = f32;

define_enum_with_kind! {
    #[derive(Clone, Debug)]
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

#[derive(Clone, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Value<'de> {
    Null,
    Bool(bool),
    Integer(Integer),
    Float(Float),
    Character(char),
    String(Escaped<&'de str>),
}

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
    Value(Value<'de>),
}
