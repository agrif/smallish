use as_variant::as_variant;
use serde::de;

use super::{ParseError, Parser, ParserState};
use crate::syntax::{Event, Float, Integer, Value};
use crate::types::{Escaped, Located, UnescapeError};
use crate::Flavor;

#[derive(Clone, Debug, thiserror::Error)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Error {
    #[error("parse error: {0}")]
    Parse(#[from] ParseError),
    #[error("unused input at end")]
    UnusedInput,
    #[error("not implemented: {0}")]
    NotImplemented(&'static str),
    #[error("integer out of range: {0}")]
    IntegerRange(Integer),
    #[error("float out of range: {0}")]
    FloatRange(Float),
    #[error("unescape buffer full")]
    BufferFull,

    #[cfg(feature = "custom-error-messages")]
    #[error("{0}")]
    Custom(heapless::String<64>),

    #[cfg(not(feature = "custom-error-messages"))]
    #[error("serde error")]
    Custom,

    #[error("invalid type")]
    InvalidType,
    #[error("invalid value")]
    InvalidValue,
    #[error("invalid length: {0}")]
    InvalidLength(usize),
    #[error("unknown variant: expected {0:?}")]
    UnknownVariant(&'static [&'static str]),
    #[error("unknown field: expected {0:?}")]
    UnknownField(&'static [&'static str]),
    #[error("missing field: {0}")]
    MissingField(&'static str),
    #[error("duplicate field: {0}")]
    DuplicateField(&'static str),
}

impl<'de> From<Located<'de, ParseError>> for Located<'de, Error> {
    fn from(other: Located<'de, ParseError>) -> Self {
        other.map(Into::into)
    }
}

impl From<UnescapeError> for Error {
    fn from(other: UnescapeError) -> Self {
        match other {
            UnescapeError::BadLiteral(e) => Error::Parse(e.into()),
            UnescapeError::BufferFull => Error::BufferFull,
        }
    }
}

impl de::Error for Error {
    #[cfg(feature = "custom-error-messages")]
    fn custom<T>(msg: T) -> Self
    where
        T: core::fmt::Display,
    {
        use core::fmt::Write;
        let mut s = heapless::String::new();
        if write!(&mut s, "{}", msg).is_err() {
            s.clear();
            let _ = s.push_str("<too large for buffer>");
        }
        Self::Custom(s)
    }

    #[cfg(not(feature = "custom-error-messages"))]
    fn custom<T>(_msg: T) -> Self
    where
        T: core::fmt::Display,
    {
        Self::Custom
    }

    fn invalid_type(_unexp: de::Unexpected<'_>, _exp: &dyn de::Expected) -> Self {
        Self::InvalidType
    }

    fn invalid_value(_unexp: de::Unexpected<'_>, _exp: &dyn de::Expected) -> Self {
        Self::InvalidValue
    }

    fn invalid_length(len: usize, _exp: &dyn de::Expected) -> Self {
        Self::InvalidLength(len)
    }

    fn unknown_variant(_variant: &str, expected: &'static [&'static str]) -> Self {
        Self::UnknownVariant(expected)
    }

    fn unknown_field(_field: &str, expected: &'static [&'static str]) -> Self {
        Self::UnknownField(expected)
    }

    fn missing_field(field: &'static str) -> Self {
        Self::MissingField(field)
    }

    fn duplicate_field(field: &'static str) -> Self {
        Self::DuplicateField(field)
    }
}

#[derive(Clone, Copy, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
enum Flag {
    NewtypeEnum,
}

#[derive(Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Deserializer<'de, S> {
    parser: Parser<'de, S>,
    peeked: Option<Event<'de>>,
    last_event_location: Located<'de, ()>,
    unescape: &'de mut [u8],
    flag: Option<Flag>,
}

