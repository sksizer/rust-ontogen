//! End-to-end tests for the full `emit()` composition (PR 4).
//!
//! Each test writes a synthetic crate's `src/` tree into a tempdir, calls
//! `scan_src_dir` to build the type pool, then `emit()` against chosen
//! root paths. The TS output is checked for the presence of specific
//! exports / wire names, since asserting against a literal string would
//! be over-tight (small ordering tweaks would break the test even though
//! the output is correct).

use std::collections::BTreeMap;
use std::fs;

use ontogen_ts::{EmitConfig, EmitError, TypePath, emit, scan_src_dir};

/// Convenience: build a tempdir from `files` (each `(rel_path, content)`).
fn make_tempdir(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    for (rel, content) in files {
        let abs = dir.path().join(rel);
        if let Some(parent) = abs.parent() {
            fs::create_dir_all(parent).expect("create parent");
        }
        fs::write(&abs, content).expect("write file");
    }
    dir
}

fn tp(segments: &[&str]) -> TypePath {
    TypePath::new(segments.iter().map(|s| (*s).to_string()).collect()).expect("non-empty")
}

#[test]
fn emit_single_struct_with_primitives() {
    let dir = make_tempdir(&[("lib.rs", "pub struct Workout { pub id: u64, pub minutes: u32 }")]);
    let pool = scan_src_dir(dir.path()).unwrap();
    let config = EmitConfig::default();
    let ts = emit(&[tp(&["Workout"])], &pool, &config).unwrap();
    assert!(ts.contains("export type Workout"), "ts was:\n{ts}");
    assert!(ts.contains("id: number"), "ts was:\n{ts}");
    assert!(ts.contains("minutes: number"), "ts was:\n{ts}");
}

#[test]
fn emit_transitive_closure_orders_deps_first() {
    let dir = make_tempdir(&[(
        "lib.rs",
        r#"
        pub struct Session { pub workout: Workout }
        pub struct Workout { pub minutes: u32 }
        "#,
    )]);
    let pool = scan_src_dir(dir.path()).unwrap();
    let config = EmitConfig::default();
    let ts = emit(&[tp(&["Session"])], &pool, &config).unwrap();

    let session_pos = ts.find("export type Session").expect("Session in output");
    let workout_pos = ts.find("export type Workout").expect("Workout in output");
    // Workout (dep) must appear before Session (dependent).
    assert!(workout_pos < session_pos, "ts was:\n{ts}");
}

#[test]
fn emit_skips_unreachable_pool_types() {
    let dir = make_tempdir(&[(
        "lib.rs",
        r#"
        pub struct Reached { pub x: u32 }
        pub struct Unreached { pub x: u32 }
        "#,
    )]);
    let pool = scan_src_dir(dir.path()).unwrap();
    let ts = emit(&[tp(&["Reached"])], &pool, &EmitConfig::default()).unwrap();
    assert!(ts.contains("export type Reached"), "ts was:\n{ts}");
    assert!(!ts.contains("export type Unreached"), "ts was:\n{ts}");
}

#[test]
fn emit_ts_opaque_short_circuits() {
    let dir = make_tempdir(&[(
        "lib.rs",
        r#"
        #[ts_opaque(target = "Date")]
        pub struct EpochSeconds { pub seconds: i64 }
        "#,
    )]);
    let pool = scan_src_dir(dir.path()).unwrap();
    let ts = emit(&[tp(&["EpochSeconds"])], &pool, &EmitConfig::default()).unwrap();
    // Opaque: type emitted as a terminal alias, not as `{ seconds: ... }`.
    assert!(ts.contains("export type EpochSeconds = Date;"), "ts was:\n{ts}");
    assert!(!ts.contains("seconds: number"), "ts was:\n{ts}");
}

#[test]
fn emit_ts_name_overrides_emitted_name() {
    let dir = make_tempdir(&[(
        "lib.rs",
        r#"
        #[ts_name = "FooStats"]
        pub struct FooStatistics { pub count: u64 }
        "#,
    )]);
    let pool = scan_src_dir(dir.path()).unwrap();
    let ts = emit(&[tp(&["FooStatistics"])], &pool, &EmitConfig::default()).unwrap();
    assert!(ts.contains("export type FooStats"), "ts was:\n{ts}");
    assert!(!ts.contains("export type FooStatistics"), "ts was:\n{ts}");
}

#[test]
fn emit_detects_name_collision() {
    let dir = make_tempdir(&[(
        "lib.rs",
        r#"
        pub mod a {
            pub struct Workout { pub id: u64 }
        }
        pub mod b {
            pub struct Workout { pub minutes: u32 }
        }
        pub struct Root { pub a: a::Workout, pub b: b::Workout }
        "#,
    )]);
    let pool = scan_src_dir(dir.path()).unwrap();
    let err = emit(&[tp(&["Root"])], &pool, &EmitConfig::default()).unwrap_err();
    let collision =
        err.iter().find(|e| matches!(e, EmitError::NameCollision { .. })).expect("expected a NameCollision error");
    match collision {
        EmitError::NameCollision { name, paths } => {
            assert_eq!(name, "Workout");
            assert_eq!(paths.len(), 2, "paths were: {paths:?}");
        }
        _ => unreachable!(),
    }
}

