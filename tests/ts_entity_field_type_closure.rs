//! Integration test for the entity-field-type → long-tail-root closure.
//!
//! AC-1 of the
//! `2026-05-20-ontogen-ts-entity-field-type-closure` task: an `EntityDef`
//! with a field whose type ident is defined only in the type pool (not in
//! the schema-known surface) — the emitted bindings must include both the
//! entity rendering AND the long-tail type body.
//!
//! This test simulates Pumice's TimerSession → IntervalKind case without
//! reaching for Pumice's source tree. We construct:
//!
//! - an `EntityDef` (`TimerSession`) with a field
//!   `interval_kind: Option<IntervalKind>`, classified as
//!   `FieldType::OptionEnum("IntervalKind")` — what the macro classifier
//!   produces in the real schema parse;
//! - a synthetic `src/` tempdir containing a `pub enum IntervalKind` —
//!   what a sibling crate would provide via `pool_extra_roots`;
//!
//! Then assert:
//!
//! - `ts_bindings::emit` (via the public `gen_clients`-shaped surface
//!   exercised here by calling `ontogen::clients::generators::ts_bindings`)
//!   renders the entity body referencing the bare ident `IntervalKind`.
//! - `ontogen_ts::emit`, given `IntervalKind` as a root and the
//!   synthetic pool, emits the enum body.
//!
//! Together those two halves are what gets written to `bindings.ts` by
//! `clients::generate_clients`: schema-known surface first, then the
//! long-tail closure appended. If the long-tail emitter never sees
//! `IntervalKind` as a root, the entity rendering type-checks against
//! nothing — exactly the Pumice gap this task closes.

use std::fs;

use ontogen::model::{EntityDef, FieldDef, FieldRole, FieldType};
use ontogen_ts::{EmitConfig, TypePath, emit as ontogen_ts_emit, scan_src_dir};

fn timer_session_entity() -> EntityDef {
    EntityDef {
        name: "TimerSession".to_string(),
        directory: "timer_sessions".to_string(),
        table: "timer_sessions".to_string(),
        type_name: "timer_session".to_string(),
        prefix: "timer".to_string(),
        fields: vec![
            FieldDef::new("id", FieldType::String, FieldRole::Id),
            FieldDef::new("interval_kind", FieldType::OptionEnum("IntervalKind".into()), FieldRole::EnumField),
        ],
    }
}

#[test]
fn entity_field_type_appears_in_emitted_long_tail() {
    // Synthetic sibling-crate src/ containing the long-tail enum the
    // entity references. In a real consumer this comes from a sibling
    // crate wired via `pool_extra_roots`.
    let pool_dir = tempfile::tempdir().expect("tempdir");
    let lib_rs = pool_dir.path().join("lib.rs");
    fs::write(
        &lib_rs,
        r#"
        pub enum IntervalKind {
            Focus,
            ShortBreak,
            LongBreak,
        }
        "#,
    )
    .expect("write fixture");

    let pool = scan_src_dir(pool_dir.path()).expect("scan pool");

    // Independently of `ts_bindings::long_tail` (which is crate-private),
    // the contract this test pins is: when a root corresponding to the
    // entity's field-type ident is fed to `ontogen_ts::emit` alongside a
    // pool containing the definition, the emitter produces a TS body for
    // it. The unit tests in `ts_bindings::tests` separately verify that
    // `long_tail` produces exactly such roots from `EntityDef.fields`.
    let root = TypePath::new(vec!["IntervalKind".to_string()]).expect("non-empty TypePath");
    let long_tail_ts = ontogen_ts_emit(&[root], &pool, &EmitConfig::default()).expect("ontogen-ts emit");

    // Long-tail half: ontogen-ts emits the enum body.
    assert!(long_tail_ts.contains("IntervalKind"), "long-tail TS missing IntervalKind:\n{long_tail_ts}");

    // Schema-known half: the entity rendering references the bare ident
    // exactly as the schema-known emitter would write it.
    let entities = vec![timer_session_entity()];
    let schema_known_ts = ontogen::clients::emit_schema_known_ts_for_tests(&entities);

    assert!(
        schema_known_ts.contains("export type TimerSession"),
        "schema-known TS missing TimerSession:\n{schema_known_ts}"
    );
    assert!(
        schema_known_ts.contains("interval_kind: IntervalKind | null"),
        "schema-known TS does not render interval_kind as bare IntervalKind ident:\n{schema_known_ts}"
    );

    // Combined artifact mirrors what `clients::generate_clients` writes
    // to bindings.ts: schema-known surface first, then ontogen-ts output
    // appended.
    let combined = format!("{schema_known_ts}\n// Long-tail types\n{long_tail_ts}");
    assert!(combined.contains("export type TimerSession"));
    assert!(combined.contains("IntervalKind"));
}
