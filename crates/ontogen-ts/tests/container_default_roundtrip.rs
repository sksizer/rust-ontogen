//! Round-trip proof that container-level `#[serde(default)]` reaches the
//! emitted TypeScript.
//!
//! `#[serde(default)]` on a struct fills every absent field from the
//! struct's `Default`, so the wire accepts `{}`. The emitter dropped the
//! attribute and rendered every field as required, which forced a TS caller
//! to spell out a value for each one — commonly `null` — to satisfy `tsc`,
//! even though serde wanted nothing at all.
//!
//! Serde is the oracle here, as in `flatten_roundtrip.rs`: [`wire_fixture!`]
//! declares the type once and captures its source via `stringify!`, so the
//! type serde compiles and the type the emitter scans cannot drift apart.

use std::fs;

use ontogen_ts::{EmitConfig, LOCAL_CRATE_ROOT, TypePath, emit, scan_src_dir};
use serde::Deserialize;

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
    #[derive(Debug, Default, Deserialize)]
    #[serde(default)]
    pub struct Settings {
        pub name: String,
        pub retries: u32,
        pub notes: Option<String>,
    }

    /// Control: the same shape without the container attribute.
    #[derive(Debug, Deserialize)]
    pub struct StrictSettings {
        pub name: String,
    }
}

/// Emit TypeScript for `root` from [`FIXTURE_SRC`].
fn emit_fixture(root: &str) -> String {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(dir.path().join("lib.rs"), FIXTURE_SRC).expect("write lib.rs");
    let pool = scan_src_dir(dir.path()).expect("scan");
    // Pool keys name their root; a crate-root type keys as
    // `["crate", "Name"]`.
    let path = TypePath::new(vec![LOCAL_CRATE_ROOT.to_string(), root.to_string()]).expect("non-empty");
    emit(&[path], &pool, &EmitConfig::default()).unwrap_or_else(|errs| panic!("emit failed: {errs:?}"))
}

/// The `export type Settings = { … }` block, without the surrounding
/// declaration — one `name: type;` entry per line.
fn property_lines(ts: &str, decl: &str) -> Vec<String> {
    let start = ts.find(decl).unwrap_or_else(|| panic!("`{decl}` missing from:\n{ts}"));
    ts[start..].lines().skip(1).take_while(|line| !line.starts_with("};")).map(|line| line.trim().to_string()).collect()
}

#[test]
fn every_field_of_a_container_default_struct_is_optional() {
    // Ground truth: serde accepts an empty object for the whole struct.
    let from_empty: Settings = serde_json::from_str("{}").expect("container default must accept `{}`");
    assert_eq!(from_empty.name, "");
    assert_eq!(from_empty.retries, 0);
    assert_eq!(from_empty.notes, None);

    // So every emitted property must be optional.
    let ts = emit_fixture("Settings");
    let props = property_lines(&ts, "export type Settings = {");
    assert!(!props.is_empty(), "no properties parsed out of:\n{ts}");
    for prop in &props {
        assert!(prop.contains("?:"), "`{prop}` must be optional; ts was:\n{ts}");
    }
}

#[test]
fn a_partial_object_round_trips_field_by_field() {
    // Each field is independently absent-able, which is what `?` per field
    // (rather than a single all-or-nothing marker) claims.
    let partial: Settings = serde_json::from_str(r#"{"retries":3}"#).expect("partial object must deserialize");
    assert_eq!(partial.retries, 3);
    assert_eq!(partial.name, "");
}

#[test]
fn a_struct_without_the_attribute_keeps_required_fields() {
    // Control: no container default, so serde rejects `{}` and the emitter
    // must keep the field required. Guards against over-applying the fix.
    serde_json::from_str::<StrictSettings>("{}").expect_err("missing field must be rejected");

    let ts = emit_fixture("StrictSettings");
    let props = property_lines(&ts, "export type StrictSettings = {");
    assert_eq!(props, vec!["name: string;".to_string()], "ts was:\n{ts}");
}
