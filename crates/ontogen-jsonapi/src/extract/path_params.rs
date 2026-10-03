//! Path parameters read from the request path as sent.
//!
//! Axum decodes every captured segment up front, and one segment that is
//! not UTF-8 fails them all. Reading the raw segments here lets a
//! [`LookupKey`](crate::LookupKey) carry an undecodable segment while the
//! typed parameters beside it still parse or fail on their own.

use std::{fmt, slice, str::FromStr};

use serde::de::{self, DeserializeSeed, Deserializer, IntoDeserializer, MapAccess, SeqAccess, Visitor};

use crate::{path::LOOKUP_KEY_TOKEN, query::percent_decode};

/// One captured parameter.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Param<'a> {
    name: &'a str,
    /// The segment as sent.
    raw: &'a str,
    /// The segment percent-decoded, or `None` when that is not UTF-8.
    text: Option<String>,
}

/// Pairs each `{name}` capture of `template` with the segment of `path` in
/// the same position, counting from the end so that a path with a prefix
/// the template lacks still lines up.
///
/// `None` when a capture has no segment, or the template has a segment
/// that captures something other than one whole segment, which generated
/// routes never do.
pub(super) fn params<'a>(template: &'a str, path: &'a str) -> Option<Vec<Param<'a>>> {
    let template: Vec<&str> = template.split('/').collect();
    let path: Vec<&str> = path.split('/').collect();
    let shift = path.len().checked_sub(template.len());
    let mut params = Vec::new();
    for (i, segment) in template.iter().enumerate() {
        if !segment.contains(['{', '}']) {
            continue;
        }
        let name = segment.strip_prefix('{')?.strip_suffix('}')?;
        if name.is_empty() || name.contains(['{', '}', '*']) {
            return None;
        }
        let raw = *path.get(i + shift?)?;
        params.push(Param { name, raw, text: String::from_utf8(percent_decode(raw, false)).ok() });
    }
    Some(params)
}

/// Why the parameters do not deserialize.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum PathError {
    /// A value that does not parse as its type: the client's error.
    Invalid(String),
    /// Parameters that do not fit the type: the route's error.
    Route(String),
}

impl PathError {
    fn in_param(self, name: &str) -> Self {
        match self {
            PathError::Invalid(detail) => PathError::Invalid(format!("path parameter `{name}`: {detail}")),
            route => route,
        }
    }
}

impl fmt::Display for PathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PathError::Invalid(detail) | PathError::Route(detail) => f.write_str(detail),
        }
    }
}

impl std::error::Error for PathError {}

impl de::Error for PathError {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        PathError::Invalid(msg.to_string())
    }

    fn invalid_length(len: usize, expected: &dyn de::Expected) -> Self {
        PathError::Route(format!("the route has {len} path parameters; the handler expects {expected}"))
    }

    fn missing_field(field: &'static str) -> Self {
        PathError::Route(format!("the route has no `{field}` path parameter"))
    }

    fn unknown_field(field: &str, _expected: &'static [&'static str]) -> Self {
        PathError::Route(format!("the handler does not expect the path parameter `{field}`"))
    }
}

/// All of a route's parameters: a tuple or sequence in path order, a
/// struct or map by name, or a single value when there is exactly one.
pub(super) struct Params<'p, 'a>(pub(super) &'p [Param<'a>]);

impl<'p, 'a> Params<'p, 'a> {
    fn single(&self) -> Result<&'p Param<'a>, PathError> {
        match self.0 {
            [param] => Ok(param),
            params => Err(PathError::Route(format!(
                "the route has {} path parameters; the handler expects one",
                params.len()
            ))),
        }
    }
}

macro_rules! single_value {
    ($($method:ident)*) => {$(
        fn $method<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, PathError> {
            let param = self.single()?;
            param.$method(visitor).map_err(|e| e.in_param(param.name))
        }
    )*};
}

impl<'de> Deserializer<'de> for Params<'_, '_> {
    type Error = PathError;

