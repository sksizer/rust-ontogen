//! The helpers generated conversions import. Every SeaORM consumer writes these
//! same two functions.

pub mod generated;

pub use generated::*;

/// An enum's serde name, with the JSON quotes stripped.
pub fn enum_to_string<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_default().trim_matches('"').to_string()
}

/// A JSON-array column of strings. Generated code calls this without `?`, so a
/// malformed column reads as an empty list.
pub fn decode_json_vec(json: &str) -> Vec<String> {
    serde_json::from_str(json).unwrap_or_default()
}
