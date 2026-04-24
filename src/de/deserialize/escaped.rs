use as_variant::as_variant;
use serde::de;

use crate::de::deserialize::{forward_to_inner_deserialize, Error, SmallishDe};
use crate::syntax::{Event, Value};

#[derive(Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub(crate) struct EscapedHandler<'a, De> {
    de: &'a mut De,
}

impl<'a, De> EscapedHandler<'a, De> {
    pub(crate) fn new(de: &'a mut De) -> Self {
        Self { de }
    }
}

impl<'a, 'de, De> SmallishDe<'de> for &mut EscapedHandler<'a, De>
where
    for<'b> &'b mut De: SmallishDe<'de>,
{
    #[inline]
    fn base(self) -> impl SmallishDe<'de> {
        self.de.base()
    }
}

impl<'a, 'de, De> de::Deserializer<'de> for &mut EscapedHandler<'a, De>
where
    for<'b> &'b mut De: SmallishDe<'de>,
{
    type Error = Error;

    forward_to_inner_deserialize! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char
        option unit unit_struct seq tuple
        tuple_struct map enum identifier ignored_any
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
}
