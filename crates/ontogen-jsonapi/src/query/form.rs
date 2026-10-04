//! Serde over decoded query values: one value read as `axum::extract::Query`
//! reads a field, a struct read from the members of one family, and the
//! field names a struct declares.

use std::fmt;

use serde::{
    de::{
        self, DeserializeOwned, DeserializeSeed, MapAccess, Visitor,
        value::{BorrowedStrDeserializer, MapAccessDeserializer},
    },
    forward_to_deserialize_any,
};

/// Why reading a value or a struct failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Error {
    /// A message from serde that names no member.
    Custom(String),
    /// serde found no member for a required field.
    MissingField(&'static str),
    /// The member is given more than once.
    Repeated(String),
    /// serde rejected the member's value.
    Member { member: String, message: String },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Custom(message) | Error::Member { message, .. } => f.write_str(message),
            Error::MissingField(field) => write!(f, "missing field `{field}`"),
            Error::Repeated(member) => write!(f, "`{member}` is given more than once"),
        }
    }
}

impl std::error::Error for Error {}

impl de::Error for Error {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        Error::Custom(msg.to_string())
    }

    // serde's derive reports a missing field through this; keeping the name
    // lets the error name the member the request lacks.
    fn missing_field(field: &'static str) -> Self {
        Error::MissingField(field)
    }
}

/// One decoded value, deserialized as `serde_urlencoded` deserializes a
/// field's value (its private `Part`), which is what `axum::extract::Query`
/// uses: anything reads as a string, `Option` is `Some` whenever the value
/// is present, an enum is a unit variant named by the value, and numbers,
/// `bool` and `char` are parsed with `FromStr`.
pub(super) struct Value<'a>(pub(super) &'a str);

macro_rules! parse_value {
    ($($method:ident => $ty:ty, $visit:ident;)*) => {
        $(
            fn $method<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
                match self.0.parse::<$ty>() {
                    Ok(value) => visitor.$visit(value),
                    Err(err) => Err(de::Error::custom(err)),
                }
            }
        )*
    };
}

impl<'de> de::Deserializer<'de> for Value<'de> {
    type Error = Error;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        visitor.visit_borrowed_str(self.0)
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        visitor.visit_some(self)
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        visitor.visit_enum(UnitVariant(self.0))
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(self, _name: &'static str, visitor: V) -> Result<V::Value, Error> {
        visitor.visit_newtype_struct(self)
    }

    parse_value! {
        deserialize_bool => bool, visit_bool;
        deserialize_u8 => u8, visit_u8;
        deserialize_u16 => u16, visit_u16;
        deserialize_u32 => u32, visit_u32;
        deserialize_u64 => u64, visit_u64;
        deserialize_u128 => u128, visit_u128;
        deserialize_i8 => i8, visit_i8;
        deserialize_i16 => i16, visit_i16;
        deserialize_i32 => i32, visit_i32;
        deserialize_i64 => i64, visit_i64;
        deserialize_i128 => i128, visit_i128;
        deserialize_f32 => f32, visit_f32;
        deserialize_f64 => f64, visit_f64;
        deserialize_char => char, visit_char;
    }

    forward_to_deserialize_any! {
        str string bytes byte_buf unit unit_struct seq tuple tuple_struct map struct identifier ignored_any
    }
}

/// An enum named by a value: only a unit variant can be.
struct UnitVariant<'a>(&'a str);

impl<'de> de::EnumAccess<'de> for UnitVariant<'de> {
    type Error = Error;
    type Variant = UnitOnly;

    fn variant_seed<S: DeserializeSeed<'de>>(self, seed: S) -> Result<(S::Value, UnitOnly), Error> {
        let variant = seed.deserialize(BorrowedStrDeserializer::new(self.0))?;
        Ok((variant, UnitOnly))
    }
}

struct UnitOnly;

impl<'de> de::VariantAccess<'de> for UnitOnly {
    type Error = Error;

    fn unit_variant(self) -> Result<(), Error> {
        Ok(())
    }

    fn newtype_variant_seed<T: DeserializeSeed<'de>>(self, _seed: T) -> Result<T::Value, Error> {
        Err(de::Error::custom("expected unit variant"))
    }

    fn tuple_variant<V: Visitor<'de>>(self, _len: usize, _visitor: V) -> Result<V::Value, Error> {
        Err(de::Error::custom("expected unit variant"))
    }

    fn struct_variant<V: Visitor<'de>>(self, _fields: &'static [&'static str], _visitor: V) -> Result<V::Value, Error> {
        Err(de::Error::custom("expected unit variant"))
    }
}

/// One member of a family as the request gave it: its name, its first
/// value, and whether it was repeated.
pub(super) type Member<'a> = (&'a str, &'a str, bool);

/// `T` read from `members`, visited in the order given. A repeated member
/// fails when it is reached, so the first failing member in that order is
/// the error, whatever kind of failure it is.
pub(super) fn read_struct<'a, T, I>(members: I) -> Result<T, Error>
where
    T: DeserializeOwned,
    I: Iterator<Item = Member<'a>>,
{
    T::deserialize(MapAccessDeserializer::new(Members { members, pending: None }))
}

struct Members<'a, I> {
    members: I,
    /// The member whose name was handed out and whose value is next.
    pending: Option<(&'a str, &'a str)>,
}

impl<'a, I: Iterator<Item = Member<'a>>> MapAccess<'a> for Members<'a, I> {
    type Error = Error;

    fn next_key_seed<K: DeserializeSeed<'a>>(&mut self, seed: K) -> Result<Option<K::Value>, Error> {
        let Some((member, value, repeated)) = self.members.next() else { return Ok(None) };
        if repeated {
            return Err(Error::Repeated(member.to_owned()));
        }
        self.pending = Some((member, value));
        seed.deserialize(BorrowedStrDeserializer::new(member)).map(Some)
    }

    fn next_value_seed<V: DeserializeSeed<'a>>(&mut self, seed: V) -> Result<V::Value, Error> {
        let Some((member, value)) = self.pending.take() else {
            return Err(de::Error::custom("a value was read before its member"));
        };
        seed.deserialize(Value(value))
            .map_err(|err| Error::Member { member: member.to_owned(), message: err.to_string() })
    }
}

/// The field names `T` passes to `deserialize_struct`, or `&[]` when `T`
/// does not deserialize as a struct.
pub(super) fn struct_fields<T: DeserializeOwned>() -> &'static [&'static str] {
    match T::deserialize(Probe) {
        Err(Probed::Fields(fields)) => fields,
        _ => &[],
    }
}

/// A deserializer that fails at once, capturing the fields when asked for
/// a struct.
struct Probe;

#[derive(Debug)]
enum Probed {
    Fields(&'static [&'static str]),
    NotAStruct,
}

impl fmt::Display for Probed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("probed for struct fields")
    }
}

impl std::error::Error for Probed {}

impl de::Error for Probed {
    fn custom<T: fmt::Display>(_msg: T) -> Self {
        Probed::NotAStruct
    }
}

impl<'de> de::Deserializer<'de> for Probe {
    type Error = Probed;

    fn deserialize_any<V: Visitor<'de>>(self, _visitor: V) -> Result<V::Value, Probed> {
        Err(Probed::NotAStruct)
    }

    fn deserialize_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        fields: &'static [&'static str],
        _visitor: V,
    ) -> Result<V::Value, Probed> {
        Err(Probed::Fields(fields))
    }

    forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string bytes byte_buf option unit
        unit_struct newtype_struct seq tuple tuple_struct map enum identifier ignored_any
    }
}
