use serde::de;

use super::parse::{self, Parser};
use super::Located;
use crate::types::{Event, Integer, Value};

#[derive(Debug, Clone, thiserror::Error)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Error {
    #[error("parse error: {0}")]
    Parse(#[from] parse::Error),
    #[error("unused input at end")]
    UnusedInput,
    #[error("not implemented: {0}")]
    NotImplemented(&'static str),
    #[error("integer out of range: {0}")]
    OutOfRange(Integer),

    #[error("unknown error")]
    Unknown,
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

impl<'de> From<Located<'de, parse::Error>> for Located<'de, Error> {
    fn from(other: Located<'de, parse::Error>) -> Self {
        other.map(Into::into)
    }
}

impl de::Error for Error {
    fn custom<T>(_msg: T) -> Self
    where
        T: core::fmt::Display,
    {
        Self::Unknown
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

#[derive(Debug, Clone)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Deserializer<'de, const STACK: usize> {
    parser: Parser<'de, STACK>,
    peeked: Option<Event<'de>>,
    location: Located<'de, ()>,
}

impl<'de, const STACK: usize> Deserializer<'de, STACK> {
    pub fn from_parser(parser: Parser<'de, STACK>) -> Self {
        Self {
            location: parser.location().clone(),
            parser: parser,
            peeked: None,
        }
    }

    pub fn from_str(input: &'de str) -> Self {
        Self::from_parser(Parser::from_str(input))
    }

    pub fn list_from_str(input: &'de str) -> Self {
        Self::from_parser(Parser::list_from_str(input))
    }

    pub fn map_from_str(input: &'de str) -> Self {
        Self::from_parser(Parser::list_from_str(input))
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

        self.location.wrap(result).to_result().map(|r| r.value)
    }

    fn next(&mut self) -> Result<Event<'de>, Error> {
        if let Some(ev) = self.peeked.take() {
            return Ok(ev);
        }

        let next = self.parser.next();
        let (loc, ev) = Located::from_result(next).split();
        self.location = loc;

        Ok(ev?)
    }

    fn peek(&mut self) -> Result<Event<'de>, Error> {
        if let Some(ev) = &self.peeked {
            return Ok(ev.clone());
        }

        let next = self.parser.next();
        let (loc, ev) = Located::from_result(next).split();
        self.location = loc;

        let ev = ev?;
        self.peeked = Some(ev.clone());
        Ok(ev)
    }

    fn next_with<T>(&mut self, f: impl FnOnce(&Event<'de>) -> Option<T>) -> Result<T, Error> {
        let ev = self.next()?;
        f(&ev).ok_or(Error::InvalidType)
    }

    fn peek_with<T>(
        &mut self,
        f: impl FnOnce(&Event<'de>) -> Option<T>,
    ) -> Result<Option<T>, Error> {
        let ev = self.peek()?;
        Ok(f(&ev))
    }

    fn consume(&mut self) {
        self.peeked = None;
    }
}

pub fn from_str<'de, T>(input: &'de str) -> Result<T, Located<'de, Error>>
where
    T: de::Deserialize<'de>,
{
    Deserializer::<64>::from_str(input).deserialize()
}

pub fn list_from_str<'de, T>(input: &'de str) -> Result<T, Located<'de, Error>>
where
    T: de::Deserialize<'de>,
{
    Deserializer::<64>::list_from_str(input).deserialize()
}

pub fn map_from_str<'de, T>(input: &'de str) -> Result<T, Located<'de, Error>>
where
    T: de::Deserialize<'de>,
{
    Deserializer::<64>::list_from_str(input).deserialize()
}