    single_value! {
        deserialize_any deserialize_bool deserialize_i8 deserialize_i16 deserialize_i32 deserialize_i64
        deserialize_i128 deserialize_u8 deserialize_u16 deserialize_u32 deserialize_u64 deserialize_u128
        deserialize_f32 deserialize_f64 deserialize_char deserialize_str deserialize_string deserialize_bytes
        deserialize_byte_buf deserialize_identifier
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, PathError> {
        visitor.visit_some(self)
    }

    fn deserialize_unit<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, PathError> {
        if self.0.is_empty() {
            visitor.visit_unit()
        } else {
            Err(de::Error::invalid_length(self.0.len(), &"no path parameters"))
        }
    }

    fn deserialize_unit_struct<V: Visitor<'de>>(self, _name: &'static str, visitor: V) -> Result<V::Value, PathError> {
        self.deserialize_unit(visitor)
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        visitor: V,
    ) -> Result<V::Value, PathError> {
        if name == LOOKUP_KEY_TOKEN {
            let param = self.single()?;
            param.deserialize_newtype_struct(name, visitor).map_err(|e| e.in_param(param.name))
        } else {
            visitor.visit_newtype_struct(self)
        }
    }

    fn deserialize_seq<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, PathError> {
        visitor.visit_seq(Seq(self.0.iter()))
    }

    fn deserialize_tuple<V: Visitor<'de>>(self, len: usize, visitor: V) -> Result<V::Value, PathError> {
        if len == self.0.len() {
            self.deserialize_seq(visitor)
        } else {
            Err(de::Error::invalid_length(self.0.len(), &format!("{len}").as_str()))
        }
    }

    fn deserialize_tuple_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        len: usize,
        visitor: V,
    ) -> Result<V::Value, PathError> {
        self.deserialize_tuple(len, visitor)
    }

    fn deserialize_map<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, PathError> {
        visitor.visit_map(Map { params: self.0.iter(), value: None })
    }

    fn deserialize_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, PathError> {
        self.deserialize_map(visitor)
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        name: &'static str,
        variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, PathError> {
        let param = self.single()?;
        param.deserialize_enum(name, variants, visitor).map_err(|e| e.in_param(param.name))
    }

    fn deserialize_ignored_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, PathError> {
        visitor.visit_unit()
    }
}

struct Seq<'p, 'a>(slice::Iter<'p, Param<'a>>);

impl<'de> SeqAccess<'de> for Seq<'_, '_> {
    type Error = PathError;

    fn next_element_seed<T: DeserializeSeed<'de>>(&mut self, seed: T) -> Result<Option<T::Value>, PathError> {
        match self.0.next() {
            Some(param) => seed.deserialize(param).map(Some).map_err(|e| e.in_param(param.name)),
            None => Ok(None),
        }
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.0.len())
    }
}

struct Map<'p, 'a> {
    params: slice::Iter<'p, Param<'a>>,
    value: Option<&'p Param<'a>>,
}

impl<'de> MapAccess<'de> for Map<'_, '_> {
    type Error = PathError;

    fn next_key_seed<K: DeserializeSeed<'de>>(&mut self, seed: K) -> Result<Option<K::Value>, PathError> {
        let Some(param) = self.params.next() else { return Ok(None) };
        self.value = Some(param);
        seed.deserialize(param.name.into_deserializer()).map(Some)
    }

    fn next_value_seed<V: DeserializeSeed<'de>>(&mut self, seed: V) -> Result<V::Value, PathError> {
        let param = self.value.take().ok_or_else(|| PathError::Route("a value was read before its name".to_owned()))?;
        seed.deserialize(param).map_err(|e| e.in_param(param.name))
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.params.len())
    }
}

impl Param<'_> {
    fn text(&self) -> Result<&str, PathError> {
        self.text.as_deref().ok_or_else(|| PathError::Invalid("not valid UTF-8 once percent-decoded".to_owned()))
    }

    fn parse<T: FromStr>(&self) -> Result<T, PathError>
    where
        T::Err: fmt::Display,
    {
        let text = self.text()?;
        text.parse().map_err(|err| PathError::Invalid(format!("`{text}` does not parse: {err}")))
    }

    fn not_a_segment(&self, kind: &str) -> PathError {
        PathError::Route(format!("`{}` is one path segment, which the handler expects as {kind}", self.name))
    }
}

macro_rules! parse_value {
    ($($method:ident => $visit:ident,)*) => {$(
        fn $method<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, PathError> {
            visitor.$visit(self.parse()?)
        }
    )*};
}

