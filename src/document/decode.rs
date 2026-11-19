use super::{Document, Number};
use serde::de::{self, DeserializeSeed, IntoDeserializer, MapAccess, SeqAccess, Visitor};
use std::sync::Arc;

type Result<T> = std::result::Result<T, serde_json::Error>;

#[derive(Clone, Copy)]
pub(super) struct Deserializer<'a> {
    pub document: &'a Document,
    pub depth: u8,
}
impl Deserializer<'_> {
    #[inline]
    fn nested(self) -> Result<Self> {
        if self.depth >= 127 {
            return Err(de::Error::custom("recursion limit exceeded"));
        }
        Ok(Self {
            depth: self.depth + 1,
            ..self
        })
    }
}
macro_rules! number {
    ($($method:ident)*) => {$(
        #[inline]
    fn $method<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
            if let Document::Number(n) = self.document {
                let text = n.text();
                let mut decoder = serde_json::Deserializer::from_reader(text.as_bytes());
                let value = de::Deserializer::$method(&mut decoder, visitor)?;
                decoder.end()?;
                Ok(value)
            } else {
                self.deserialize_any(visitor)
            }
        }
    )*};
}
impl<'de> de::Deserializer<'de> for Deserializer<'de> {
    type Error = serde_json::Error;
    #[inline]
    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        match self.document {
            Document::Null => visitor.visit_unit(),
            Document::Bool(b) => visitor.visit_bool(*b),
            Document::String(s) => visitor.visit_borrowed_str(s),
            Document::Number(Number::Small(n)) if *n >= 0 => visitor.visit_u64(*n as u64),
            Document::Number(Number::Small(n)) => visitor.visit_i64(*n),
            Document::Number(Number::Float(n)) if n.is_finite() => visitor.visit_f64(*n),
            Document::Number(n) => {
                let text = n.text();
                let mut decoder = serde_json::Deserializer::from_reader(text.as_bytes());
                let value = de::Deserializer::deserialize_any(&mut decoder, visitor)?;
                decoder.end()?;
                Ok(value)
            }
            Document::Array(values) => {
                let nested = self.nested()?;
                let mut access = Sequence {
                    values: values.iter(),
                    depth: nested.depth,
                };
                let value = visitor.visit_seq(&mut access)?;
                if access.values.len() != 0 {
                    return Err(de::Error::custom("trailing array elements"));
                }
                Ok(value)
            }
            Document::Object(values) => {
                let nested = self.nested()?;
                let mut access = Object {
                    values: values.iter(),
                    value: None,
                    depth: nested.depth,
                };
                let value = visitor.visit_map(&mut access)?;
                if access.value.is_some() || access.values.len() != 0 {
                    return Err(de::Error::custom("trailing object entries"));
                }
                Ok(value)
            }
        }
    }
    number!(deserialize_i128 deserialize_u128 deserialize_f32 deserialize_f64);
    #[inline]
    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        if matches!(self.document, Document::Null) {
            visitor.visit_none()
        } else {
            visitor.visit_some(self)
        }
    }
    #[inline]
    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        _: &'static str,
        visitor: V,
    ) -> Result<V::Value> {
        visitor.visit_newtype_struct(self)
    }
    #[inline]
    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _: &'static str,
        _: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value> {
        match self.document {
            Document::String(s) => visitor.visit_enum(s.as_str().into_deserializer()),
            Document::Object(values) if values.len() == 1 => {
                let nested = self.nested()?;
                visitor.visit_enum(de::value::MapAccessDeserializer::new(Object {
                    values: values.iter(),
                    value: None,
                    depth: nested.depth,
                }))
            }
            _ => Err(de::Error::custom(
                "expected a string or single-key enum object",
            )),
        }
    }
    #[inline]
    fn deserialize_bytes<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        if let Document::String(s) = self.document {
            visitor.visit_borrowed_bytes(s.as_bytes())
        } else {
            self.deserialize_any(visitor)
        }
    }
    #[inline]
    fn deserialize_byte_buf<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        self.deserialize_bytes(visitor)
    }
    #[inline]
    fn deserialize_ignored_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        #[inline]
        fn validate(document: &Document) -> Result<()> {
            match document {
                Document::Number(Number::Float(n)) if !n.is_finite() => {
                    return Err(de::Error::custom("expected value"));
                }
                Document::Number(Number::Integer(n)) => {
                    serde_json::from_str::<de::IgnoredAny>(n)?;
                }
                Document::Array(a) => {
                    for value in a {
                        validate(value)?;
                    }
                }
                Document::Object(o) => {
                    for (_, value) in o {
                        validate(value)?;
                    }
                }
                _ => {}
            }
            Ok(())
        }
        validate(self.document)?;
        visitor.visit_unit()
    }
    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 u8 u16 u32 u64 char str string unit unit_struct seq tuple tuple_struct map struct identifier
    }
}
struct Sequence<'a> {
    values: std::slice::Iter<'a, Document>,
    depth: u8,
}
impl<'de> SeqAccess<'de> for Sequence<'de> {
    type Error = serde_json::Error;
    #[inline]
    fn size_hint(&self) -> Option<usize> {
        Some(self.values.len())
    }
    #[inline]
    fn next_element_seed<T: DeserializeSeed<'de>>(&mut self, seed: T) -> Result<Option<T::Value>> {
        self.values
            .next()
            .map(|document| {
                seed.deserialize(Deserializer {
                    document,
                    depth: self.depth,
                })
            })
            .transpose()
    }
}
struct Object<'a> {
    values: std::slice::Iter<'a, (Arc<str>, Document)>,
    value: Option<&'a Document>,
    depth: u8,
}
impl<'de> MapAccess<'de> for Object<'de> {
    type Error = serde_json::Error;
    #[inline]
    fn size_hint(&self) -> Option<usize> {
        Some(self.values.len())
    }
    #[inline]
    fn next_key_seed<K: DeserializeSeed<'de>>(&mut self, seed: K) -> Result<Option<K::Value>> {
        let Some((key, value)) = self.values.next() else {
            return Ok(None);
        };
        self.value = Some(value);
        seed.deserialize(Key(key)).map(Some)
    }
    #[inline]
    fn next_value_seed<V: DeserializeSeed<'de>>(&mut self, seed: V) -> Result<V::Value> {
        let document = self
            .value
            .take()
            .ok_or_else(|| de::Error::custom("missing map value"))?;
        seed.deserialize(Deserializer {
            document,
            depth: self.depth,
        })
    }
}
struct Key<'a>(&'a str);
macro_rules! numeric_key {
    ($($method:ident)*) => {$(
        #[inline]
    fn $method<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
            if !super::valid_number(self.0) {
                return Err(de::Error::custom("expected numeric key"));
            }
            let mut decoder = serde_json::Deserializer::from_str(self.0);
            let value = de::Deserializer::$method(&mut decoder, visitor)?;
            decoder.end()?;
            Ok(value)
        }
    )*};
}
impl<'de> de::Deserializer<'de> for Key<'de> {
    type Error = serde_json::Error;
    #[inline]
    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        visitor.visit_borrowed_str(self.0)
    }
    numeric_key!(deserialize_i8 deserialize_i16 deserialize_i32 deserialize_i64 deserialize_i128
        deserialize_u8 deserialize_u16 deserialize_u32 deserialize_u64 deserialize_u128
        deserialize_f32 deserialize_f64);
    #[inline]
    fn deserialize_bool<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        match self.0 {
            "true" => visitor.visit_bool(true),
            "false" => visitor.visit_bool(false),
            _ => self.deserialize_any(visitor),
        }
    }
    #[inline]
    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        visitor.visit_some(self)
    }
    #[inline]
    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        _: &'static str,
        visitor: V,
    ) -> Result<V::Value> {
        visitor.visit_newtype_struct(self)
    }
    #[inline]
    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _: &'static str,
        _: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value> {
        visitor.visit_enum(self.0.into_deserializer())
    }
    #[inline]
    fn deserialize_bytes<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        visitor.visit_borrowed_bytes(self.0.as_bytes())
    }
    #[inline]
    fn deserialize_byte_buf<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        self.deserialize_bytes(visitor)
    }
    serde::forward_to_deserialize_any! {
        char str string unit unit_struct seq tuple tuple_struct map struct identifier ignored_any
    }
}