#[test]
fn emit_external_type_resolves_to_default_rendering() {
    // chrono::DateTime is in the shipped default external-types table.
    // emit_type's fall-through should render it as `string`.
    let dir = make_tempdir(&[(
        "lib.rs",
        r#"
        pub struct Event { pub at: chrono::DateTime<Utc> }
        "#,
    )]);
    let pool = scan_src_dir(dir.path()).unwrap();
    let ts = emit(&[tp(&["Event"])], &pool, &EmitConfig::default()).unwrap();
    assert!(ts.contains("at: string"), "ts was:\n{ts}");
}

#[test]
fn emit_user_override_wins_on_external_types() {
    // User passes a per-call override; it should beat the shipped default.
    let dir = make_tempdir(&[(
        "lib.rs",
        r#"
        pub struct Event { pub when: chrono::DateTime<Utc> }
        "#,
    )]);
    let pool = scan_src_dir(dir.path()).unwrap();
    let mut overrides = BTreeMap::new();
    overrides.insert("chrono::DateTime".to_string(), "Moment".to_string());
    let config = EmitConfig { external_types: overrides, ..Default::default() };
    let ts = emit(&[tp(&["Event"])], &pool, &config).unwrap();
    assert!(ts.contains("when: Moment"), "ts was:\n{ts}");
    assert!(!ts.contains("when: string"), "ts was:\n{ts}");
}

#[test]
fn emit_aggregates_multiple_errors_into_one_vec() {
    // Two malformed types should produce two errors, not first-fail.
    let dir = make_tempdir(&[(
        "lib.rs",
        r#"
        pub struct A { pub locked: std::sync::Mutex<u32> }
        pub struct B { pub also_locked: std::sync::RwLock<u32> }
        pub struct Root { pub a: A, pub b: B }
        "#,
    )]);
    let pool = scan_src_dir(dir.path()).unwrap();
    let err = emit(&[tp(&["Root"])], &pool, &EmitConfig::default()).unwrap_err();
    // Both A and B should surface errors.
    let unsupported_count = err.iter().filter(|e| matches!(e, EmitError::UnsupportedShape { .. })).count();
    assert!(unsupported_count >= 2, "expected at least 2 UnsupportedShape errors, got: {err:?}");
}

#[test]
fn emit_unknown_root_yields_unresolved_reference() {
    let dir = make_tempdir(&[("lib.rs", "pub struct A { pub x: u32 }")]);
    let pool = scan_src_dir(dir.path()).unwrap();
    let err = emit(&[tp(&["DoesNotExist"])], &pool, &EmitConfig::default()).unwrap_err();
    assert!(
        err.iter().any(|e| matches!(e, EmitError::UnresolvedReference { .. })),
        "expected UnresolvedReference, got: {err:?}"
    );
}

#[test]
fn emit_unit_struct_surfaces_unsupported_shape() {
    let dir = make_tempdir(&[("lib.rs", "pub struct Marker;")]);
    let pool = scan_src_dir(dir.path()).unwrap();
    let err = emit(&[tp(&["Marker"])], &pool, &EmitConfig::default()).unwrap_err();
    assert!(err.iter().any(|e| matches!(e, EmitError::UnsupportedShape { .. })));
}

#[test]
fn emit_rename_all_propagates_through_pipeline() {
    let dir = make_tempdir(&[(
        "lib.rs",
        r#"
        #[serde(rename_all = "camelCase")]
        pub struct UserProfile {
            pub display_name: String,
            pub age_years: u32,
        }
        "#,
    )]);
    let pool = scan_src_dir(dir.path()).unwrap();
    let ts = emit(&[tp(&["UserProfile"])], &pool, &EmitConfig::default()).unwrap();
    assert!(ts.contains("displayName: string"), "ts was:\n{ts}");
    assert!(ts.contains("ageYears: number"), "ts was:\n{ts}");
    assert!(!ts.contains("display_name"), "ts was:\n{ts}");
}

#[test]
fn emit_module_qualified_type_resolves_via_crate_prefix() {
    let dir = make_tempdir(&[
        ("lib.rs", "pub mod models;\npub struct Root { pub w: crate::models::Workout }"),
        ("models.rs", "pub struct Workout { pub id: u64 }"),
    ]);
    let pool = scan_src_dir(dir.path()).unwrap();
    let ts = emit(&[tp(&["Root"])], &pool, &EmitConfig::default()).unwrap();
    // Both types emit, Workout before Root.
    assert!(ts.contains("export type Workout"), "ts was:\n{ts}");
    assert!(ts.contains("export type Root"), "ts was:\n{ts}");
}

#[test]
fn emit_empty_pool_with_no_roots_returns_empty_string() {
    let dir = make_tempdir(&[("lib.rs", "")]);
    let pool = scan_src_dir(dir.path()).unwrap();
    let ts = emit(&[], &pool, &EmitConfig::default()).unwrap();
    assert_eq!(ts, "");
}
