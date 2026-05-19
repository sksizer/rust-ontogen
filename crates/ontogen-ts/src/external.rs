//! External types: canonical-path → TS rendering map.
//!
//! Some Rust types ship from crates ontogen-ts doesn't scan (chrono, uuid,
//! time, url, etc.), so the walker can't recurse into their definitions to
//! emit a TS shape. Instead it consults this table and uses the configured
//! rendering as a terminal value.
//!
//! Phase-1 ships a default set keyed by full canonical path
//! (`chrono::DateTime`, `uuid::Uuid`, etc.). Users can override or extend
//! via [`crate::EmitConfig::external_types`]; user-provided entries win on
//! conflict. Generic args are stripped at match time so `DateTime<Utc>`,
//! `DateTime<Local>`, `DateTime<FixedOffset>` all hit the same entry.

use std::collections::BTreeMap;

use crate::types::TypePath;

/// Default external-type renderings shipped by ontogen-ts. Each tuple is
/// `(canonical_path, ts_rendering)`. Lifted directly from OF-015's design
/// pass.
///
/// Deliberately excluded because the wire encoding depends on consumer
/// serde flags: `std::time::Duration`, `std::time::SystemTime`,
/// `bytes::Bytes`, `rust_decimal::Decimal`, `bigdecimal::BigDecimal`.
pub(crate) const DEFAULT_EXTERNAL_TYPES: &[(&str, &str)] = &[
    // Date / time
    ("chrono::DateTime", "string"),
    ("chrono::NaiveDate", "string"),
    ("chrono::NaiveDateTime", "string"),
    ("chrono::NaiveTime", "string"),
    ("time::OffsetDateTime", "string"),
    ("time::PrimitiveDateTime", "string"),
    ("time::Date", "string"),
    ("time::Time", "string"),
    // IDs / strings
    ("uuid::Uuid", "string"),
    ("url::Url", "string"),
    ("std::path::PathBuf", "string"),
    // Networking
    ("std::net::IpAddr", "string"),
    ("std::net::Ipv4Addr", "string"),
    ("std::net::Ipv6Addr", "string"),
    // JSON escape hatch — value of any shape.
    ("serde_json::Value", "unknown"),
];

/// Resolve a canonical type path against the external-types table.
///
/// User-provided overrides in `user_overrides` win on conflict with the
/// shipped defaults. Returns `Some(rendering)` if the path is in either
/// table; `None` if it's a user-crate type the walker should recurse into.
///
/// Matching ignores generic args — the canonical path passed in is the
/// already-canonicalized one with args stripped (the caller handles
/// stripping).
pub(crate) fn resolve(canonical: &TypePath, user_overrides: &BTreeMap<String, String>) -> Option<String> {
    let key = canonical_path_string(canonical);
    if let Some(override_value) = user_overrides.get(&key) {
        return Some(override_value.clone());
    }
    for (default_key, default_value) in DEFAULT_EXTERNAL_TYPES {
        if *default_key == key {
            return Some((*default_value).to_string());
        }
    }
    None
}

/// Render a [`TypePath`] as a `::`-joined canonical string for table lookup.
pub(crate) fn canonical_path_string(path: &TypePath) -> String {
    path.segments().join("::")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::TypePath;

    fn tp(segments: &[&str]) -> TypePath {
        TypePath::new(segments.iter().map(|s| (*s).to_string()).collect()).expect("non-empty")
    }

    #[test]
    fn default_chrono_datetime_resolves_to_string() {
        let overrides = BTreeMap::new();
        let path = tp(&["chrono", "DateTime"]);
        assert_eq!(resolve(&path, &overrides).as_deref(), Some("string"));
    }

    #[test]
    fn default_uuid_resolves_to_string() {
        let overrides = BTreeMap::new();
        assert_eq!(resolve(&tp(&["uuid", "Uuid"]), &overrides).as_deref(), Some("string"));
    }

    #[test]
    fn default_serde_json_value_resolves_to_unknown() {
        let overrides = BTreeMap::new();
        assert_eq!(resolve(&tp(&["serde_json", "Value"]), &overrides).as_deref(), Some("unknown"));
    }

    #[test]
    fn unknown_path_returns_none() {
        let overrides = BTreeMap::new();
        assert_eq!(resolve(&tp(&["my_crate", "Workout"]), &overrides), None);
    }

    #[test]
    fn user_override_supplements_defaults() {
        let mut overrides = BTreeMap::new();
        overrides.insert("my_crate::WorkoutId".to_string(), "string".to_string());
        assert_eq!(resolve(&tp(&["my_crate", "WorkoutId"]), &overrides).as_deref(), Some("string"));
    }

    #[test]
    fn user_override_wins_on_conflict() {
        // User maps chrono::DateTime to a custom TS rendering — beats the
        // default `string`.
        let mut overrides = BTreeMap::new();
        overrides.insert("chrono::DateTime".to_string(), "Moment".to_string());
        assert_eq!(resolve(&tp(&["chrono", "DateTime"]), &overrides).as_deref(), Some("Moment"));
    }

    #[test]
    fn user_override_to_unknown_works() {
        // Mapping a user type to `unknown` is a valid escape hatch for
        // wire-shape opacity.
        let mut overrides = BTreeMap::new();
        overrides.insert("my_crate::Blob".to_string(), "unknown".to_string());
        assert_eq!(resolve(&tp(&["my_crate", "Blob"]), &overrides).as_deref(), Some("unknown"));
    }

    #[test]
    fn all_default_paths_resolve() {
        // Sanity: every entry in the const table is reachable through
        // `resolve` with the same path it declares.
        let overrides = BTreeMap::new();
        for (path_str, expected) in DEFAULT_EXTERNAL_TYPES {
            let segments: Vec<&str> = path_str.split("::").collect();
            let path = tp(&segments);
            let actual = resolve(&path, &overrides);
            assert_eq!(actual.as_deref(), Some(*expected), "default `{path_str}` should resolve");
        }
    }
}
