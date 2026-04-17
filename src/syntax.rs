pub type Integer = i64;
pub type Float = f32;

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
    Value(Value),
}

#[derive(Clone, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum TokenKind {
    Newline,
    Comma,
    Equals,
    ParenOpen,
    ParenClose,
    ListOpen,
    ListClose,
    MapOpen,
    MapClose,
    Ident,
    Value,
}

impl<'de> Token<'de> {
    pub fn kind(&self) -> TokenKind {
        match self {
            Self::Newline => TokenKind::Newline,
            Self::Comma => TokenKind::Comma,
            Self::Equals => TokenKind::Equals,
            Self::ParenOpen => TokenKind::ParenOpen,
            Self::ParenClose => TokenKind::ParenClose,
            Self::ListOpen => TokenKind::ListOpen,
            Self::ListClose => TokenKind::ListClose,
            Self::MapOpen => TokenKind::MapOpen,
            Self::MapClose => TokenKind::MapClose,
            Self::Ident(_) => TokenKind::Ident,
            Self::Value(_) => TokenKind::Value,
        }
    }
}

#[derive(Clone, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Value {
    Null,
    Bool(bool),
    Integer(Integer),
    Float(Float),
}

impl Value {
    pub fn as_null(&self) -> Option<()> {
        matches!(self, Self::Null).then_some(())
    }

    pub fn as_bool(&self) -> Option<bool> {
        if let Self::Bool(v) = self {
            Some(*v)
        } else {
            None
        }
    }

    pub fn as_integer(&self) -> Option<Integer> {
        if let Self::Integer(v) = self {
            Some(*v)
        } else {
            None
        }
    }

    pub fn as_float(&self) -> Option<Float> {
        if let Self::Float(v) = self {
            Some(*v)
        } else {
            None
        }
    }
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
    Value(Value),
}

impl<'de> Event<'de> {
    pub fn as_list_open(&self) -> Option<()> {
        matches!(self, Self::ListOpen).then_some(())
    }

    pub fn as_list_close(&self) -> Option<()> {
        matches!(self, Self::ListClose).then_some(())
    }

    pub fn as_map_open(&self) -> Option<()> {
        matches!(self, Self::MapOpen).then_some(())
    }

    pub fn as_map_close(&self) -> Option<()> {
        matches!(self, Self::MapClose).then_some(())
    }

    pub fn as_enum_open(&self) -> Option<&'de str> {
        if let Self::EnumOpen(name) = self {
            Some(name)
        } else {
            None
        }
    }

    pub fn as_enum_close(&self) -> Option<()> {
        matches!(self, Self::EnumClose).then_some(())
    }

    pub fn as_key(&self) -> Option<&'de str> {
        if let Self::Key(name) = self {
            Some(name)
        } else {
            None
        }
    }

    pub fn as_value(&self) -> Option<&Value> {
        if let Self::Value(v) = self {
            Some(v)
        } else {
            None
        }
    }
}
