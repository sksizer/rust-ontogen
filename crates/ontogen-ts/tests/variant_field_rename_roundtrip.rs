//! Round-trip proof that struct-variant FIELD names match the wire
//! (issue #133).
//!
//! Serde keeps two independent renaming axes on an enum, and the emitter
//! used to collapse them: it applied the enum's `rename_all` — which serde
//! uses for variant names only — to the fields inside struct variants, while
//! ignoring `rename_all_fields`, the attribute that actually asks for that.
//! The result looked plausible and was wrong, with no diagnostic.
//!
//! These tests take serde as the oracle rather than asserting a static
//! string: [`wire_fixture!`] declares each enum once and captures its source
//! via `stringify!`, so the types serde compiles and the types the emitter
//! scans cannot drift apart. Each test then compares the emitted TypeScript
//! against the keys `serde_json` actually produces.

use std::collections::BTreeSet;
use std::fs;

use ontogen_ts::{EmitConfig, TypePath, emit, scan_src_dir};
use serde::Serialize;

/// Declare items normally AND capture their source text, so the emitter is
/// guaranteed to be looking at exactly the types serde compiled.
macro_rules! wire_fixture {
    ($($item:item)*) => {
        $($item)*
        /// Source text of the items above, as the emitter sees them.
        const FIXTURE_SRC: &str = stringify!($($item)*);
    };
}

wire_fixture! {
    /// Enum `rename_all` renames variants only — `prompt_template` must
    /// survive verbatim.
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    pub enum PlainEvent {
        ToolCall { prompt_template: String },
    }

    /// `rename_all_fields` is the attribute that renames struct-variant
    /// fields.
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
    pub enum FieldRenamedEvent {
        ToolCall { prompt_template: String },
    }

    /// A variant's own `rename_all` governs that variant's fields and beats
    /// the container's `rename_all_fields`; the sibling keeps following the
    /// container.
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
    pub enum MixedEvent {
        #[serde(rename_all = "SCREAMING_SNAKE_CASE")]
        ToolCall { prompt_template: String },
        ToolResult { exit_code: u32 },
    }
}

/// Emit TypeScript for `root` from [`FIXTURE_SRC`].
fn emit_fixture(root: &str) -> String {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(dir.path().join("lib.rs"), FIXTURE_SRC).expect("write lib.rs");
    let pool = scan_src_dir(dir.path()).expect("scan");
    let path = TypePath::new(vec![root.to_string()]).expect("non-empty");
    emit(&[path], &pool, &EmitConfig::default()).unwrap_or_else(|errs| panic!("emit failed: {errs:?}"))
}

/// Serialize an externally-tagged struct-variant value and return
/// `(variant_key, field_keys)` exactly as they appear on the wire.
fn wire_shape<T: Serialize>(value: &T) -> (String, BTreeSet<String>) {
    let json = serde_json::to_value(value).expect("serialize");
    let outer = json.as_object().unwrap_or_else(|| panic!("expected a JSON object, got {json}"));
    assert_eq!(outer.len(), 1, "externally-tagged variant should be a single-key object, got {json}");
    let (variant_key, payload) = outer.iter().next().expect("checked non-empty");
    let fields = payload.as_object().unwrap_or_else(|| panic!("expected a struct-variant payload, got {payload}"));
    (variant_key.clone(), fields.keys().cloned().collect())
}

/// Assert the emitted TS declares `{ variant_key: { field: ... } }` for
/// exactly the wire's keys.
fn assert_ts_matches_wire(ts: &str, variant_key: &str, fields: &BTreeSet<String>) {
    assert!(ts.contains(&format!("{{ {variant_key}: {{")), "variant key `{variant_key}` missing; ts was:\n{ts}");
    for field in fields {
        assert!(ts.contains(&format!("{field}: ")), "field `{field}` missing; ts was:\n{ts}");
    }
}

#[test]
fn enum_rename_all_renames_the_variant_but_not_its_fields() {
    let (variant_key, fields) = wire_shape(&PlainEvent::ToolCall { prompt_template: "p".to_string() });
    // Ground truth from serde.
    assert_eq!(variant_key, "toolCall");
    assert_eq!(fields, ["prompt_template".to_string()].into_iter().collect::<BTreeSet<_>>());

    let ts = emit_fixture("PlainEvent");
    assert_ts_matches_wire(&ts, &variant_key, &fields);
    // The regression itself: the emitter used to camelCase this field.
    assert!(!ts.contains("promptTemplate"), "enum rename_all must not reach variant fields; ts was:\n{ts}");
}

#[test]
fn rename_all_fields_renames_struct_variant_fields() {
    let (variant_key, fields) = wire_shape(&FieldRenamedEvent::ToolCall { prompt_template: "p".to_string() });
    assert_eq!(variant_key, "toolCall");
    assert_eq!(fields, ["promptTemplate".to_string()].into_iter().collect::<BTreeSet<_>>());

    let ts = emit_fixture("FieldRenamedEvent");
    assert_ts_matches_wire(&ts, &variant_key, &fields);
}

#[test]
fn variant_rename_all_overrides_container_rename_all_fields() {
    let (call_key, call_fields) = wire_shape(&MixedEvent::ToolCall { prompt_template: "p".to_string() });
    assert_eq!(call_key, "toolCall");
    assert_eq!(call_fields, ["PROMPT_TEMPLATE".to_string()].into_iter().collect::<BTreeSet<_>>());

    let (result_key, result_fields) = wire_shape(&MixedEvent::ToolResult { exit_code: 0 });
    assert_eq!(result_key, "toolResult");
    // The sibling variant still follows the container's rename_all_fields.
    assert_eq!(result_fields, ["exitCode".to_string()].into_iter().collect::<BTreeSet<_>>());

    let ts = emit_fixture("MixedEvent");
    assert_ts_matches_wire(&ts, &call_key, &call_fields);
    assert_ts_matches_wire(&ts, &result_key, &result_fields);
}