impl<'de, S> Deserializer<'de, S>
where
    S: AsRef<[ParserState]> + AsMut<[ParserState]>,
{
    pub fn from_parser(parser: Parser<'de, S>, unescape: &'de mut [u8]) -> Self {
        Self {
            last_event_location: parser.location().clone(),
            parser: parser,
            peeked: None,
            unescape,
            flag: None,
        }
    }

    pub fn new(flavor: Flavor, input: &'de [u8], state: S, unescape: &'de mut [u8]) -> Self {
        Self::from_parser(Parser::new(flavor, input, state), unescape)
    }

    pub fn deserialize<T>(&mut self) -> Result<T, Located<'de, Error>>
    where
        T: de::Deserialize<'de>,
    {
        let result = T::deserialize(&mut *self);
        self.finalize(result)
    }

    fn finalize<T>(&self, mut result: Result<T, Error>) -> Result<T, Located<'de, Error>> {
        if result.is_ok() && !self.parser.is_eof() {
            result = Err(Error::UnusedInput);
        }

        result.map_err(|e| self.last_event_location.wrap(e))
    }
}

trait SmallishDe<'de>: de::Deserializer<'de, Error = Error> {
    fn base(self) -> impl SmallishDe<'de>;

    #[inline]
    fn next(self) -> Result<Event<'de>, Error> {
        self.base().next()
    }

    #[inline]
    fn peek(self) -> Result<Event<'de>, Error> {
        self.base().peek()
    }

    #[inline]
    fn consume(self) {
        self.base().consume()
    }

    #[inline]
    fn location(self) -> Located<'de, ()> {
        self.base().location()
    }

    #[inline]
    fn error_without_event<T>(self, err: Error) -> Result<T, Error> {
        self.base().error_without_event(err)
    }

    #[inline]
    fn next_with<T>(self, f: impl FnOnce(Event<'de>) -> Option<T>) -> Result<T, Error> {
        let ev = self.next()?;
        f(ev).ok_or(Error::InvalidType)
    }

    #[inline]
    fn peek_with<T>(self, f: impl FnOnce(Event<'de>) -> Option<T>) -> Result<Option<T>, Error> {
        let ev = self.peek()?;
        Ok(f(ev))
    }
}

impl<'de, S> SmallishDe<'de> for &mut Deserializer<'de, S>
where
    S: AsRef<[ParserState]> + AsMut<[ParserState]>,
{
    #[inline]
    fn base(self) -> impl SmallishDe<'de> {
        self
    }

    fn next(self) -> Result<Event<'de>, Error> {
        if let Some(ev) = self.peeked.take() {
            self.flag = None;
            return Ok(ev);
        }

        let next = self.parser.next();
        let (loc, ev) = Located::from_result(next).split();
        self.last_event_location = loc;

        let ev = ev?;
        self.flag = None;
        Ok(ev)
    }

    fn peek(self) -> Result<Event<'de>, Error> {
        if let Some(ev) = &self.peeked {
            return Ok(ev.clone());
        }

        let next = self.parser.next();
        let (loc, ev) = Located::from_result(next).split();
        self.last_event_location = loc;

        let ev = ev?;
        self.peeked = Some(ev.clone());
        Ok(ev)
    }

    #[inline]
    fn consume(self) {
        self.flag = None;
        self.peeked = None;
    }

    #[inline]
    fn location(self) -> Located<'de, ()> {
        if self.peeked.is_some() {
            self.last_event_location
        } else {
            *self.parser.location()
        }
    }

    #[inline]
    fn error_without_event<T>(self, err: Error) -> Result<T, Error> {
        self.last_event_location = self.location();
        Err(err)
    }
}

