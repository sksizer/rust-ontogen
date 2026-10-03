//! Lookup keys in the request path (§8.1, §9).

use std::fmt;

use serde::{
    Deserialize, Deserializer,
    de::{self, Visitor},
};

/// The newtype name through which [`LookupKey`] asks `extract::Path` for
/// a segment that may not decode to UTF-8. Any other deserializer reads a
/// plain string.
pub(crate) const LOOKUP_KEY_TOKEN: &str = "$ontogen_jsonapi::LookupKey";

/// A path parameter that names something to look up: a resource's `{id}`,
/// or a relationship's `{rel}`.
///
/// The contract never validates such a key (§8.1): one that names nothing
/// is a `404` from the store, after every query and body check (§13.2 step
/// 9), with the entity's own `{entity}_not_found` code, which only the
/// handler knows. So a segment that does not percent-decode to UTF-8
/// (`GET /api/tasks/%FF`) is not rejected when the path is read, as it is
/// for a typed parameter. It is carried instead, [`as_str`](Self::as_str)
/// is `None`, and the handler answers the `404` where it would have called
/// the store. `Display` writes the segment as sent (`%FF`) for the error's
/// detail.
///
/// ```
/// use ontogen_jsonapi::LookupKey;
///
/// #[derive(Debug, PartialEq)]
/// enum AppError {
///     TaskNotFound(String),
/// }
///
/// fn get_task(id: &str) -> Result<String, AppError> {
///     Err(AppError::TaskNotFound(id.to_owned()))
/// }
///
/// // The handler, after its query checks, where it calls the store.
/// fn lookup(id: &LookupKey) -> Result<String, AppError> {
///     match id.as_str() {
///         Some(id) => get_task(id),
///         None => Err(AppError::TaskNotFound(id.to_string())),
///     }
/// }
///
/// assert_eq!(lookup(&LookupKey::from("nope")), Err(AppError::TaskNotFound("nope".to_owned())));
/// ```
///
/// A `{rel}` that does not decode is likewise not a relationship of the
/// type: `404 relationship_not_found` (§9).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LookupKey(Key);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Key {
    Text(String),
    /// The segment as sent, which is ASCII.
    Undecodable(String),
}

impl LookupKey {
    /// The decoded key, or `None` when the segment does not decode to
    /// UTF-8 and so names nothing.
    pub fn as_str(&self) -> Option<&str> {
        match &self.0 {
            Key::Text(text) => Some(text),
            Key::Undecodable(_) => None,
        }
    }
}

impl From<String> for LookupKey {
    fn from(key: String) -> Self {
        LookupKey(Key::Text(key))
    }
}

impl From<&str> for LookupKey {
    fn from(key: &str) -> Self {
        LookupKey(Key::Text(key.to_owned()))
    }
}

impl fmt::Display for LookupKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            Key::Text(text) | Key::Undecodable(text) => f.write_str(text),
        }
    }
}

impl<'de> Deserialize<'de> for LookupKey {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct KeyVisitor;

        impl<'de> Visitor<'de> for KeyVisitor {
            type Value = LookupKey;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a path segment")
            }

            fn visit_str<E: de::Error>(self, key: &str) -> Result<LookupKey, E> {
                Ok(LookupKey::from(key))
            }

            fn visit_string<E: de::Error>(self, key: String) -> Result<LookupKey, E> {
                Ok(LookupKey::from(key))
            }

            /// Only `extract::Path` sends bytes: the raw segment of a key
            /// that does not decode.
            fn visit_bytes<E: de::Error>(self, raw: &[u8]) -> Result<LookupKey, E> {
                Ok(LookupKey(Key::Undecodable(String::from_utf8_lossy(raw).into_owned())))
            }

            fn visit_newtype_struct<D: Deserializer<'de>>(self, deserializer: D) -> Result<LookupKey, D::Error> {
                deserializer.deserialize_str(self)
            }
        }

        deserializer.deserialize_newtype_struct(LOOKUP_KEY_TOKEN, KeyVisitor)
    }
}

#[cfg(test)]
impl LookupKey {
    pub(crate) fn undecodable(raw: &str) -> Self {
        LookupKey(Key::Undecodable(raw.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_decoded_key_is_its_text() {
        let key = LookupKey::from("a b");
        assert_eq!(key.as_str(), Some("a b"));
        assert_eq!(key.to_string(), "a b");
    }

    #[test]
    fn an_undecodable_key_names_nothing_and_displays_as_sent() {
        let key = LookupKey::undecodable("%FF");
        assert_eq!(key.as_str(), None);
        assert_eq!(key.to_string(), "%FF");
        // No decoded key equals it, whatever its text.
        assert_ne!(key, LookupKey::from("%FF"));
    }

    #[test]
    fn other_deserializers_read_a_string() {
        let key: LookupKey = serde_json::from_str(r#""ship-the-emitter""#).unwrap();
        assert_eq!(key.as_str(), Some("ship-the-emitter"));
    }
}
