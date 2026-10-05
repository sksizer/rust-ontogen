//! Serde over query values: one value read as `axum::extract::Query`
//! reads a field, or as a sequence of comma-separated items, a struct read
//! from the members of one family, and the field names a struct declares.

use std::{borrow::Cow, cell::Cell, fmt};

use serde::{
    de::{
        self, DeserializeOwned, DeserializeSeed, MapAccess, SeqAccess, Visitor,
        value::{BorrowedStrDeserializer, CowStrDeserializer, MapAccessDeserializer},
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
    /// serde saw one field twice, as two members that name it (a serde
    /// alias); no member is attached to it.
    Duplicate(&'static str),
    /// serde rejected the member's value.
    Member { member: String, message: String },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Custom(message) | Error::Member { message, .. } => f.write_str(message),
            Error::MissingField(field) => write!(f, "missing field `{field}`"),
            Error::Repeated(member) => write!(f, "`{member}` is given more than once"),
            Error::Duplicate(field) => write!(f, "duplicate field `{field}`"),
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

    // A field reached through two members, which only an alias allows.
    fn duplicate_field(field: &'static str) -> Self {
        Error::Duplicate(field)
    }
}

/// One value, deserialized as `serde_urlencoded` deserializes a field's
/// value (its private `Part`), which is what `axum::extract::Query` uses:
/// anything reads as a string, `Option` is `Some` whenever the value is
/// present, an enum is a unit variant named by the value, and numbers,
/// `bool` and `char` are parsed with `FromStr`.
///
/// A sequence, which `serde_urlencoded` cannot read from one value, is the
/// value as sent split at each literal `,`, each item percent-decoded and
/// read as above: a comma inside an item arrives as `%2C`, which the split
/// leaves alone. An empty value is the empty sequence. An item is never a
/// sequence itself.
pub(super) struct Value<'a> {
    /// The value, percent-decoded.
    text: Cow<'a, str>,
    /// The value as sent, still percent-encoded, for a sequence to split;
    /// `None` for an item of one.
    raw: Option<&'a str>,
}

impl<'a> Value<'a> {
    /// A value as `decoded` from its query-string form `raw`.
    pub(super) fn new(decoded: &'a str, raw: &'a str) -> Self {
        Value { text: Cow::Borrowed(decoded), raw: Some(raw) }
    }
}

macro_rules! parse_value {
    ($($method:ident => $ty:ty, $visit:ident;)*) => {
        $(
            fn $method<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
                match self.text.parse::<$ty>() {
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
        match self.text {
            Cow::Borrowed(text) => visitor.visit_borrowed_str(text),
            Cow::Owned(text) => visitor.visit_string(text),
        }
    }

    fn deserialize_seq<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        match self.raw {
            Some("") => visitor.visit_seq(Items { pieces: None, index: 0 }),
            Some(raw) => visitor.visit_seq(Items { pieces: Some(raw.split(',')), index: 0 }),
            None => self.deserialize_any(visitor),
        }
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
        visitor.visit_enum(UnitVariant(self.text))
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
        str string bytes byte_buf unit unit_struct tuple tuple_struct map struct identifier ignored_any
    }
}

/// The items of a sequence value: the pieces of the value as sent between
/// literal commas, `None` for an empty value.
struct Items<'a> {
    pieces: Option<std::str::Split<'a, char>>,
    /// The number of items read so far.
    index: usize,
}

impl<'de> SeqAccess<'de> for Items<'de> {
    type Error = Error;

    fn next_element_seed<T: DeserializeSeed<'de>>(&mut self, seed: T) -> Result<Option<T::Value>, Error> {
        let Some(piece) = self.pieces.as_mut().and_then(Iterator::next) else { return Ok(None) };
        let item = super::decode(piece);
        self.index += 1;
        let at = self.index;
        seed.deserialize(Value { text: Cow::Owned(item.clone()), raw: None })
            .map(Some)
            .map_err(|err| de::Error::custom(format!("item {at} (`{item}`): {err}")))
    }
}

/// An enum named by a value: only a unit variant can be.
struct UnitVariant<'a>(Cow<'a, str>);

