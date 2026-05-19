//! Property tests: for each of serde's eight `rename_all` modes, the wire
//! names [`ontogen_ts::RenameAll::apply_to_field`] / `apply_to_variant`
//! emit MUST match what `serde_json::to_string` actually puts on the JSON
//! wire. Failure here means our generated TS would lie about the wire
//! shape and downstream consumers would see runtime keys that don't appear
//! in their `.d.ts`-equivalent type defs — which is exactly the foot-gun
//! ontogen-ts exists to prevent.
//!
//! Each test:
//!
//! 1. Declares a fixture type with `#[serde(rename_all = "MODE")]` plus a
//!    handful of fields/variants that exercise word boundaries (multi-word
//!    snake_case, all-caps acronyms, single-word).
//! 2. Serializes a value of that type through `serde_json::to_string` and
//!    pulls the keys (struct) or the string body (C-style enum variant)
//!    out of the resulting JSON.
//! 3. Independently applies `RenameAll::MODE.apply_to_field` /
//!    `apply_to_variant` to the same raw identifiers.
//! 4. Asserts the two lists are equal.
//!
//! Why round-trip the actual serde behavior instead of using a static
//! expected-output table? Because serde's case rules are the ground truth
//! we have to mirror — if a future serde release changes its behavior, the
//! property tests fail loudly and we update the rename engine to match,
//! rather than discovering the drift at runtime in user crates.

use ontogen_ts::RenameAll;
use serde::Serialize;
use serde_json::Value;

// ── Helper: pull keys / string body from a serde_json round-trip. ─────────

fn struct_keys<T: Serialize>(value: &T) -> Vec<String> {
    let json = serde_json::to_string(value).expect("serialize");
    let parsed: Value = serde_json::from_str(&json).expect("parse JSON");
    parsed.as_object().unwrap_or_else(|| panic!("expected JSON object, got {json}")).keys().cloned().collect()
}

fn variant_str<T: Serialize>(value: &T) -> String {
    let json = serde_json::to_string(value).expect("serialize");
    serde_json::from_str::<String>(&json)
        .unwrap_or_else(|err| panic!("expected JSON string variant body, got {json} ({err})"))
}

// ─────────────────────────────────────────────────────────────────────────
// Struct fields × 8 rename_all modes.
//
// Three field idents chosen to exercise (a) multi-word snake_case, (b)
// snake_case with digit suffix, (c) single-word. Each is a `u32` so the
// serialized JSON is `{"<wire>": 0, ...}`.
// ─────────────────────────────────────────────────────────────────────────

const RAW_FIELD_IDENTS: &[&str] = &["parse_url_handler", "parse_v2", "single"];

#[derive(Default, Serialize)]
struct AsIsFields {
    parse_url_handler: u32,
    parse_v2: u32,
    single: u32,
}

#[derive(Default, Serialize)]
#[serde(rename_all = "lowercase")]
struct LowercaseFields {
    parse_url_handler: u32,
    parse_v2: u32,
    single: u32,
}

#[derive(Default, Serialize)]
#[serde(rename_all = "UPPERCASE")]
struct UppercaseFields {
    parse_url_handler: u32,
    parse_v2: u32,
    single: u32,
}

#[derive(Default, Serialize)]
#[serde(rename_all = "PascalCase")]
struct PascalFields {
    parse_url_handler: u32,
    parse_v2: u32,
    single: u32,
}

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct CamelFields {
    parse_url_handler: u32,
    parse_v2: u32,
    single: u32,
}

#[derive(Default, Serialize)]
#[serde(rename_all = "snake_case")]
struct SnakeFields {
    parse_url_handler: u32,
    parse_v2: u32,
    single: u32,
}

#[derive(Default, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
struct ScreamingSnakeFields {
    parse_url_handler: u32,
    parse_v2: u32,
    single: u32,
}

#[derive(Default, Serialize)]
#[serde(rename_all = "kebab-case")]
struct KebabFields {
    parse_url_handler: u32,
    parse_v2: u32,
    single: u32,
}

#[derive(Default, Serialize)]
#[serde(rename_all = "SCREAMING-KEBAB-CASE")]
struct ScreamingKebabFields {
    parse_url_handler: u32,
    parse_v2: u32,
    single: u32,
}

/// Assert that ontogen-ts's `apply_to_field` outputs match serde_json's
/// wire keys for a given mode. Failures point at the diverging pair.
fn assert_field_mode_roundtrips<T: Serialize + Default>(mode: RenameAll, _value_witness: &T) {
    let expected: Vec<String> = RAW_FIELD_IDENTS.iter().map(|f| mode.apply_to_field(f)).collect();
    let actual = struct_keys(&T::default());
    assert_eq!(actual, expected, "rename_all={mode:?} field-side wire mismatch");
}

