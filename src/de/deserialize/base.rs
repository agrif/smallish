use as_variant::as_variant;
use serde::de;

use crate::de::deserialize::{escaped, inline, located, Error};
use crate::de::{Parser, ParserState};
use crate::syntax::{Event, Value};
use crate::types::{Escaped, Located};
use crate::Flavor;

/// Deserialize source into a value.
///
/// This is the implementation of [serde::de::Deserializer] for
/// *smallish*. If the convenience functions aren't enough, you can
/// use this directly.
#[derive(Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Deserializer<'de, S> {
    parser: Parser<'de, S>,
    peeked: Option<Event<'de>>,
    last_event_location: Located<'de, ()>,
    unescape: &'de mut [u8],
}

impl<'de, S> Deserializer<'de, S>
where
    S: AsRef<[ParserState]> + AsMut<[ParserState]>,
{
    /// Create a new `flavor`-flavored deserializer operating on the
    /// given `input`.
    ///
    /// See [Parser::new] for the meaning of the `state` argument.
    ///
    /// The `unescape` argument is a scratch buffer where strings and
    /// bytes that contain escapes are un-escaped. If this buffer
    /// fills completely, deserialization will fail with
    /// [Error::BufferFull]. To avoid this, either provide a larger
    /// buffer or use [Escaped].
    ///
    /// It is valid to provide an empty buffer for `unescape` if you
    /// are certain your input will contain no string escapes.
    pub fn new(flavor: Flavor, input: &'de [u8], state: S, unescape: &'de mut [u8]) -> Self {
        let parser = Parser::new(flavor, input, state);
        Self {
            last_event_location: parser.location().clone(),
            parser: parser,
            peeked: None,
            unescape,
        }
    }

    /// Deserialize the `input` into a value.
    ///
    /// This is [T::deserialize](serde::Deserialize::deserialize), but
    /// it also annotates any errors with their source location and
    /// makes sure all input is consumed.
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

pub(crate) trait SmallishDe<'de>: de::Deserializer<'de, Error = Error> {
    fn base(self) -> impl SmallishDe<'de>;

    // implemented in terms of base()

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

    // common implementations rarely changed

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

    fn hook_special<V, F>(
        mut self,
        name: &'static str,
        visitor: V,
        default: F,
    ) -> Result<V::Value, Error>
    where
        V: de::Visitor<'de>,
        F: FnOnce(Self, V) -> Result<V::Value, Error>,
        Self: core::ops::DerefMut,
        Self::Target: Sized,
        for<'a> &'a mut Self::Target: SmallishDe<'de>,
    {
        match name {
            Escaped::<()>::SERDE_NAME => {
                visitor.visit_newtype_struct(&mut escaped::EscapedHandler::new(&mut *self))
            }
            Located::SERDE_NAME => {
                let loc = (&mut *self).location();
                visitor.visit_map(located::LocatedHandler::new(loc, &mut *self))
            }
            _ => default(self, visitor),
        }
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
            return Ok(ev);
        }

        let next = self.parser.next();
        let (loc, ev) = Located::from_result(next).split();
        self.last_event_location = loc;

        let ev = ev?;
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
        match self.peek()? {
            Event::ListOpen => self.deserialize_seq(visitor),
            Event::MapOpen => self.deserialize_map(visitor),
            Event::EnumOpen(_) => self.deserialize_enum("", &[], visitor),
            Event::Value(v) => match v {
                Value::Unit => self.deserialize_unit(visitor),
                Value::None => self.deserialize_option(visitor),
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
            .peek_with(|t| as_variant!(t, Event::Value).and_then(as_variant!(Value::None => ())))?
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
        self.next_with(|t| as_variant!(t, Event::Value).and_then(as_variant!(Value::Unit => ())))?;
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
        self.hook_special(name, visitor, |de, visitor| {
            visitor.visit_newtype_struct(de)
        })
    }

    fn deserialize_seq<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        self.next_with(as_variant!(Event::ListOpen => ()))?;
        let v = visitor.visit_seq(Access::new(self))?;
        self.next_with(as_variant!(Event::ListClose => ()))?;
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
        self.next_with(as_variant!(Event::MapOpen => ()))?;
        let v = visitor.visit_map(Access::new(self))?;
        self.next_with(as_variant!(Event::MapClose => ()))?;
        Ok(v)
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
        self.hook_special(name, visitor, |de, visitor| de.deserialize_map(visitor))
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

#[derive(Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub(crate) struct Access<'a, De> {
    de: &'a mut De,
}

impl<'a, De> Access<'a, De> {
    pub(crate) fn new(de: &'a mut De) -> Self {
        Self { de }
    }
}

impl<'a, 'de, De> de::SeqAccess<'de> for Access<'a, De>
where
    for<'b> &'b mut De: SmallishDe<'de>,
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

impl<'a, 'de, De> de::EnumAccess<'de> for Access<'a, De>
where
    for<'b> &'b mut De: SmallishDe<'de>,
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

impl<'a, 'de, De> de::VariantAccess<'de> for Access<'a, De>
where
    for<'b> &'b mut De: SmallishDe<'de>,
{
    type Error = Error;

    fn unit_variant(self) -> Result<(), Self::Error> {
        Ok(())
    }

    fn newtype_variant_seed<T>(self, seed: T) -> Result<T::Value, Self::Error>
    where
        T: de::DeserializeSeed<'de>,
    {
        seed.deserialize(&mut inline::InlineHandler::new(&mut *self.de))
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

impl<'a, 'de, De> de::MapAccess<'de> for Access<'a, De>
where
    for<'b> &'b mut De: SmallishDe<'de>,
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