impl<'de> de::EnumAccess<'de> for UnitVariant<'de> {
    type Error = Error;
    type Variant = UnitOnly;

    fn variant_seed<S: DeserializeSeed<'de>>(self, seed: S) -> Result<(S::Value, UnitOnly), Error> {
        let variant = seed.deserialize(CowStrDeserializer::new(self.0))?;
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
/// value decoded and as sent, and whether it was repeated.
pub(super) type Member<'a> = (&'a str, &'a str, &'a str, bool);

/// `T` read from `members`, visited in the order given. A repeated member
/// fails when it is reached, so the first failing member in that order is
/// the error, whatever kind of failure it is.
pub(super) fn read_struct<'a, T, I>(members: I) -> Result<T, Error>
where
    T: DeserializeOwned,
    I: Iterator<Item = Member<'a>>,
{
    // serde's derive raises a duplicate field after reading the later
    // member's name and before its value, so the member whose name was
    // handed out last is the one that triggered it.
    let last = Cell::new(None);
    T::deserialize(MapAccessDeserializer::new(Members { members, pending: None, last: &last })).map_err(|err| match err
    {
        Error::Duplicate(field) => match last.get() {
            Some(member) => Error::Member { member: member.to_owned(), message: format!("duplicate field `{field}`") },
            None => Error::Custom(format!("duplicate field `{field}`")),
        },
        other => other,
    })
}

struct Members<'a, 'r, I> {
    members: I,
    last: &'r Cell<Option<&'a str>>,
    /// The member whose name was handed out, with its value decoded and as
    /// sent, which is next.
    pending: Option<(&'a str, &'a str, &'a str)>,
}

impl<'a, I: Iterator<Item = Member<'a>>> MapAccess<'a> for Members<'a, '_, I> {
    type Error = Error;

    fn next_key_seed<K: DeserializeSeed<'a>>(&mut self, seed: K) -> Result<Option<K::Value>, Error> {
        let Some((member, value, raw, repeated)) = self.members.next() else { return Ok(None) };
        if repeated {
            return Err(Error::Repeated(member.to_owned()));
        }
        self.pending = Some((member, value, raw));
        self.last.set(Some(member));
        seed.deserialize(BorrowedStrDeserializer::new(member)).map(Some)
    }

    fn next_value_seed<V: DeserializeSeed<'a>>(&mut self, seed: V) -> Result<V::Value, Error> {
        let Some((member, value, raw)) = self.pending.take() else {
            return Err(de::Error::custom("a value was read before its member"));
        };
        seed.deserialize(Value::new(value, raw))
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

#[cfg(test)]
mod tests {
    use std::fmt::Debug;

    use serde::Deserialize;

    use super::*;

    #[derive(Debug, PartialEq, Deserialize)]
    #[serde(rename_all = "lowercase")]
    enum Mode {
        Fast,
        Slow,
    }

    #[derive(Debug, PartialEq, Deserialize)]
    struct Newtype(u8);

    /// The reference reader: `serde_urlencoded` reading one pair into a field.
    fn urlencoded<T: DeserializeOwned>(value: &str) -> Result<T, String> {
        #[derive(Deserialize)]
        struct One<T> {
            v: T,
        }
        let pair = format!("v={}", encode(value));
        serde_urlencoded::from_str::<One<T>>(&pair).map(|one| one.v).map_err(|err| err.to_string())
    }

    fn encode(value: &str) -> String {
        let mut out = String::new();
        for byte in value.bytes() {
            match byte {
                b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(byte as char),
                _ => out.push_str(&format!("%{byte:02X}")),
            }
        }
        out
    }

    fn ours<T: DeserializeOwned>(value: &str) -> Result<T, String> {
        T::deserialize(Value::new(value, value)).map_err(|err| err.to_string())
    }