#[test]
fn as_is_field_keys_match_raw_idents() {
    // Sanity: a struct with no rename_all serializes field idents verbatim.
    let actual = struct_keys(&AsIsFields::default());
    let expected: Vec<String> = RAW_FIELD_IDENTS.iter().map(|s| s.to_string()).collect();
    assert_eq!(actual, expected);
}

#[test]
fn lowercase_fields_roundtrip() {
    assert_field_mode_roundtrips(RenameAll::Lowercase, &LowercaseFields::default());
}

#[test]
fn uppercase_fields_roundtrip() {
    assert_field_mode_roundtrips(RenameAll::Uppercase, &UppercaseFields::default());
}

#[test]
fn pascal_case_fields_roundtrip() {
    assert_field_mode_roundtrips(RenameAll::PascalCase, &PascalFields::default());
}

#[test]
fn camel_case_fields_roundtrip() {
    assert_field_mode_roundtrips(RenameAll::CamelCase, &CamelFields::default());
}

#[test]
fn snake_case_fields_roundtrip() {
    assert_field_mode_roundtrips(RenameAll::SnakeCase, &SnakeFields::default());
}

#[test]
fn screaming_snake_case_fields_roundtrip() {
    assert_field_mode_roundtrips(RenameAll::ScreamingSnakeCase, &ScreamingSnakeFields::default());
}

#[test]
fn kebab_case_fields_roundtrip() {
    assert_field_mode_roundtrips(RenameAll::KebabCase, &KebabFields::default());
}

#[test]
fn screaming_kebab_case_fields_roundtrip() {
    assert_field_mode_roundtrips(RenameAll::ScreamingKebabCase, &ScreamingKebabFields::default());
}

// ─────────────────────────────────────────────────────────────────────────
// Enum variants × 8 modes. C-style variants serialize as JSON strings, so
// we pull `variant_str` and compare.
//
// Three variant idents chosen for: (a) two-word PascalCase, (b) ALL-CAPS
// acronym-bearing, (c) single-word.
// ─────────────────────────────────────────────────────────────────────────

const RAW_VARIANT_IDENTS: &[&str] = &["ApiClient", "HTMLParser", "Single"];

#[derive(Serialize)]
enum AsIsVariants {
    ApiClient,
    HTMLParser,
    Single,
}

#[derive(Serialize)]
#[serde(rename_all = "lowercase")]
enum LowercaseVariants {
    ApiClient,
    HTMLParser,
    Single,
}

#[derive(Serialize)]
#[serde(rename_all = "UPPERCASE")]
enum UppercaseVariants {
    ApiClient,
    HTMLParser,
    Single,
}

#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
enum PascalVariants {
    ApiClient,
    HTMLParser,
    Single,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
enum CamelVariants {
    ApiClient,
    HTMLParser,
    Single,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum SnakeVariants {
    ApiClient,
    HTMLParser,
    Single,
}

#[derive(Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum ScreamingSnakeVariants {
    ApiClient,
    HTMLParser,
    Single,
}

#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
enum KebabVariants {
    ApiClient,
    HTMLParser,
    Single,
}

#[derive(Serialize)]
#[serde(rename_all = "SCREAMING-KEBAB-CASE")]
enum ScreamingKebabVariants {
    ApiClient,
    HTMLParser,
    Single,
}

/// Assert that ontogen-ts's `apply_to_variant` outputs match serde_json's
/// wire strings for each variant under a given mode.
fn assert_variant_mode_roundtrips(mode: RenameAll, values: &[&dyn erased::DynSerialize]) {
    let expected: Vec<String> = RAW_VARIANT_IDENTS.iter().map(|v| mode.apply_to_variant(v)).collect();
    let actual: Vec<String> = values.iter().map(|v| variant_str_dyn(*v)).collect();
    assert_eq!(actual, expected, "rename_all={mode:?} variant-side wire mismatch");
}

/// Tiny dyn-Serialize bridge so the test helper can take a heterogenous
/// list of enum values. (We can't make `assert_variant_mode_roundtrips`
/// generic over an enum's three variants without listing them explicitly,
/// and dyn Serialize isn't object-safe directly.)
mod erased {
    use serde::Serialize;

    pub trait DynSerialize {
        fn to_json(&self) -> String;
    }

