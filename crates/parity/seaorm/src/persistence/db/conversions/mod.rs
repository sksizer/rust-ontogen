pub mod generated;

/// Serialize an enum value to the string the column stores: its serde
/// name, the value markdown frontmatter holds too.
pub fn enum_to_string<T: serde::Serialize>(value: &T) -> String {
    let json = serde_json::to_string(value).unwrap_or_default();
    json.trim_matches('"').to_string()
}