    fn agree<T: DeserializeOwned + Debug>(value: &str) {
        let (a, b) = (urlencoded::<T>(value), ours::<T>(value));
        match (&a, &b) {
            // Compared as text so that NaN agrees with itself.
            (Ok(x), Ok(y)) => {
                assert_eq!(format!("{x:?}"), format!("{y:?}"), "{value:?} as {}", std::any::type_name::<T>())
            }
            (Err(_), Err(_)) => {}
            _ => panic!("{value:?} as {}: urlencoded {a:?}, ours {b:?}", std::any::type_name::<T>()),
        }
    }

    const VALUES: &[&str] = &[
        "",
        "0",
        "1",
        "-1",
        "+1",
        "+",
        "-",
        "--1",
        "007",
        "255",
        "256",
        "-129",
        "128",
        "65536",
        "4294967296",
        "9223372036854775807",
        "9223372036854775808",
        "-9223372036854775808",
        "-9223372036854775809",
        "18446744073709551615",
        "18446744073709551616",
        "true",
        "false",
        "True",
        "TRUE",
        "1.5",
        "-1.5",
        "+1.5",
        ".5",
        "5.",
        "1e3",
        "1E-3",
        "inf",
        "-inf",
        "NaN",
        "nan",
        "infinity",
        "1.7976931348623157e309",
        "fast",
        "slow",
        "Fast",
        "other",
        "a b",
        "a+b",
        "a%2Bb",
        "a%20b",
        "%",
        "%zz",
        "caf\u{e9}",
        "x",
        " 1",
        "1 ",
    ];

    #[test]
    fn the_value_reader_agrees_with_serde_urlencoded() {
        for value in VALUES {
            agree::<bool>(value);
            agree::<u8>(value);
            agree::<u16>(value);
            agree::<u32>(value);
            agree::<u64>(value);
            agree::<i8>(value);
            agree::<i16>(value);
            agree::<i32>(value);
            agree::<i64>(value);
            agree::<f32>(value);
            agree::<f64>(value);
            agree::<String>(value);
            agree::<Mode>(value);
            agree::<Option<String>>(value);
            agree::<Option<u32>>(value);
            agree::<Option<bool>>(value);
            agree::<Option<Mode>>(value);
            agree::<Newtype>(value);
        }
    }

    #[test]
    fn an_empty_value_is_some_for_a_string_and_an_error_for_a_number() {
        assert_eq!(ours::<Option<String>>(""), Ok(Some(String::new())));
        assert!(ours::<Option<u32>>("").is_err());
        assert!(ours::<u32>("").is_err());
        assert_eq!(ours::<Option<String>>(""), urlencoded::<Option<String>>(""));
    }

    #[test]
    fn plus_and_percent_encoding_decode_before_the_reader_sees_them() {
        // The reader takes decoded text: `a+b` arrives as `a b`.
        let decoded: Vec<(String, String)> = serde_urlencoded::from_str("v=a+b%2Bc%20d").unwrap();
        assert_eq!(decoded[0].1, "a b+c d");
        assert_eq!(ours::<String>(&decoded[0].1), Ok("a b+c d".to_owned()));
        assert_eq!(urlencoded::<String>("a b+c d"), Ok("a b+c d".to_owned()));
    }

    #[test]
    fn leading_signs_and_overflow_are_parsed_as_from_str_does() {
        assert_eq!(ours::<i32>("+7"), Ok(7));
        assert_eq!(ours::<i32>("-7"), Ok(-7));
        assert!(ours::<u8>("-1").is_err());
        assert!(ours::<u8>("256").is_err());
        assert!(ours::<i8>("-129").is_err());
    }

    // Intentional differences: serde_urlencoded's value deserializer has no
    // 128-bit or `char` support, so those fields cannot be read from a
    // query at all there. They read here through `FromStr`, which accepts
    // a superset and breaks nothing.
    #[test]
    fn the_reader_also_reads_128_bit_integers_and_chars() {
        assert!(urlencoded::<u128>("5").is_err());
        assert!(urlencoded::<i128>("-5").is_err());
        assert_eq!(ours::<u128>("5"), Ok(5));
        assert_eq!(ours::<i128>("-5"), Ok(-5));
        assert!(ours::<u128>("-5").is_err());
        assert_eq!(ours::<char>("x"), Ok('x'));
        assert!(ours::<char>("xy").is_err());
    }
}