    impl<T: Serialize> DynSerialize for T {
        fn to_json(&self) -> String {
            serde_json::to_string(self).expect("serialize")
        }
    }
}

fn variant_str_dyn(d: &dyn erased::DynSerialize) -> String {
    let json = d.to_json();
    serde_json::from_str::<String>(&json)
        .unwrap_or_else(|err| panic!("expected JSON string variant body, got {json} ({err})"))
}

#[test]
fn as_is_variant_strings_match_raw_idents() {
    let actual = vec![
        variant_str(&AsIsVariants::ApiClient),
        variant_str(&AsIsVariants::HTMLParser),
        variant_str(&AsIsVariants::Single),
    ];
    let expected: Vec<String> = RAW_VARIANT_IDENTS.iter().map(|s| s.to_string()).collect();
    assert_eq!(actual, expected);
}

#[test]
fn lowercase_variants_roundtrip() {
    assert_variant_mode_roundtrips(
        RenameAll::Lowercase,
        &[
            &LowercaseVariants::ApiClient as &dyn erased::DynSerialize,
            &LowercaseVariants::HTMLParser,
            &LowercaseVariants::Single,
        ],
    );
}

#[test]
fn uppercase_variants_roundtrip() {
    assert_variant_mode_roundtrips(
        RenameAll::Uppercase,
        &[
            &UppercaseVariants::ApiClient as &dyn erased::DynSerialize,
            &UppercaseVariants::HTMLParser,
            &UppercaseVariants::Single,
        ],
    );
}

#[test]
fn pascal_case_variants_roundtrip() {
    assert_variant_mode_roundtrips(
        RenameAll::PascalCase,
        &[
            &PascalVariants::ApiClient as &dyn erased::DynSerialize,
            &PascalVariants::HTMLParser,
            &PascalVariants::Single,
        ],
    );
}

#[test]
fn camel_case_variants_roundtrip() {
    assert_variant_mode_roundtrips(
        RenameAll::CamelCase,
        &[&CamelVariants::ApiClient as &dyn erased::DynSerialize, &CamelVariants::HTMLParser, &CamelVariants::Single],
    );
}

#[test]
fn snake_case_variants_roundtrip() {
    assert_variant_mode_roundtrips(
        RenameAll::SnakeCase,
        &[&SnakeVariants::ApiClient as &dyn erased::DynSerialize, &SnakeVariants::HTMLParser, &SnakeVariants::Single],
    );
}

#[test]
fn screaming_snake_case_variants_roundtrip() {
    assert_variant_mode_roundtrips(
        RenameAll::ScreamingSnakeCase,
        &[
            &ScreamingSnakeVariants::ApiClient as &dyn erased::DynSerialize,
            &ScreamingSnakeVariants::HTMLParser,
            &ScreamingSnakeVariants::Single,
        ],
    );
}

#[test]
fn kebab_case_variants_roundtrip() {
    assert_variant_mode_roundtrips(
        RenameAll::KebabCase,
        &[&KebabVariants::ApiClient as &dyn erased::DynSerialize, &KebabVariants::HTMLParser, &KebabVariants::Single],
    );
}

#[test]
fn screaming_kebab_case_variants_roundtrip() {
    assert_variant_mode_roundtrips(
        RenameAll::ScreamingKebabCase,
        &[
            &ScreamingKebabVariants::ApiClient as &dyn erased::DynSerialize,
            &ScreamingKebabVariants::HTMLParser,
            &ScreamingKebabVariants::Single,
        ],
    );
}

// ─────────────────────────────────────────────────────────────────────────
// Field-level `#[serde(rename = "wireName")]` wins over container
// `rename_all`. This is serde's documented precedence rule.
// ─────────────────────────────────────────────────────────────────────────

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct FieldRenameOverridesContainer {
    #[serde(rename = "_internal_id")]
    parse_url_handler: u32,
    // No field-level rename — container's camelCase applies.
    age_years: u32,
}

#[test]
fn field_level_rename_wins_over_container_rename_all() {
    let actual = struct_keys(&FieldRenameOverridesContainer::default());
    assert_eq!(actual, vec!["_internal_id".to_string(), "ageYears".to_string()]);
}

// ─────────────────────────────────────────────────────────────────────────
// `#[serde(skip)]` drops the field from JSON.
// ─────────────────────────────────────────────────────────────────────────

#[derive(Default, Serialize)]
struct SkipDropsField {
    visible: u32,
    #[serde(skip)]
    #[allow(dead_code)]
    hidden: u32,
}

#[test]
fn serde_skip_drops_field_from_wire() {
    let actual = struct_keys(&SkipDropsField::default());
    assert_eq!(actual, vec!["visible".to_string()]);
}