impl<'de, S> de::Deserializer<'de> for &mut Deserializer<'de, S>
where
    S: AsRef<[ParserState]> + AsMut<[ParserState]>,
{
    type Error = Error;

    fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        if let Some(Flag::NewtypeEnum) = self.flag {
            return match self.peek()? {
                Event::Key(_) => self.deserialize_map(visitor),
                _ => self.deserialize_seq(visitor),
            };
        }

        self.flag = None;

        match self.peek()? {
            Event::ListOpen => self.deserialize_seq(visitor),
            Event::MapOpen => self.deserialize_map(visitor),
            Event::EnumOpen(_) => self.deserialize_enum("", &[], visitor),
            Event::Value(v) => match v {
                Value::Null => self.deserialize_unit(visitor),
                Value::Bool(_) => self.deserialize_bool(visitor),
                Value::Integer(_) => self.deserialize_i64(visitor),
                Value::Float(_) => self.deserialize_f32(visitor),
                Value::Character(_) => self.deserialize_char(visitor),
                Value::String(_) => self.deserialize_str(visitor),
                Value::Bytes(_) => self.deserialize_bytes(visitor),
            },
            Event::ListClose | Event::MapClose | Event::EnumClose | Event::Key(_) => {
                Err(Error::InvalidType)
            }
        }
    }

    fn deserialize_bool<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        let v = self.next_with(|t| {
            as_variant!(t, Event::Value).and_then(as_variant!(Value::Bool(v) => v))
        })?;
        visitor.visit_bool(v)
    }

    fn deserialize_i8<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        let v = self.next_with(|t| {
            as_variant!(t, Event::Value).and_then(as_variant!(Value::Integer(v) => v))
        })?;
        let v = v.try_into().map_err(|_| Error::IntegerRange(v))?;
        visitor.visit_i8(v)
    }

    fn deserialize_u8<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        let v = self.next_with(|t| {
            as_variant!(t, Event::Value).and_then(as_variant!(Value::Integer(v) => v))
        })?;
        let v = v.try_into().map_err(|_| Error::IntegerRange(v))?;
        visitor.visit_u8(v)
    }

    fn deserialize_i16<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        let v = self.next_with(|t| {
            as_variant!(t, Event::Value).and_then(as_variant!(Value::Integer(v) => v))
        })?;
        let v = v.try_into().map_err(|_| Error::IntegerRange(v))?;
        visitor.visit_i16(v)
    }

    fn deserialize_u16<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        let v = self.next_with(|t| {
            as_variant!(t, Event::Value).and_then(as_variant!(Value::Integer(v) => v))
        })?;
        let v = v.try_into().map_err(|_| Error::IntegerRange(v))?;
        visitor.visit_u16(v)
    }

    fn deserialize_i32<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        let v = self.next_with(|t| {
            as_variant!(t, Event::Value).and_then(as_variant!(Value::Integer(v) => v))
        })?;
        let v = v.try_into().map_err(|_| Error::IntegerRange(v))?;
        visitor.visit_i32(v)
    }

    fn deserialize_u32<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        let v = self.next_with(|t| {
            as_variant!(t, Event::Value).and_then(as_variant!(Value::Integer(v) => v))
        })?;
        let v = v.try_into().map_err(|_| Error::IntegerRange(v))?;
        visitor.visit_u32(v)
    }

    fn deserialize_i64<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        let v = self.next_with(|t| {
            as_variant!(t, Event::Value).and_then(as_variant!(Value::Integer(v) => v))
        })?;
        visitor.visit_i64(v)
    }

    fn deserialize_u64<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        let v = self.next_with(|t| {
            as_variant!(t, Event::Value).and_then(as_variant!(Value::Integer(v) => v))
        })?;
        let v = v.try_into().map_err(|_| Error::IntegerRange(v))?;
        visitor.visit_u64(v)
    }

    fn deserialize_i128<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        let v = self.next_with(|t| {
            as_variant!(t, Event::Value).and_then(as_variant!(Value::Integer(v) => v))
        })?;
        let v = v.try_into().map_err(|_| Error::IntegerRange(v))?;
        visitor.visit_i128(v)
    }

    fn deserialize_u128<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        let v = self.next_with(|t| {
            as_variant!(t, Event::Value).and_then(as_variant!(Value::Integer(v) => v))
        })?;
        let v = v.try_into().map_err(|_| Error::IntegerRange(v))?;
        visitor.visit_u128(v)
    }

    fn deserialize_f32<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        let v = self.next_with(|t| {
            as_variant!(t, Event::Value).and_then(as_variant!(Value::Float(v) => v))
        })?;
        visitor.visit_f32(v)
    }

    fn deserialize_f64<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        let v = self.next_with(|t| {
            as_variant!(t, Event::Value).and_then(as_variant!(Value::Float(v) => v))
        })?;
        let v = v.try_into().map_err(|_| Error::FloatRange(v))?;
        visitor.visit_f64(v)
    }

    fn deserialize_char<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        let v = self.next_with(|t| {
            as_variant!(t, Event::Value).and_then(as_variant!(Value::Character(v) => v))
        })?;
        visitor.visit_char(v)
    }

    fn deserialize_str<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        let v = self.next_with(|t| {
            as_variant!(t, Event::Value).and_then(as_variant!(Value::String(v) => v))
        })?;

        if !v.has_escapes() {
            visitor.visit_borrowed_str(*v)
        } else {
            let unescape = core::mem::replace(&mut self.unescape, &mut []);
            let (unescape, v) = v.unescape(unescape)?;
            self.unescape = unescape;
            visitor.visit_borrowed_str(v)
        }
    }

    fn deserialize_string<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        self.deserialize_str(visitor)
    }

    fn deserialize_bytes<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        let v = self.next_with(|t| {
            as_variant!(t, Event::Value).and_then(as_variant!(Value::Bytes(v) => v))
        })?;

        if !v.has_escapes() {
            visitor.visit_borrowed_bytes(*v)
        } else {
            let unescape = core::mem::replace(&mut self.unescape, &mut []);
            let (unescape, v) = v.unescape(unescape)?;
            self.unescape = unescape;
            visitor.visit_borrowed_bytes(v)
        }
    }

    fn deserialize_byte_buf<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        self.deserialize_bytes(visitor)
    }

    fn deserialize_option<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        if self
            .peek_with(|t| as_variant!(t, Event::Value).and_then(as_variant!(Value::Null => ())))?
            .is_some()
        {
            self.consume();
            visitor.visit_none()
        } else {
            visitor.visit_some(self)
        }
    }

    fn deserialize_unit<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        self.next_with(|t| as_variant!(t, Event::Value).and_then(as_variant!(Value::Null => ())))?;
        visitor.visit_unit()
    }

    fn deserialize_unit_struct<V>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        self.deserialize_unit(visitor)
    }

    fn deserialize_newtype_struct<V>(
        self,
        name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        if name == Escaped::<()>::SERDE_NAME {
            visitor.visit_newtype_struct(&mut EscapedAccess::new(self))
        } else {
            visitor.visit_newtype_struct(self)
        }
    }

    fn deserialize_seq<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        if let Some(Flag::NewtypeEnum) = self.flag {
            self.flag = None;
            visitor.visit_seq(Access::new(self))
        } else {
            self.next_with(as_variant!(Event::ListOpen => ()))?;
            let v = visitor.visit_seq(Access::new(self))?;
            self.next_with(as_variant!(Event::ListClose => ()))?;
            Ok(v)
        }
    }

    fn deserialize_tuple<V>(self, _len: usize, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        self.deserialize_seq(visitor)
    }

    fn deserialize_tuple_struct<V>(
        self,
        _name: &'static str,
        _len: usize,
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        self.deserialize_seq(visitor)
    }

    fn deserialize_map<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        if let Some(Flag::NewtypeEnum) = self.flag {
            self.flag = None;
            visitor.visit_map(Access::new(self))
        } else {
            self.next_with(as_variant!(Event::MapOpen => ()))?;
            let v = visitor.visit_map(Access::new(self))?;
            self.next_with(as_variant!(Event::MapClose => ()))?;
            Ok(v)
        }
    }

    fn deserialize_struct<V>(
        self,
        name: &'static str,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        if name == Located::SERDE_NAME {
            return visitor.visit_map(LocatedAccess::new(self.location(), self));
        }

        self.deserialize_map(visitor)
    }

    fn deserialize_enum<V>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        let v = visitor.visit_enum(Access::new(self))?;
        self.next_with(as_variant!(Event::EnumClose => ()))?;
        Ok(v)
    }

    fn deserialize_identifier<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        // only internally tagged enums seem to use this, and
        // those can't read enums themselves (only strings), so
        // the most consistent choice here is "only ever strings"
        self.deserialize_str(visitor)
    }

    fn deserialize_ignored_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        // simple: check nesting depth, do not distinguish type
        // (assume the parser is emitting well-formed events)
        let mut depth: usize = 0;
        loop {
            match self.next()? {
                // nesting events
                Event::ListOpen | Event::MapOpen | Event::EnumOpen(_) => {
                    depth += 1;
                }
                // un-nesting, value producing events
                Event::ListClose | Event::MapClose | Event::EnumClose => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        break;
                    }
                }
                // value event (no nesting)
                Event::Value(_) => {
                    if depth == 0 {
                        break;
                    }
                }
                // keys should not occur unless nested
                Event::Key(_) => {
                    if depth == 0 {
                        return Err(Error::InvalidType);
                    }
                }
            }
        }

        visitor.visit_unit()
    }
}

