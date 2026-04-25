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
    /// [Error::UnescapeBufferFull]. To avoid this, either provide a larger
    /// buffer or use [Escaped].
    ///
    /// It is valid to provide an empty buffer for `unescape` if you
    /// are certain your input will contain no string escapes.
    pub fn new(flavor: Flavor, input: &'de [u8], state: S, unescape: &'de mut [u8]) -> Self {
        let parser = Parser::new(flavor, input, state);
        Self {
            last_event_location: parser.location(),
            parser,
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

    fn finalize<T>(&mut self, mut result: Result<T, Error>) -> Result<T, Located<'de, Error>> {
        if result.is_ok() && (self.peeked.is_some() || !self.parser.is_eof()) {
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

        let next = self.parser.next_event();
        let (loc, ev) = Located::from_result(next).split();
        self.last_event_location = loc;

        let ev = ev?;
        Ok(ev)
    }

    fn peek(self) -> Result<Event<'de>, Error> {
        if let Some(ev) = self.peeked {
            return Ok(ev);
        }

        let next = self.parser.next_event();
        let (loc, ev) = Located::from_result(next).split();
        self.last_event_location = loc;

        let ev = ev?;
        self.peeked = Some(ev);
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
            self.parser.location()
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
            // things that use deserialize_any (like enum representations)
            // are not usually equipped to handle enum events, so instead
            // turn them into externally-tagged maps or strings
            Event::EnumOpen(name) => {
                self.consume();
                if self
                    .peek_with(as_variant!(Event::EnumClose => ()))?
                    .is_some()
                {
                    // Unit enum, be a string.
                    // This could also be an empty seq/map, but unit enums
                    // seem to be more likely. Sorry!
                    self.consume();
                    visitor.visit_borrowed_str(name)
                } else {
                    let v = visitor.visit_map(ExternalEnumAccess::new(self, name));
                    self.next_with(as_variant!(Event::EnumClose => ()))?;
                    v
                }
            }
            Event::Value(v) => match v {
                Value::Unit => self.deserialize_unit(visitor),
                Value::None => self.deserialize_option(visitor),
                Value::Bool(_) => self.deserialize_bool(visitor),
                Value::Int(_) => self.deserialize_i64(visitor),
                Value::Float(_) => self.deserialize_f32(visitor),
                Value::Char(_) => self.deserialize_char(visitor),
                Value::Str(_) => self.deserialize_str(visitor),
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
        let v = self.next_with(as_variant!(Event::Value(Value::Bool(v)) => v))?;
        visitor.visit_bool(v)
    }

    fn deserialize_i8<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        let v = self.next_with(as_variant!(Event::Value(Value::Int(v)) => v))?;
        let v = v.try_into().map_err(|_| Error::IntRange(v))?;
        visitor.visit_i8(v)
    }

    fn deserialize_u8<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        let v = self.next_with(as_variant!(Event::Value(Value::Int(v)) => v))?;
        let v = v.try_into().map_err(|_| Error::IntRange(v))?;
        visitor.visit_u8(v)
    }

    fn deserialize_i16<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        let v = self.next_with(as_variant!(Event::Value(Value::Int(v)) => v))?;
        let v = v.try_into().map_err(|_| Error::IntRange(v))?;
        visitor.visit_i16(v)
    }

    fn deserialize_u16<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        let v = self.next_with(as_variant!(Event::Value(Value::Int(v)) => v))?;
        let v = v.try_into().map_err(|_| Error::IntRange(v))?;
        visitor.visit_u16(v)
    }

    fn deserialize_i32<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        let v = self.next_with(as_variant!(Event::Value(Value::Int(v)) => v))?;
        let v = v.try_into().map_err(|_| Error::IntRange(v))?;
        visitor.visit_i32(v)
    }

    fn deserialize_u32<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        let v = self.next_with(as_variant!(Event::Value(Value::Int(v)) => v))?;
        let v = v.try_into().map_err(|_| Error::IntRange(v))?;
        visitor.visit_u32(v)
    }

    fn deserialize_i64<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        let v = self.next_with(as_variant!(Event::Value(Value::Int(v)) => v))?;
        visitor.visit_i64(v)
    }

    fn deserialize_u64<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        let v = self.next_with(as_variant!(Event::Value(Value::Int(v)) => v))?;
        let v = v.try_into().map_err(|_| Error::IntRange(v))?;
        visitor.visit_u64(v)
    }

    fn deserialize_i128<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        let v = self.next_with(as_variant!(Event::Value(Value::Int(v)) => v))?;
        let v = v.into();
        visitor.visit_i128(v)
    }

    fn deserialize_u128<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        let v = self.next_with(as_variant!(Event::Value(Value::Int(v)) => v))?;
        let v = v.try_into().map_err(|_| Error::IntRange(v))?;
        visitor.visit_u128(v)
    }

    fn deserialize_f32<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        let v = self.next_with(as_variant!(Event::Value(Value::Float(v)) => v))?;
        visitor.visit_f32(v)
    }

    fn deserialize_f64<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        let v = self.next_with(as_variant!(Event::Value(Value::Float(v)) => v))?;
        let v = v.into();
        visitor.visit_f64(v)
    }

    fn deserialize_char<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        let v = self.next_with(as_variant!(Event::Value(Value::Char(v)) => v))?;
        visitor.visit_char(v)
    }

    fn deserialize_str<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        let v = self.next_with(as_variant!(Event::Value(Value::Str(v)) => v))?;
        if !v.has_escapes() {
            visitor.visit_borrowed_str(*v)
        } else {
            let unescape = core::mem::take(&mut self.unescape);
            let (unescape, v) = v.unescape(unescape).map_err(|e| *e)?;
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
        let v = self.next_with(as_variant!(Event::Value(Value::Bytes(v)) => v))?;
        if !v.has_escapes() {
            visitor.visit_borrowed_bytes(*v)
        } else {
            let unescape = core::mem::take(&mut self.unescape);
            let (unescape, v) = v.unescape(unescape).map_err(|e| *e)?;
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
            .peek_with(as_variant!(Event::Value(Value::None) => ()))?
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
        self.next_with(as_variant!(Event::Value(Value::Unit) => ()))?;
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
        // enum representations use this for the variant, so look for
        // bare enums.
        let name = self.next_with(as_variant!(Event::EnumOpen(n) => n))?;
        self.next_with(as_variant!(Event::EnumClose => ()))?;
        visitor.visit_borrowed_str(name)
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

#[derive(Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
struct ExternalEnumAccess<'a, 'de, De> {
    de: &'a mut De,
    name: Option<&'de str>,
}

impl<'a, 'de, De> ExternalEnumAccess<'a, 'de, De> {
    fn new(de: &'a mut De, name: &'de str) -> Self {
        Self {
            de,
            name: Some(name),
        }
    }
}

impl<'a, 'de, De> de::MapAccess<'de> for ExternalEnumAccess<'a, 'de, De>
where
    for<'b> &'b mut De: SmallishDe<'de>,
{
    type Error = Error;

    fn next_key_seed<K>(&mut self, seed: K) -> Result<Option<K::Value>, Self::Error>
    where
        K: de::DeserializeSeed<'de>,
    {
        if let Some(name) = self.name.take() {
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
        let mut de = inline::InlineHandler::new(&mut *self.de);
        seed.deserialize(&mut de)
    }
}

#[cfg(test)]
mod test {
    extern crate alloc;
    extern crate std;

    macro_rules! de_test {
        ($(#[$attr:meta])* $name:ident, $flavor:ident, $src:expr, $val:expr $(, $ty:ty)? $(,)?) => {
            #[test]
            $(#[$attr])*
            fn $name() {
                use super::{Flavor};
                use crate::from_slice_escaped;
                let mut buf = [0; 128];
                let v $(: $ty)? = from_slice_escaped(
                    Flavor::$flavor,
                    $src.as_ref(),
                    &mut buf,
                ).unwrap();
                assert_eq!($val, v);
            }
        };
    }

    de_test!(
        #[should_panic(expected = "UnusedInput")]
        unused_input,
        Value,
        "  () \n  () ",
        (),
    );

    #[derive(Debug, PartialEq, Eq, serde::Deserialize)]
    struct MyUnit;

    de_test!(val_unit, Value, "  ()  \n ", ());
    de_test!(val_unit_struct, Value, "  ()  \n ", MyUnit);
    de_test!(val_option_unit, Value, " \n ()   \n ", Some(()));
    de_test!(val_option_none, Value, " \n none   \n ", None, Option<()>);
    de_test!(val_true, Value, "   true \n ", true);
    de_test!(val_false, Value, "   false \n ", false);

    macro_rules! int_test {
        ($uprim:ident, $iprim:ident, $s:literal, $n: expr, $ibig:literal) => {
            int_test!($uprim, $iprim, $s, $n, $ibig, "IntRange", "IntRange");
        };
        ($uprim:ident, $iprim:ident, $s:literal, $n: expr, $ibig:literal, $uerror:literal, $ierror: literal) => {
            paste::paste! {
                de_test!([<val_ $uprim>], Value, concat!(" ", $s, " \n "), $n as $uprim);
                de_test!([<val_ $iprim>], Value, concat!(" -", $s, " \n "), -$n as $iprim);
                de_test!(
                    #[should_panic(expected = $uerror)]
                    [<val_ $uprim _range_error>],
                    Value,
                    concat!(" -", $s, " \n "),
                    $n as $uprim, // dummy
                    $uprim,
                );
                de_test!(
                    #[should_panic(expected = $ierror)]
                    [<val_ $iprim _range_error>],
                    Value,
                    concat!(" ", $ibig, " \n "),
                    $n as $iprim, // dummy
                    $iprim,
                );
            }
        };
    }

    int_test!(u8, i8, "42", 42, "0x80");
    int_test!(u16, i16, "42", 42, "0x8000");
    int_test!(u32, i32, "42", 42, "0x80000000");
    int_test!(u64, i64, "42", 42, "0x8000000000000000");
    int_test!(u128, i128, "42", 42, "0x80000000000000000000000000000000");

    de_test!(
        val_f32,
        Value,
        "  1.2000000476837158 \n ",
        1.2000000476837158,
        f32
    );
    de_test!(
        val_f64,
        Value,
        "  1.2000000476837158 \n ",
        1.2000000476837158,
        f64
    );

    de_test!(val_char, Value, "  'A' \n ", 'A', char);

    de_test!(val_str, Value, r#" "hello" "#, "hello", &str);
    de_test!(val_str_escape, Value, r#" "\n" "#, "\n", &str);
    de_test!(
        val_str_mixed,
        Value,
        r#" "hello\nworld" "#,
        "hello\nworld",
        &str,
    );
    de_test!(
        val_string,
        Value,
        r#" "hello" "#,
        "hello",
        alloc::string::String
    );
    #[test]
    fn val_str_buffer_full() {
        use super::{Error, Flavor};
        use crate::from_slice_escaped;
        let mut buf = [0; 0];
        let e = from_slice_escaped::<&str>(Flavor::Value, br#" "\n" "#, &mut buf).unwrap_err();
        assert_eq!(Error::UnescapeBufferFull, *e);
    }

    de_test!(val_bytes, Value, r#" b"hello" "#, b"hello", &[u8]);
    de_test!(val_bytes_escape, Value, r#" b"\n" "#, b"\n", &[u8]);
    de_test!(
        val_bytes_mixed,
        Value,
        r#" b"hello\nworld" "#,
        b"hello\nworld",
        &[u8],
    );
    #[test]
    fn val_byte_buf() {
        use super::Flavor;
        use crate::from_slice_escaped;
        use serde_bytes::ByteBuf;
        let v: ByteBuf = from_slice_escaped(Flavor::Value, br#" b"hello" "#, &mut []).unwrap();
        assert_eq!(b"hello".as_ref(), v.into_vec());
    }
    #[test]
    fn val_bytes_buffer_full() {
        use super::{Error, Flavor};
        use crate::from_slice_escaped;
        let mut buf = [0; 0];
        let e = from_slice_escaped::<&[u8]>(Flavor::Value, br#" b"\n" "#, &mut buf).unwrap_err();
        assert_eq!(Error::UnescapeBufferFull, *e);
    }

    #[derive(Debug, PartialEq, Eq, serde::Deserialize)]
    struct NewtypeStruct(u8);
    de_test!(newtype_struct, Value, "  42 \n ", NewtypeStruct(42));

    de_test!(
        seq,
        List,
        "  1 \n 2, 3 \n ",
        [1, 2, 3].as_ref(),
        alloc::vec::Vec<u8>
    );

    de_test!(tuple, List, " 1 \n 2, 3 \n ", (1, 2, 3), (u8, u8, u8));

    #[derive(Debug, PartialEq, Eq, serde::Deserialize)]
    struct TupleStruct(u8, u8, u8);
    de_test!(tuple_struct, List, " 1 \n 2, 3 \n ", TupleStruct(1, 2, 3));

    de_test!(
        map,
        Map,
        " a = 1 \n b=2 \n ",
        [("a", 1u8), ("b", 2)].into_iter()
            .collect::<std::collections::HashMap<&str, u8>>(),
        std::collections::HashMap<&str, u8>,
    );

    #[derive(Debug, PartialEq, Eq, serde::Deserialize)]
    struct Struct {
        a: u8,
        b: u8,
    }
    de_test!(
        plain_struct,
        Map,
        " a = 1 \n b=2 \n ",
        Struct { a: 1, b: 2 }
    );

    #[derive(Debug, PartialEq, Eq, serde::Deserialize)]
    struct FlattenStruct {
        c: u8,
        #[serde(flatten)]
        sub: Struct,
    }
    de_test!(
        flatten_struct,
        Map,
        " a = 1 \n b=2, c=3 \n ",
        FlattenStruct {
            c: 3,
            sub: Struct { a: 1, b: 2 }
        },
    );

    #[derive(Debug, PartialEq, Eq, serde::Deserialize)]
    enum Enum {
        UnitVariant,
        NewtypeVariantSimple(u8),
        NewtypeVariantMap(Struct),
        NewtypeVariantSeq(TupleStruct),
        TupleVariant(char, char),
        StructVariant { foo: char, bar: char },
        NewtypeVariantTuple((char, char)),
        NewtypeVariantNewtype(NewtypeStruct),
        NewtypeVec(alloc::vec::Vec<u8>),
    }
    de_test!(enum_unit, Value, " UnitVariant \n ", Enum::UnitVariant);
    de_test!(
        enum_newtype_simple,
        Value,
        " NewtypeVariantSimple 42 \n ",
        Enum::NewtypeVariantSimple(42),
    );
    de_test!(
        enum_newtype_map,
        Value,
        " NewtypeVariantMap a=1 b=2 \n ",
        Enum::NewtypeVariantMap(Struct { a: 1, b: 2 }),
    );
    de_test!(
        enum_newtype_seq,
        Value,
        " NewtypeVariantSeq 1 2 3 \n ",
        Enum::NewtypeVariantSeq(TupleStruct(1, 2, 3)),
    );
    de_test!(
        enum_tuple,
        Value,
        " TupleVariant 'a' 'b' \n ",
        Enum::TupleVariant('a', 'b'),
    );
    de_test!(
        enum_struct,
        Value,
        " StructVariant foo='a' bar='b' \n ",
        Enum::StructVariant { foo: 'a', bar: 'b' },
    );
    de_test!(
        enum_newtype_tuple,
        Value,
        " NewtypeVariantTuple 'a' 'b' \n ",
        Enum::NewtypeVariantTuple(('a', 'b')),
    );
    de_test!(
        enum_newtype_newtype,
        Value,
        " NewtypeVariantNewtype 42 \n ",
        Enum::NewtypeVariantNewtype(NewtypeStruct(42)),
    );
    de_test!(
        enum_newtype_vec_empty,
        Value,
        " NewtypeVec \n ",
        Enum::NewtypeVec(alloc::vec![]),
    );
    de_test!(
        enum_newtype_vec_some,
        Value,
        " NewtypeVec 1 2 \n ",
        Enum::NewtypeVec(alloc::vec![1, 2]),
    );

    #[derive(Debug, PartialEq, serde::Deserialize)]
    #[serde(tag = "type")]
    enum InternalEnum {
        StructVariant { c: u8, d: u8 },
        NewtypeVariant(Struct),
        EnumInEnum { foo: Enum },
    }
    de_test!(
        internally_tagged_struct,
        Value,
        " {type=StructVariant, c=1, d=2} \n ",
        InternalEnum::StructVariant { c: 1, d: 2 },
    );
    de_test!(
        internally_tagged_newtype,
        Value,
        " {type=NewtypeVariant, a=1, b=2} \n ",
        InternalEnum::NewtypeVariant(Struct { a: 1, b: 2 }),
    );
    de_test!(
        internally_tagged_nested_enum_unit,
        Value,
        " {type=EnumInEnum, foo=UnitVariant} \n ",
        InternalEnum::EnumInEnum {
            foo: Enum::UnitVariant,
        },
    );
    de_test!(
        internally_tagged_nested_enum_tuple,
        Value,
        " {type=EnumInEnum, foo=(TupleVariant 'a' 'b')} \n ",
        InternalEnum::EnumInEnum {
            foo: Enum::TupleVariant('a', 'b'),
        },
    );
    de_test!(
        internally_tagged_nested_enum_struct,
        Value,
        " {type=EnumInEnum, foo=(StructVariant foo='a' bar='b')} \n ",
        InternalEnum::EnumInEnum {
            foo: Enum::StructVariant { foo: 'a', bar: 'b' },
        },
    );
    de_test!(
        #[should_panic(expected = "InvalidType")]
        internally_tagged_nested_enum_vec_empty_fails,
        Value,
        " {type=EnumInEnum, foo=NewtypeVec} \n ",
        InternalEnum::EnumInEnum {
            foo: Enum::NewtypeVec(alloc::vec![]),
        },
    );
    de_test!(
        internally_tagged_nested_enum_vec_some,
        Value,
        " {type=EnumInEnum, foo=(NewtypeVec 1 2)} \n ",
        InternalEnum::EnumInEnum {
            foo: Enum::NewtypeVec(alloc::vec![1, 2]),
        },
    );

    #[derive(Debug, PartialEq, serde::Deserialize)]
    #[serde(tag = "t", content = "c")]
    enum AdjacentEnum {
        StructVariant { c: u8, d: u8 },
        TupleVariant(u8, u8),
        NewtypeSimple(u8),
        NewtypeVariant(Struct),
        EnumInEnum { foo: Enum },
    }
    de_test!(
        adjacently_tagged_struct,
        Value,
        " {c={c=1, d=2}, t=StructVariant} \n ",
        AdjacentEnum::StructVariant { c: 1, d: 2 },
    );
    de_test!(
        adjacently_tagged_tuple,
        Value,
        " {c=[1, 2], t=TupleVariant} \n ",
        AdjacentEnum::TupleVariant(1, 2),
    );
    de_test!(
        adjacently_tagged_simple,
        Value,
        " {c=42, t=NewtypeSimple} \n ",
        AdjacentEnum::NewtypeSimple(42),
    );
    de_test!(
        adjacently_tagged_newtype,
        Value,
        " {c={a=1, b=2}, t=NewtypeVariant} \n ",
        AdjacentEnum::NewtypeVariant(Struct { a: 1, b: 2 }),
    );
    de_test!(
        adjacently_tagged_nested_enum_unit,
        Value,
        " {c={foo=UnitVariant}, t=EnumInEnum} \n ",
        AdjacentEnum::EnumInEnum {
            foo: Enum::UnitVariant,
        },
    );
    de_test!(
        adjacently_tagged_nested_enum_tuple,
        Value,
        " {c={foo=(TupleVariant 'a' 'b')}, t=EnumInEnum} \n ",
        AdjacentEnum::EnumInEnum {
            foo: Enum::TupleVariant('a', 'b'),
        },
    );
    de_test!(
        adjacently_tagged_nested_enum_struct,
        Value,
        " {c={foo=(StructVariant foo='a' bar='b')}, t=EnumInEnum} \n ",
        AdjacentEnum::EnumInEnum {
            foo: Enum::StructVariant { foo: 'a', bar: 'b' },
        },
    );
    de_test!(
        #[should_panic(expected = "InvalidType")]
        adjacently_tagged_nested_enum_vec_empty_fail,
        Value,
        " {c={foo=NewtypeVec}, t=EnumInEnum} \n ",
        AdjacentEnum::EnumInEnum {
            foo: Enum::NewtypeVec(alloc::vec![]),
        },
    );
    de_test!(
        adjacently_tagged_nested_enum_vec_empty_ok,
        Value,
        " {t=EnumInEnum, c={foo=NewtypeVec}} \n ",
        AdjacentEnum::EnumInEnum {
            foo: Enum::NewtypeVec(alloc::vec![]),
        },
    );
    de_test!(
        adjacently_tagged_nested_enum_vec_some,
        Value,
        " {c={foo=(NewtypeVec 1 2)}, t=EnumInEnum} \n ",
        AdjacentEnum::EnumInEnum {
            foo: Enum::NewtypeVec(alloc::vec![1, 2]),
        },
    );

    #[derive(Debug, PartialEq, serde::Deserialize)]
    #[serde(untagged)]
    enum UntaggedEnum {
        StructVariant { c: u8, d: u8 },
        TupleVariant(u8, u8),
        NewtypeSimple(u8),
        NewtypeVariant(Struct),
        EnumInEnum { foo: Enum },
    }
    de_test!(
        untagged_struct,
        Value,
        " {c=1, d=2} \n ",
        UntaggedEnum::StructVariant { c: 1, d: 2 },
    );
    de_test!(
        untagged_tagged_tuple,
        Value,
        " [1, 2] \n ",
        UntaggedEnum::TupleVariant(1, 2),
    );
    de_test!(
        untagged_simple,
        Value,
        " 42 \n ",
        UntaggedEnum::NewtypeSimple(42),
    );
    de_test!(
        untagged_newtype,
        Value,
        " {a=1, b=2} \n ",
        UntaggedEnum::NewtypeVariant(Struct { a: 1, b: 2 }),
    );
    de_test!(
        untagged_nested_enum_unit,
        Value,
        " {foo=UnitVariant} \n ",
        UntaggedEnum::EnumInEnum {
            foo: Enum::UnitVariant,
        },
    );
    de_test!(
        untagged_nested_enum_tuple,
        Value,
        " {foo=(TupleVariant 'a' 'b')} \n ",
        UntaggedEnum::EnumInEnum {
            foo: Enum::TupleVariant('a', 'b'),
        },
    );
    de_test!(
        untagged_nested_enum_struct,
        Value,
        " {foo=(StructVariant foo='a' bar='b')} \n ",
        UntaggedEnum::EnumInEnum {
            foo: Enum::StructVariant { foo: 'a', bar: 'b' },
        },
    );
    de_test!(
        #[should_panic(expected = "Custom")]
        untagged_nested_enum_vec_empty_fails,
        Value,
        " {foo=NewtypeVec} \n ",
        UntaggedEnum::EnumInEnum {
            foo: Enum::NewtypeVec(alloc::vec![]),
        },
    );
    de_test!(
        untagged_nested_enum_vec_some,
        Value,
        " {foo=(NewtypeVec 1 2)} \n ",
        UntaggedEnum::EnumInEnum {
            foo: Enum::NewtypeVec(alloc::vec![1, 2]),
        },
    );

    #[derive(Debug, PartialEq, serde::Deserialize)]
    #[serde(untagged)]
    enum AnyEnum<'a> {
        Unit(()),
        OptionBool(Option<bool>),
        Integer(crate::syntax::Int),
        Float(crate::syntax::Float),
        Character(char),
        String(&'a str),
        Bytes(&'a [u8]),
    }
    de_test!(any_unit, Value, " () \n ", AnyEnum::Unit(()));
    de_test!(any_none, Value, " none \n ", AnyEnum::OptionBool(None));
    de_test!(
        any_opt_true,
        Value,
        " true \n ",
        AnyEnum::OptionBool(Some(true)),
    );
    de_test!(any_integer, Value, " 42 \n ", AnyEnum::Integer(42));
    de_test!(any_float, Value, " 42.1 \n ", AnyEnum::Float(42.1));
    de_test!(any_char, Value, " 'a' \n ", AnyEnum::Character('a'));
    de_test!(any_str, Value, " \"ab\" \n ", AnyEnum::String("ab"));
    de_test!(
        any_bytes,
        Value,
        " b\"a\\xf0\" \n ",
        AnyEnum::Bytes(b"a\xf0"),
    );
    de_test!(
        vec_of_any,
        Value,
        " [42] \n ",
        alloc::vec![AnyEnum::Integer(42)],
        alloc::vec::Vec<AnyEnum>,
    );

    #[derive(Debug, PartialEq, serde::Deserialize)]
    struct PartialStruct {
        a: u8,
        b: serde::de::IgnoredAny,
    }
    de_test!(
        ignored_any_simple,
        Map,
        " b = 3 \n a = 42 ",
        PartialStruct {
            a: 42,
            b: Default::default()
        }
    );
    de_test!(
        ignored_any_seq,
        Map,
        " b = [1, 2] \n a = 42 ",
        PartialStruct {
            a: 42,
            b: Default::default()
        }
    );
    de_test!(
        ignored_any_map,
        Map,
        " b = {foo=1, bar=2} \n a = 42 ",
        PartialStruct {
            a: 42,
            b: Default::default()
        }
    );
    de_test!(
        ignored_any_enum,
        Map,
        " b = variant 3 \n a = 42 ",
        PartialStruct {
            a: 42,
            b: Default::default()
        }
    );
    de_test!(
        ignored_any_deep,
        Map,
        " b = {foo=[{bar=[1, 2, 3]}], bar=variant []} \n a = 42 ",
        PartialStruct {
            a: 42,
            b: Default::default()
        }
    );
}
