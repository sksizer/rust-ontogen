//! Round-trip proof for `#[serde(flatten)]` emission (issue #132).
//!
//! The failure this guards against is the one that motivated the fix: a
//! flattened field's keys land in the *parent* object on the wire, but the
//! emitter used to render the field as an ordinary nested property. A
//! consumer typed against that TS would construct `{ meta: {...}, program }`
//! and serde would reject it at the boundary.
//!
//! Rather than assert against a static expected string, each test derives
//! both sides from the same source text:
//!
//! 1. [`wire_fixture!`] declares the Rust types once and captures their
//!    source via `stringify!`, so the types serde compiles and the types
//!    the emitter scans cannot drift apart.
//! 2. `serde_json::to_value` gives the ground-truth wire shape.
//! 3. `emit` gives the TypeScript.
//! 4. The test asserts the TS property set matches the wire's key set.
//!
//! Keeping serde as the oracle means a future serde change in flatten
//! semantics fails here loudly instead of silently shipping wrong `.ts`.

use std::collections::BTreeSet;
use std::fs;

use ontogen_ts::{EmitConfig, TypePath, emit, scan_src_dir};
use serde::{Deserialize, Serialize};

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
    #[derive(Debug, Serialize, Deserialize)]
    pub struct StepMeta {
        pub id: String,
        pub label: String,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct Step {
        #[serde(flatten)]
        pub meta: StepMeta,
        pub program: String,
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

/// Top-level JSON keys serde actually puts on the wire for `value`.
fn wire_keys<T: Serialize>(value: &T) -> BTreeSet<String> {
    let json = serde_json::to_value(value).expect("serialize");
    json.as_object().unwrap_or_else(|| panic!("expected a JSON object, got {json}")).keys().cloned().collect()
}

/// Property names declared across every `export type` block in `ts`.
///
/// Deliberately crude — it collects `foo:` / `foo?:` at any nesting depth in
/// the emitted source. That is enough here because the fixture's emitted
/// types are flat objects, and being over-inclusive can only make a missing
/// key harder to miss, never easier.
fn ts_property_names(ts: &str) -> BTreeSet<String> {
    ts.lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            let (name, _) = trimmed.split_once(':')?;
            let name = name.trim().trim_end_matches('?');
            (!name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')).then(|| name.to_string())
        })
        .collect()
}

#[test]
fn flattened_field_keys_match_the_wire() {
    let value =
        Step { meta: StepMeta { id: "s1".to_string(), label: "build".to_string() }, program: "bun".to_string() };
    let keys = wire_keys(&value);
    // Ground truth: serde splices StepMeta's keys into the parent object.
    assert_eq!(keys, ["id", "label", "program"].iter().map(|s| (*s).to_string()).collect::<BTreeSet<_>>());

    let ts = emit_fixture("Step");
    // The emitted closure covers Step and its flattened StepMeta, and every
    // wire key appears as a declared property across the two.
    assert_eq!(ts_property_names(&ts), keys, "ts was:\n{ts}");
}

#[test]
fn flattened_field_emits_an_intersection_not_a_property() {
    let ts = emit_fixture("Step");
    assert!(ts.contains("export type Step = StepMeta & {"), "ts was:\n{ts}");
    // The regression itself: `meta` is the Rust field name and never reaches
    // the wire, so it must not appear as a TS property.
    assert!(!ts.contains("meta:"), "`meta` must not be emitted as a property; ts was:\n{ts}");
}

#[test]
fn the_shape_the_old_emission_described_is_rejected_by_serde() {
    // Pins down *why* the old output was wrong rather than merely different:
    // a consumer typed against `{ meta: StepMeta; program: string }` builds
    // this payload, and serde refuses it.
    //
    // Note the error is `missing field \`id\``, not `unknown field \`meta\``:
    // a flattened field deserializes from the leftover map, so the stray
    // `meta` key is swallowed by it and the keys it should have carried are
    // simply absent. Either way the payload does not round-trip.
    let nested = r#"{"meta":{"id":"s1","label":"build"},"program":"bun"}"#;
    let err = serde_json::from_str::<Step>(nested).expect_err("nested shape must not deserialize");
    assert!(err.to_string().contains("missing field"), "error was: {err}");

    // The shape the new output describes round-trips.
    let flat = r#"{"id":"s1","label":"build","program":"bun"}"#;
    let step: Step = serde_json::from_str(flat).expect("flat shape must deserialize");
    assert_eq!(step.meta.id, "s1");
    assert_eq!(step.program, "bun");
}