// helper to forward to self.de, in the same style as
// serde::forward_to_deserialize_any!
macro_rules! forward_to_inner_deserialize {
    // build one method, with arguments
    (@method, $func:ident<$l:tt, $v:ident>($($arg:ident : $ty:ty),*)) => {
        paste::paste! {
            #[inline]
            fn [<deserialize_ $func>]<$v>(self, $($arg: $ty,)* visitor: $v) -> Result<$v::Value, Self::Error>
            where
                $v: ::serde::de::Visitor<$l>,
            {
                self.de.[<deserialize_ $func>]($($arg,)* visitor)
            }
        }
    };

    // build one method, dispatching on type
    (@helper, unit_struct<$l:tt, $v:ident>) => {
        forward_to_inner_deserialize! { @method, unit_struct<$l, $v>(name: &'static str) }
    };
    (@helper, newtype_struct<$l:tt, $v:ident>) => {
        forward_to_inner_deserialize! { @method, newtype_struct<$l, $v>(name: &'static str) }
    };
    (@helper, tuple<$l:tt, $v:ident>) => {
        forward_to_inner_deserialize! { @method, tuple<$l, $v>(len: usize) }
    };
    (@helper, tuple_struct<$l:tt, $v:ident>) => {
        forward_to_inner_deserialize! { @method, tuple_struct<$l, $v>(name: &'static str, len: usize) }
    };
    (@helper, struct<$l:tt, $v:ident>) => {
        forward_to_inner_deserialize! { @method, struct<$l, $v>(name: &'static str, fields: &'static [&'static str]) }
    };
    (@helper, enum<$l:tt, $v:ident>) => {
        forward_to_inner_deserialize! { @method, enum<$l, $v>(name: &'static str, variants: &'static [&'static str]) }
    };

    // generic helper that only accepts visitor
    (@helper, $func:ident<$l:tt, $v:ident>) => {
        forward_to_inner_deserialize! { @method, $func<$l, $v>() }
    };

    // entry points
    (<$visitor:ident : Visitor<$lifetime:tt>> $($func:ident)*) => {
        $(forward_to_inner_deserialize! { @helper, $func<$lifetime, $visitor> })*
    };
    ($($func:ident)*) => {
        forward_to_inner_deserialize! { <V: Visitor<'de>> $($func)* }
    };
}

#[derive(Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
struct Access<'a, 'de: 'a, S> {
    de: &'a mut Deserializer<'de, S>,
}

impl<'a, 'de, S> Access<'a, 'de, S> {
    fn new(de: &'a mut Deserializer<'de, S>) -> Self {
        Self { de }
    }
}

impl<'a, 'de, S> de::SeqAccess<'de> for Access<'a, 'de, S>
where
    S: AsRef<[ParserState]> + AsMut<[ParserState]>,
{
    type Error = Error;

    fn next_element_seed<T>(&mut self, seed: T) -> Result<Option<T::Value>, Self::Error>
    where
        T: de::DeserializeSeed<'de>,
    {
        if self
            .de
            .peek_with(as_variant!(Event::ListClose | Event::EnumClose => ()))?
            .is_some()
        {
            Ok(None)
        } else {
            seed.deserialize(&mut *self.de).map(Some)
        }
    }
}

impl<'a, 'de, S> de::EnumAccess<'de> for Access<'a, 'de, S>
where
    S: AsRef<[ParserState]> + AsMut<[ParserState]>,
{
    type Error = Error;
    type Variant = Self;

    fn variant_seed<T>(self, seed: T) -> Result<(T::Value, Self::Variant), Self::Error>
    where
        T: de::DeserializeSeed<'de>,
    {
        let name = self.de.next_with(as_variant!(Event::EnumOpen(n) => n))?;
        let de = de::value::BorrowedStrDeserializer::<'de, Error>::new(name);
        let val = seed.deserialize(de)?;
        Ok((val, self))
    }
}

impl<'a, 'de, S> de::VariantAccess<'de> for Access<'a, 'de, S>
where
    S: AsRef<[ParserState]> + AsMut<[ParserState]>,
{
    type Error = Error;

    fn unit_variant(self) -> Result<(), Self::Error> {
        Ok(())
    }

    fn newtype_variant_seed<T>(self, seed: T) -> Result<T::Value, Self::Error>
    where
        T: de::DeserializeSeed<'de>,
    {
        self.de.flag = Some(Flag::NewtypeEnum);
        seed.deserialize(&mut *self.de)
    }

    fn tuple_variant<V>(self, _len: usize, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        visitor.visit_seq(self)
    }

    fn struct_variant<V>(
        self,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        visitor.visit_map(self)
    }
}

impl<'a, 'de, S> de::MapAccess<'de> for Access<'a, 'de, S>
where
    S: AsRef<[ParserState]> + AsMut<[ParserState]>,
{
    type Error = Error;

    fn next_key_seed<K>(&mut self, seed: K) -> Result<Option<K::Value>, Self::Error>
    where
        K: de::DeserializeSeed<'de>,
    {
        if let Some(name) = self.de.peek_with(as_variant!(Event::Key(n) => n))? {
            self.de.consume();
            let de = de::value::BorrowedStrDeserializer::<'de, Error>::new(name);
            seed.deserialize(de).map(Some)
        } else {
            Ok(None)
        }
    }

    fn next_value_seed<V>(&mut self, seed: V) -> Result<V::Value, Self::Error>
    where
        V: de::DeserializeSeed<'de>,
    {
        seed.deserialize(&mut *self.de)
    }
}

#[derive(Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
struct EscapedAccess<'a, De> {
    de: &'a mut De,
}

impl<'a, De> EscapedAccess<'a, De> {
    fn new(de: &'a mut De) -> Self {
        Self { de }
    }
}

impl<'a, 'de, De> SmallishDe<'de> for &mut EscapedAccess<'a, De>
where
    for<'b> &'b mut De: SmallishDe<'de>,
{
    fn base(self) -> impl SmallishDe<'de> {
        self.de.base()
    }
}

impl<'a, 'de, De> de::Deserializer<'de> for &mut EscapedAccess<'a, De>
where
    for<'b> &'b mut De: SmallishDe<'de>,
{
    type Error = Error;

    forward_to_inner_deserialize! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char
        option unit unit_struct seq tuple
        tuple_struct map struct enum identifier ignored_any
    }

    fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        match self.de.peek()? {
            Event::Value(Value::String(_)) => self.deserialize_str(visitor),
            Event::Value(Value::Bytes(_)) => self.deserialize_bytes(visitor),
            _ => self.de.deserialize_any(visitor),
        }
    }

    fn deserialize_newtype_struct<V>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_str<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        let v = self.de.next_with(|t| {
            as_variant!(t, Event::Value).and_then(as_variant!(Value::String(v) => v))
        })?;
        visitor.visit_borrowed_str(*v)
    }

    fn deserialize_string<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        self.deserialize_str(visitor)
    }

    fn deserialize_bytes<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        let v = self.de.next_with(|t| {
            as_variant!(t, Event::Value).and_then(as_variant!(Value::Bytes(v) => v))
        })?;
        visitor.visit_borrowed_bytes(*v)
    }

    fn deserialize_byte_buf<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        self.deserialize_bytes(visitor)
    }
}

#[derive(Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
struct LocatedAccess<'a, 'de, De> {
    location: Located<'de, ()>,
    de: &'a mut De,
    state: LocatedState,
}

#[derive(Clone, Copy, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
enum LocatedState {
    Source,
    Line,
    Column,
    Offset,
    Value,
    End,
}

impl<'a, 'de, De> LocatedAccess<'a, 'de, De> {
    fn new(location: Located<'de, ()>, de: &'a mut De) -> Self {
        Self {
            location,
            de,
            state: LocatedState::Source,
        }
    }
}

impl<'a, 'de, De> SmallishDe<'de> for &mut LocatedAccess<'a, 'de, De>
where
    for<'b> &'b mut De: SmallishDe<'de>,
{
    fn base(self) -> impl SmallishDe<'de> {
        self.de.base()
    }
}

impl<'a, 'de, De> de::Deserializer<'de> for &mut LocatedAccess<'a, 'de, De>
where
    for<'b> &'b mut De: SmallishDe<'de>,
{
    type Error = Error;

    fn deserialize_any<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        self.de.error_without_event(Error::InvalidType)
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        unit unit_struct newtype_struct seq tuple
        tuple_struct map struct enum identifier ignored_any
    }

    fn deserialize_bytes<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        if let Some(src) = self.location.source {
            visitor.visit_borrowed_bytes(src)
        } else {
            self.de.error_without_event(Error::InvalidType)
        }
    }

    fn deserialize_byte_buf<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        self.deserialize_bytes(visitor)
    }

    fn deserialize_option<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        // only called on Option<&'de [u8]> for source
        if let Some(_) = self.location.source {
            visitor.visit_some(self)
        } else {
            visitor.visit_none()
        }
    }
}

