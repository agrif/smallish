use serde::de;

use crate::de::deserialize::{Error, SmallishDe};
use crate::types::Located;

#[derive(Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub(crate) struct LocatedHandler<'a, 'de, De> {
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

impl<'a, 'de, De> LocatedHandler<'a, 'de, De> {
    pub(crate) fn new(location: Located<'de, ()>, de: &'a mut De) -> Self {
        Self {
            location,
            de,
            state: LocatedState::Source,
        }
    }
}

impl<'a, 'de, De> SmallishDe<'de> for &mut LocatedHandler<'a, 'de, De>
where
    for<'b> &'b mut De: SmallishDe<'de>,
{
    #[inline]
    fn base(self) -> impl SmallishDe<'de> {
        self.de.base()
    }
}

impl<'a, 'de, De> de::Deserializer<'de> for &mut LocatedHandler<'a, 'de, De>
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
        byte_buf unit unit_struct newtype_struct seq tuple
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

impl<'a, 'de, De> de::MapAccess<'de> for LocatedHandler<'a, 'de, De>
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
                unreachable!("end of Located");
            }
        }
    }
}
