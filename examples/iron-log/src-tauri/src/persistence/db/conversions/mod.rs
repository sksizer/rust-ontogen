// Generated conversion modules are placed in generated/ by ontogen.
// This file re-exports them and provides shared helper functions.
pub mod generated;
pub use generated::*;

/// Decode a JSON-encoded Vec<String> from a DB column.
pub fn decode_json_vec(json: &str) -> Vec<String> {
    serde_json::from_str(json).unwrap_or_default()
}

/// Serialize an enum value to its string representation for DB storage.
pub fn enum_to_string<T: serde::Serialize>(value: &T) -> String {
    let json = serde_json::to_string(value).unwrap_or_default();
    // Strip surrounding quotes from JSON string serialization
    json.trim_matches('"').to_string()
}