impl<'a, 'de, De> de::MapAccess<'de> for LocatedAccess<'a, 'de, De>
where
    for<'b> &'b mut De: SmallishDe<'de>,
{
    type Error = Error;

    fn next_key_seed<K>(&mut self, seed: K) -> Result<Option<K::Value>, Self::Error>
    where
        K: de::DeserializeSeed<'de>,
    {
        let key = match self.state {
            LocatedState::Source => "source",
            LocatedState::Line => "line",
            LocatedState::Column => "column",
            LocatedState::Offset => "offset",
            LocatedState::Value => "value",
            LocatedState::End => {
                return Ok(None);
            }
        };

        let de = de::value::BorrowedStrDeserializer::new(key);
        seed.deserialize(de).map(Some)
    }

    fn next_value_seed<V>(&mut self, seed: V) -> Result<V::Value, Self::Error>
    where
        V: de::DeserializeSeed<'de>,
    {
        match self.state {
            LocatedState::Source => {
                self.state = LocatedState::Line;
                seed.deserialize(self)
            }
            LocatedState::Line => {
                self.state = LocatedState::Column;
                let de = de::value::UsizeDeserializer::new(self.location.line);
                seed.deserialize(de)
            }
            LocatedState::Column => {
                self.state = LocatedState::Offset;
                let de = de::value::UsizeDeserializer::new(self.location.column);
                seed.deserialize(de)
            }
            LocatedState::Offset => {
                self.state = LocatedState::Value;
                let de = de::value::UsizeDeserializer::new(self.location.offset);
                seed.deserialize(de)
            }
            LocatedState::Value => {
                self.state = LocatedState::End;
                seed.deserialize(&mut *self.de)
            }
            LocatedState::End => {
                unreachable!();
            }
        }
    }
}