impl<'de, const STACK: usize> de::Deserializer<'de> for &mut Deserializer<'de, STACK> {
    type Error = Error;

    fn deserialize_any<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        Err(Error::NotImplemented("any"))
    }

    fn deserialize_bool<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        let v = self.next_with(|t| t.as_value().and_then(Value::as_bool))?;
        visitor.visit_bool(v)
    }

    fn deserialize_i8<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        let v = self.next_with(|t| t.as_value().and_then(Value::as_integer))?;
        let v = v.try_into().map_err(|_| Error::OutOfRange(v))?;
        visitor.visit_i8(v)
    }

    fn deserialize_u8<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        let v = self.next_with(|t| t.as_value().and_then(Value::as_integer))?;
        let v = v.try_into().map_err(|_| Error::OutOfRange(v))?;
        visitor.visit_u8(v)
    }

    fn deserialize_i16<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        let v = self.next_with(|t| t.as_value().and_then(Value::as_integer))?;
        let v = v.try_into().map_err(|_| Error::OutOfRange(v))?;
        visitor.visit_i16(v)
    }

    fn deserialize_u16<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        let v = self.next_with(|t| t.as_value().and_then(Value::as_integer))?;
        let v = v.try_into().map_err(|_| Error::OutOfRange(v))?;
        visitor.visit_u16(v)
    }

    fn deserialize_i32<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        let v = self.next_with(|t| t.as_value().and_then(Value::as_integer))?;
        let v = v.try_into().map_err(|_| Error::OutOfRange(v))?;
        visitor.visit_i32(v)
    }

    fn deserialize_u32<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        let v = self.next_with(|t| t.as_value().and_then(Value::as_integer))?;
        let v = v.try_into().map_err(|_| Error::OutOfRange(v))?;
        visitor.visit_u32(v)
    }

    fn deserialize_i64<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        let v = self.next_with(|t| t.as_value().and_then(Value::as_integer))?;
        visitor.visit_i64(v)
    }

    fn deserialize_u64<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        let v = self.next_with(|t| t.as_value().and_then(Value::as_integer))?;
        let v = v.try_into().map_err(|_| Error::OutOfRange(v))?;
        visitor.visit_u64(v)
    }

    fn deserialize_i128<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        Err(Error::NotImplemented("i128"))
    }

    fn deserialize_u128<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        Err(Error::NotImplemented("u128"))
    }

    fn deserialize_f32<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        let v = self.next_with(|t| t.as_value().and_then(Value::as_float))?;
        visitor.visit_f32(v)
    }

    fn deserialize_f64<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        Err(Error::NotImplemented("f64"))
    }

    fn deserialize_char<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        Err(Error::NotImplemented("char"))
    }

    fn deserialize_str<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        Err(Error::NotImplemented("str"))
    }

    fn deserialize_string<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        Err(Error::NotImplemented("str"))
    }

    fn deserialize_bytes<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        Err(Error::NotImplemented("bytes"))
    }

    fn deserialize_byte_buf<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        Err(Error::NotImplemented("bytes"))
    }

    fn deserialize_option<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        if self
            .peek_with(|t| t.as_value().and_then(Value::as_null))?
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
        self.next_with(|t| t.as_value().and_then(Value::as_null))?;
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
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_seq<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        self.next_with(Event::as_list_open)?;
        let v = visitor.visit_seq(Access::new(self))?;
        self.next_with(Event::as_list_close)?;
        Ok(v)
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
        self.next_with(Event::as_map_open)?;
        let v = visitor.visit_map(Access::new(self))?;
        self.next_with(Event::as_map_close)?;
        Ok(v)
    }

    fn deserialize_struct<V>(
        self,
        _name: &'static str,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
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
        self.next_with(Event::as_enum_close)?;
        Ok(v)
    }

    fn deserialize_identifier<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        Err(Error::NotImplemented("identifier"))
    }

    fn deserialize_ignored_any<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        Err(Error::NotImplemented("ignored_any"))
    }
}

#[derive(Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
struct Access<'a, 'de: 'a, const STACK: usize> {
    de: &'a mut Deserializer<'de, STACK>,
}

impl<'a, 'de, const STACK: usize> Access<'a, 'de, STACK> {
    fn new(de: &'a mut Deserializer<'de, STACK>) -> Self {
        Self { de }
    }
}

impl<'a, 'de, const STACK: usize> de::SeqAccess<'de> for Access<'a, 'de, STACK> {
    type Error = Error;

    fn next_element_seed<T>(&mut self, seed: T) -> Result<Option<T::Value>, Self::Error>
    where
        T: de::DeserializeSeed<'de>,
    {
        if self.de.peek_with(Event::as_list_close)?.is_some() {
            Ok(None)
        } else {
            seed.deserialize(&mut *self.de).map(Some)
        }
    }
}

impl<'a, 'de, const STACK: usize> de::EnumAccess<'de> for Access<'a, 'de, STACK> {
    type Error = Error;
    type Variant = Self;

    fn variant_seed<T>(self, seed: T) -> Result<(T::Value, Self::Variant), Self::Error>
    where
        T: de::DeserializeSeed<'de>,
    {
        let name = self.de.next_with(Event::as_enum_open)?;
        let de = de::value::BorrowedStrDeserializer::<'de, Error>::new(name);
        let val = seed.deserialize(de)?;
        Ok((val, self))
    }
}

impl<'a, 'de, const STACK: usize> de::VariantAccess<'de> for Access<'a, 'de, STACK> {
    type Error = Error;

    fn unit_variant(self) -> Result<(), Self::Error> {
        Ok(())
    }

    fn newtype_variant_seed<T>(self, seed: T) -> Result<T::Value, Self::Error>
    where
        T: de::DeserializeSeed<'de>,
    {
        seed.deserialize(&mut *self.de)
    }

    fn tuple_variant<V>(self, _len: usize, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        de::Deserializer::deserialize_seq(&mut *self.de, visitor)
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

impl<'a, 'de, const STACK: usize> de::MapAccess<'de> for Access<'a, 'de, STACK> {
    type Error = Error;

    fn next_key_seed<K>(&mut self, seed: K) -> Result<Option<K::Value>, Self::Error>
    where
        K: de::DeserializeSeed<'de>,
    {
        if let Some(name) = self.de.peek_with(Event::as_key)? {
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
