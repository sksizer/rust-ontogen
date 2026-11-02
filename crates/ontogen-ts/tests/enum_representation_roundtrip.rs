//! Round-trip proof for serde's enum representations — `tag`, `content`,
//! `untagged`, and variant-level `untagged` (OF-015 phase 2) — and for
//! struct-level `tag`.
//!
//! As in `flatten_roundtrip.rs`, [`wire_fixture!`] declares the types once
//! and hands the same source text to the emitter, so serde and the emitter
//! cannot be looking at different types. Each test pins both sides: the
//! JSON serde actually produces for every variant, and the TypeScript the
//! emitter renders for it. A future serde change in either representation
//! fails the wire half loudly instead of silently shipping wrong `.ts`.
//!
//! When `ONTOGEN_TS_DUMP` names a file, the tests also write a TypeScript
//! module there assigning every serialized value to its emitted type, so
//! `tsc --strict` can check the pairing end to end.

use std::fs;

use ontogen_ts::{EmitConfig, LOCAL_CRATE_ROOT, TypePath, emit, scan_src_dir};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

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
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    pub struct Point {
        pub x: i32,
        pub y: i32,
    }

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    #[serde(tag = "type", rename_all = "snake_case")]
    pub enum Internal {
        Ping,
        At(Point),
        Move { dx: i32, dy: i32 },
    }

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    #[serde(tag = "t", content = "c")]
    pub enum Adjacent {
        Ping,
        Count(u32),
        Move { dx: i32 },
        Nothing(),
    }

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    #[serde(untagged)]
    pub enum Untagged {
        Nothing,
        Num(f64),
        At(Point),
    }

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    #[serde(tag = "kind")]
    pub enum Mixed {
        Known { id: u32 },
        #[serde(untagged)]
        Other(String),
    }

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    pub enum External {
        Empty(),
        One(u32),
    }

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    #[serde(tag = "type")]
    pub struct Tagged {
        pub at: u64,
    }
}

/// Emit TypeScript for `root` (and its closure) from [`FIXTURE_SRC`].
fn emit_fixture(root: &str) -> String {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(dir.path().join("lib.rs"), FIXTURE_SRC).expect("write lib.rs");
    let pool = scan_src_dir(dir.path()).expect("scan");
    let path = TypePath::new(vec![LOCAL_CRATE_ROOT.to_string(), root.to_string()]).expect("non-empty");
    emit(&[path], &pool, &EmitConfig::default()).unwrap_or_else(|errs| panic!("emit failed: {errs:?}"))
}

/// The `export type {root} = ...;` line out of an emitted module.
fn type_line(ts: &str, root: &str) -> String {
    let prefix = format!("export type {root} = ");
    let start = ts.find(&prefix).unwrap_or_else(|| panic!("no `{prefix}` in:\n{ts}"));
    let end = ts[start..].find(";\n").map_or(ts.len(), |i| start + i + 1);
    ts[start..end].to_string()
}

/// Assert each value serializes to the paired JSON and deserializes back,
/// then, when dumping, record `const _: Root = <json>;` for `tsc`.
fn assert_wire<T>(root: &str, cases: Vec<(T, Value)>)
where
    T: Serialize + for<'de> Deserialize<'de> + PartialEq + std::fmt::Debug,
{
    let mut checks = String::new();
    for (i, (value, expected)) in cases.into_iter().enumerate() {
        let wire = serde_json::to_value(&value).expect("serialize");
        assert_eq!(wire, expected, "{root}: {value:?} serialized differently");
        // Through text, not `from_value`: serde_json's `Value` deserializer
        // hands an empty array to a zero-field tuple variant as `null`.
        let back: T = serde_json::from_str(&wire.to_string()).expect("deserialize");
        assert_eq!(back, value, "{root}: {wire} did not round-trip");
        checks.push_str(&format!("const _{root}{i}: {root} = {wire};\n"));
    }
    if let Ok(path) = std::env::var("ONTOGEN_TS_DUMP") {
        let ts = emit_fixture(root);
        fs::write(format!("{path}.{root}.ts"), format!("{ts}\n{checks}")).expect("dump");
    }
}

#[test]
fn internally_tagged() {
    assert_wire(
        "Internal",
        vec![
            (Internal::Ping, json!({ "type": "ping" })),
            (Internal::At(Point { x: 1, y: 2 }), json!({ "type": "at", "x": 1, "y": 2 })),
            (Internal::Move { dx: 3, dy: 4 }, json!({ "type": "move", "dx": 3, "dy": 4 })),
        ],
    );
    assert_eq!(
        type_line(&emit_fixture("Internal"), "Internal"),
        "export type Internal = { type: 'ping' } | { type: 'at' } & Point | { type: 'move'; dx: number; dy: number };"
    );
}

#[test]
fn adjacently_tagged() {
    assert_wire(
        "Adjacent",
        vec![
            (Adjacent::Ping, json!({ "t": "Ping" })),
            (Adjacent::Count(5), json!({ "t": "Count", "c": 5 })),
            (Adjacent::Move { dx: 1 }, json!({ "t": "Move", "c": { "dx": 1 } })),
            (Adjacent::Nothing(), json!({ "t": "Nothing", "c": [] })),
        ],
    );
    assert_eq!(
        type_line(&emit_fixture("Adjacent"), "Adjacent"),
        "export type Adjacent = { t: 'Ping' } | { t: 'Count'; c: number } | { t: 'Move'; c: { dx: number } } | { t: \
         'Nothing'; c: [] };"
    );
}

#[test]
fn untagged() {
    assert_wire(
        "Untagged",
        vec![
            (Untagged::Nothing, json!(null)),
            (Untagged::Num(1.5), json!(1.5)),
            (Untagged::At(Point { x: 1, y: 2 }), json!({ "x": 1, "y": 2 })),
        ],
    );
    assert_eq!(type_line(&emit_fixture("Untagged"), "Untagged"), "export type Untagged = null | number | Point;");
}

#[test]
fn variant_level_untagged() {
    assert_wire(
        "Mixed",
        vec![
            (Mixed::Known { id: 7 }, json!({ "kind": "Known", "id": 7 })),
            (Mixed::Other("free".to_string()), json!("free")),
        ],
    );
    assert_eq!(
        type_line(&emit_fixture("Mixed"), "Mixed"),
        "export type Mixed = { kind: 'Known'; id: number } | string;"
    );
}

#[test]
fn external_empty_tuple_variant_is_an_empty_array() {
    assert_wire("External", vec![(External::Empty(), json!({ "Empty": [] })), (External::One(1), json!({ "One": 1 }))]);
    assert_eq!(
        type_line(&emit_fixture("External"), "External"),
        "export type External = { Empty: [] } | { One: number };"
    );
}

#[test]
fn struct_tag() {
    assert_wire("Tagged", vec![(Tagged { at: 9 }, json!({ "type": "Tagged", "at": 9 }))]);
    assert!(emit_fixture("Tagged").contains("export type Tagged = {\n  type: 'Tagged';\n  at: number;\n};"));
    // Serde ignores a struct's tag on the way in, which is why a
    // deserialize-only struct renders it optional.
    let lax: Tagged = serde_json::from_value(json!({ "at": 9 })).expect("tag is not required on deserialize");
    assert_eq!(lax, Tagged { at: 9 });
}
