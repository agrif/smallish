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
            Event::Value(Value::Str(_)) => self.deserialize_str(visitor),
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
        let v = self
            .de
            .next_with(as_variant!(Event::Value(Value::Str(v)) => v))?;
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
        let v = self
            .de
            .next_with(as_variant!(Event::Value(Value::Bytes(v)) => v))?;
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

#[cfg(test)]
mod test {
    extern crate alloc;

    #[test]
    fn val_str() {
        use crate::{from_slice_escaped, types::Escaped, Flavor};
        let v: Escaped<&str> = from_slice_escaped(Flavor::Value, br#" "\n" "#, &mut []).unwrap();
        assert_eq!("\\n", *v);
    }
    #[test]
    fn val_string() {
        use crate::{from_slice_escaped, types::Escaped, Flavor};
        let v: Escaped<alloc::string::String> =
            from_slice_escaped(Flavor::Value, br#" "\n" "#, &mut []).unwrap();
        assert_eq!("\\n", *v);
    }

    #[test]
    fn val_bytes() {
        use crate::{from_slice_escaped, types::Escaped, Flavor};
        let v: Escaped<&[u8]> = from_slice_escaped(Flavor::Value, br#" b"\n" "#, &mut []).unwrap();
        assert_eq!(b"\\n".as_ref(), *v);
    }
    #[test]
    fn val_byte_buf() {
        use crate::{from_slice_escaped, types::Escaped, Flavor};
        let v: Escaped<serde_bytes::ByteBuf> =
            from_slice_escaped(Flavor::Value, br#" b"\n" "#, &mut []).unwrap();
        assert_eq!(b"\\n".as_ref(), v.to_vec());
    }

    #[derive(Debug, PartialEq, Eq, serde::Deserialize)]
    struct Newtype<T>(T);
    #[test]
    fn newtype_str() {
        use crate::{from_slice_escaped, types::Escaped, Flavor};
        let v: Escaped<Newtype<&str>> =
            from_slice_escaped(Flavor::Value, br#" "\n" "#, &mut []).unwrap();
        let v = v.0;
        assert_eq!("\\n", v);
    }
    #[test]
    fn newtype_bytes() {
        use crate::{from_slice_escaped, types::Escaped, Flavor};
        let v: Escaped<Newtype<&[u8]>> =
            from_slice_escaped(Flavor::Value, br#" b"\n" "#, &mut []).unwrap();
        let v = v.0;
        assert_eq!(b"\\n".as_ref(), v);
    }

    #[test]
    fn escaped_located_str() {
        use crate::{
            from_slice_escaped,
            types::{Escaped, Located},
            Flavor,
        };
        let v: Escaped<Located<&str>> =
            from_slice_escaped(Flavor::Value, br#" "\n" "#, &mut []).unwrap();
        assert_eq!("\\n", **v);
    }
    #[test]
    fn escaped_located_bytes() {
        use crate::{
            from_slice_escaped,
            types::{Escaped, Located},
            Flavor,
        };
        let v: Escaped<Located<&[u8]>> =
            from_slice_escaped(Flavor::Value, br#" b"\n" "#, &mut []).unwrap();
        assert_eq!(b"\\n".as_ref(), **v);
    }

    #[derive(Debug, PartialEq, Eq, serde::Deserialize)]
    enum Enum<T> {
        Variant(crate::types::Escaped<T>),
    }
    #[test]
    fn enum_escaped_str() {
        use crate::{from_slice_escaped, Flavor};
        let v: Enum<&str> =
            from_slice_escaped(Flavor::Value, br#" Variant "\n" "#, &mut []).unwrap();
        let Enum::Variant(s) = v;
        assert_eq!("\\n", *s);
    }
    #[test]
    fn enum_escaped_bytes() {
        use crate::{from_slice_escaped, Flavor};
        let v: Enum<&[u8]> =
            from_slice_escaped(Flavor::Value, br#" Variant b"\n" "#, &mut []).unwrap();
        let Enum::Variant(s) = v;
        assert_eq!(b"\\n", *s);
    }

    #[derive(Debug, PartialEq, serde::Deserialize)]
    #[serde(untagged)]
    enum AnyEnum<'a> {
        Unit(()),
        String(&'a str),
        Bytes(&'a [u8]),
    }
    #[test]
    fn escaped_any_unit() {
        use crate::{from_slice_escaped, types::Escaped, Flavor};
        let v: Escaped<AnyEnum> = from_slice_escaped(Flavor::Value, br#" () "#, &mut []).unwrap();
        assert_eq!(*v, AnyEnum::Unit(()));
    }
    #[test]
    fn escaped_any_str() {
        use crate::{from_slice_escaped, types::Escaped, Flavor};
        let v: Escaped<AnyEnum> =
            from_slice_escaped(Flavor::Value, b" \"a\\n\" ", &mut []).unwrap();
        assert_eq!(*v, AnyEnum::String("a\\n"));
    }
    #[test]
    fn escaped_any_bytes() {
        use crate::{from_slice_escaped, types::Escaped, Flavor};
        let v: Escaped<AnyEnum> =
            from_slice_escaped(Flavor::Value, b" b\"a\xf0\" ", &mut []).unwrap();
        assert_eq!(*v, AnyEnum::Bytes(b"a\xf0"));
    }
}