impl<'de> Deserializer<'de> for &Param<'_> {
    type Error = PathError;

    parse_value! {
        deserialize_bool => visit_bool,
        deserialize_i8 => visit_i8,
        deserialize_i16 => visit_i16,
        deserialize_i32 => visit_i32,
        deserialize_i64 => visit_i64,
        deserialize_i128 => visit_i128,
        deserialize_u8 => visit_u8,
        deserialize_u16 => visit_u16,
        deserialize_u32 => visit_u32,
        deserialize_u64 => visit_u64,
        deserialize_u128 => visit_u128,
        deserialize_f32 => visit_f32,
        deserialize_f64 => visit_f64,
        deserialize_char => visit_char,
    }

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, PathError> {
        visitor.visit_str(self.text()?)
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, PathError> {
        visitor.visit_some(self)
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        visitor: V,
    ) -> Result<V::Value, PathError> {
        match (&self.text, name == LOOKUP_KEY_TOKEN) {
            (Some(text), true) => visitor.visit_str(text),
            (None, true) => visitor.visit_bytes(self.raw.as_bytes()),
            (_, false) => visitor.visit_newtype_struct(self),
        }
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, PathError> {
        visitor.visit_enum(IntoDeserializer::<'de, PathError>::into_deserializer(self.text()?))
    }

    fn deserialize_unit<V: Visitor<'de>>(self, _visitor: V) -> Result<V::Value, PathError> {
        Err(self.not_a_segment("a unit"))
    }

    fn deserialize_unit_struct<V: Visitor<'de>>(self, _name: &'static str, _visitor: V) -> Result<V::Value, PathError> {
        Err(self.not_a_segment("a unit"))
    }

    fn deserialize_seq<V: Visitor<'de>>(self, _visitor: V) -> Result<V::Value, PathError> {
        Err(self.not_a_segment("a sequence"))
    }

    fn deserialize_tuple<V: Visitor<'de>>(self, _len: usize, _visitor: V) -> Result<V::Value, PathError> {
        Err(self.not_a_segment("a tuple"))
    }

    fn deserialize_tuple_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _len: usize,
        _visitor: V,
    ) -> Result<V::Value, PathError> {
        Err(self.not_a_segment("a tuple"))
    }

    fn deserialize_map<V: Visitor<'de>>(self, _visitor: V) -> Result<V::Value, PathError> {
        Err(self.not_a_segment("a map"))
    }

    fn deserialize_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _fields: &'static [&'static str],
        _visitor: V,
    ) -> Result<V::Value, PathError> {
        Err(self.not_a_segment("a struct"))
    }

    fn deserialize_ignored_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, PathError> {
        visitor.visit_unit()
    }

    serde::forward_to_deserialize_any! { str string bytes byte_buf identifier }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde::{Deserialize, de::DeserializeOwned};

    use super::*;
    use crate::LookupKey;

    fn read<T: DeserializeOwned>(template: &str, path: &str) -> Result<T, PathError> {
        T::deserialize(Params(&params(template, path).expect("the template fits the path")))
    }

    #[test]
    fn captures_pair_with_segments_from_the_end() {
        let found = params("/api/projects/{project_id}/tasks/{id}", "/api/projects/7/tasks/a%20b").unwrap();
        assert_eq!(
            found,
            [
                Param { name: "project_id", raw: "7", text: Some("7".to_owned()) },
                Param { name: "id", raw: "a%20b", text: Some("a b".to_owned()) },
            ]
        );
        // A path with a prefix the template lacks.
        assert_eq!(params("/api/tasks/{id}", "/v1/api/tasks/x").unwrap()[0].raw, "x");
        // `+` is a literal in a path, and an undecodable segment is kept.
        let found = params("/api/tasks/{id}", "/api/tasks/a+%FF").unwrap();
        assert_eq!(found[0], Param { name: "id", raw: "a+%FF", text: None });
        assert_eq!(params("/api/tasks/{id}", "/api/tasks/a+b").unwrap()[0].text.as_deref(), Some("a+b"));
    }

    #[test]
    fn templates_that_are_not_whole_segment_captures_do_not_fit() {
        assert_eq!(params("/api/{*rest}", "/api/a/b"), None);
        assert_eq!(params("/api/x{id}", "/api/x1"), None);
        assert_eq!(params("/api/{}", "/api/x"), None);
        assert_eq!(params("/a/b/{id}", "/b/x"), None);
    }

    #[test]
    fn values_parse_as_their_types() {
        assert_eq!(read::<u32>("/t/{id}", "/t/42"), Ok(42));
        assert_eq!(read::<String>("/t/{id}", "/t/a%2Fb"), Ok("a/b".to_owned()));
        assert_eq!(read::<(u8, String)>("/p/{p}/t/{id}", "/p/1/t/x"), Ok((1, "x".to_owned())));
        assert_eq!(read::<Option<i64>>("/t/{id}", "/t/-3"), Ok(Some(-3)));
        assert_eq!(read::<bool>("/t/{id}", "/t/true"), Ok(true));

        #[derive(Debug, PartialEq, Deserialize)]
        struct Scope {
            project_id: u16,
        }
        assert_eq!(read::<Scope>("/p/{project_id}/t/{id}", "/p/9/t/x"), Ok(Scope { project_id: 9 }));
        let map = read::<BTreeMap<String, String>>("/p/{a}/{b}", "/p/1/2").unwrap();
        assert_eq!(map.into_iter().collect::<Vec<_>>(), [("a".into(), "1".into()), ("b".into(), "2".into())]);

        #[derive(Debug, PartialEq, Deserialize)]
        #[serde(rename_all = "lowercase")]
        enum Kind {
            Open,
        }
        assert_eq!(read::<Kind>("/k/{kind}", "/k/open"), Ok(Kind::Open));
    }

    #[test]
    fn bad_values_are_invalid_and_name_their_parameter() {
        let Err(PathError::Invalid(detail)) = read::<u32>("/t/{id}", "/t/x") else { panic!() };
        assert!(detail.starts_with("path parameter `id`: "), "{detail}");
        let Err(PathError::Invalid(detail)) = read::<(u32, String)>("/p/{p}/t/{id}", "/p/1/t/%FF") else { panic!() };
        assert_eq!(detail, "path parameter `id`: not valid UTF-8 once percent-decoded");
        assert!(matches!(read::<String>("/t/{id}", "/t/%C3"), Err(PathError::Invalid(_))));
    }

    #[test]
    fn a_lookup_key_carries_an_undecodable_segment() {
        let key = read::<LookupKey>("/t/{id}", "/t/%FF").unwrap();
        assert_eq!((key.as_str(), key.to_string()), (None, "%FF".to_owned()));
        assert_eq!(read::<LookupKey>("/t/{id}", "/t/a%20b").unwrap().as_str(), Some("a b"));
        let (project, key) = read::<(u32, LookupKey)>("/p/{p}/t/{id}", "/p/7/t/%FF%FE").unwrap();
        assert_eq!((project, key.as_str(), key.to_string()), (7, None, "%FF%FE".to_owned()));
        // The typed parameter beside it still fails on its own.
        assert!(matches!(read::<(u32, LookupKey)>("/p/{p}/t/{id}", "/p/x/t/%FF"), Err(PathError::Invalid(_))));
    }

    #[test]
    fn parameters_that_do_not_fit_the_type_are_the_routes_error() {
        assert!(matches!(read::<String>("/p/{p}/t/{id}", "/p/1/t/x"), Err(PathError::Route(_))));
        assert!(matches!(read::<(String, String)>("/t/{id}", "/t/x"), Err(PathError::Route(_))));
        assert!(matches!(read::<(String,)>("/p/{p}/t/{id}", "/p/1/t/x"), Err(PathError::Route(_))));

        #[derive(Debug, Deserialize)]
        #[allow(dead_code)]
        struct Wants {
            other: String,
        }
        assert!(matches!(read::<Wants>("/t/{id}", "/t/x"), Err(PathError::Route(_))));
        assert!(matches!(read::<Vec<String>>("/t/{id}", "/t/x").map(|v| v.len()), Ok(1)));
        assert!(matches!(read::<String>("/t", "/t"), Err(PathError::Route(_))));
        assert!(matches!(read::<()>("/t", "/t"), Ok(())));
    }
}
