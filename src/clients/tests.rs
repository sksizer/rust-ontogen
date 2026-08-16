//! Tests for the clients module: TypeScript bindings, HTTP/IPC transport,
//! HTTP-only client, and admin-registry generators.
//!
//! Mirrors the structure of [`crate::servers::tests`] for the server-side
//! generators. Client-side test cases were relocated here as part of the
//! servers→clients split.

use std::fs;

use ontogen_core::utils::TsFormatter;

use super::{
    LONG_TAIL_MARKER, append_long_tail_to_bindings, extra_root_crate_name, package_name_from_manifest, strip_long_tail,
};

/// The long-tail emitter's raw style — single-quoted literals — standing in
/// for what `ontogen_ts::emit` produces.
const LONG_TAIL_TS: &str = "export type RuleCategory = 'Heading' | 'List' | 'Other';";

/// A schema-known bindings file as the earlier `write_and_format_ts` pass
/// would have left it.
const SCHEMA_KNOWN: &str = "export type Note = { id: string };\n";

/// Stand-in for a real formatter's quote normalization (biome and prettier
/// both canonicalize to double quotes by default).
fn double_quote_formatter() -> TsFormatter {
    TsFormatter::custom(|src: &str, _path: &std::path::Path| Ok(src.replace('\'', "\"")))
}

/// A formatter that also collapses blank lines — the adversarial case for
/// marker detection, since it rewrites the whitespace the marker sits in.
fn collapsing_formatter() -> TsFormatter {
    TsFormatter::custom(|src: &str, _path: &std::path::Path| {
        let mut out = src.replace('\'', "\"");
        while out.contains("\n\n") {
            out = out.replace("\n\n", "\n");
        }
        Ok(out)
    })
}

/// Write `SCHEMA_KNOWN` into a fresh tempdir and return `(dir, path)`.
fn seeded_bindings() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("types.ts");
    fs::write(&path, SCHEMA_KNOWN).expect("seed bindings");
    (dir, path)
}

#[test]
fn long_tail_section_goes_through_the_formatter() {
    // Issue #123: the base was written through `write_and_format_ts`, then
    // the long-tail chunk was appended with a plain `write_if_changed` —
    // after the formatter pass — so the appended section kept ontogen-ts's
    // raw emit style inside an otherwise formatter-canonical file.
    let (_dir, path) = seeded_bindings();
    append_long_tail_to_bindings(&path, LONG_TAIL_TS, &double_quote_formatter()).expect("append");

    let written = fs::read_to_string(&path).expect("read back");
    assert!(written.contains(LONG_TAIL_MARKER), "marker missing; file was:\n{written}");
    assert!(written.contains(r#""Heading""#), "long-tail section was not formatted; file was:\n{written}");
    assert!(!written.contains('\''), "single quotes survived the formatter; file was:\n{written}");
}

#[test]
fn unformatted_config_still_writes_the_section_verbatim() {
    // `TsFormatter::None` must stay a pass-through — routing through the
    // formatter should not change output for consumers who opted out.
    let (_dir, path) = seeded_bindings();
    append_long_tail_to_bindings(&path, LONG_TAIL_TS, &TsFormatter::None).expect("append");

    let written = fs::read_to_string(&path).expect("read back");
    assert!(written.starts_with(SCHEMA_KNOWN), "base was altered; file was:\n{written}");
    assert!(written.contains(LONG_TAIL_TS), "raw long-tail missing; file was:\n{written}");
}

#[test]
fn rerunning_replaces_the_section_rather_than_doubling_it() {
    for formatter in [TsFormatter::None, double_quote_formatter(), collapsing_formatter()] {
        let (_dir, path) = seeded_bindings();
        append_long_tail_to_bindings(&path, LONG_TAIL_TS, &formatter).expect("first append");
        let first = fs::read_to_string(&path).expect("read back");

        append_long_tail_to_bindings(&path, LONG_TAIL_TS, &formatter).expect("second append");
        let second = fs::read_to_string(&path).expect("read back");

        assert_eq!(first, second, "rebuild was not a fixpoint for {formatter:?}");
        assert_eq!(second.matches(LONG_TAIL_MARKER).count(), 1, "section doubled for {formatter:?}:\n{second}");
    }
}

#[test]
fn rerunning_after_a_blank_line_collapsing_formatter_still_finds_the_marker() {
    // Regression guard specific to formatting after assembly: the marker
    // used to be matched together with its surrounding newlines, so a
    // formatter that rewrote that whitespace would make the strip miss and
    // every rebuild would append another copy of the section.
    let (_dir, path) = seeded_bindings();
    let formatter = collapsing_formatter();
    append_long_tail_to_bindings(&path, LONG_TAIL_TS, &formatter).expect("first append");

    let after_first = fs::read_to_string(&path).expect("read back");
    assert!(!after_first.contains("\n\n"), "test formatter should have collapsed blank lines:\n{after_first}");

    // The base is still recoverable from the collapsed file.
    assert_eq!(strip_long_tail(&after_first), SCHEMA_KNOWN.trim_end());

    append_long_tail_to_bindings(&path, LONG_TAIL_TS, &formatter).expect("second append");
    let after_second = fs::read_to_string(&path).expect("read back");
    assert_eq!(after_first, after_second);
}

#[test]
fn changed_long_tail_replaces_the_previous_section() {
    let (_dir, path) = seeded_bindings();
    let formatter = double_quote_formatter();
    append_long_tail_to_bindings(&path, "export type Old = 'a';", &formatter).expect("first append");
    append_long_tail_to_bindings(&path, "export type New = 'b';", &formatter).expect("second append");

    let written = fs::read_to_string(&path).expect("read back");
    assert!(written.contains("export type New"), "new section missing; file was:\n{written}");
    assert!(!written.contains("export type Old"), "stale section survived; file was:\n{written}");
    assert!(written.starts_with(SCHEMA_KNOWN), "base was lost; file was:\n{written}");
}

#[test]
fn missing_bindings_file_is_created() {
    // The schema-known emitter writes the file first, but the append path is
    // defensive about it. With no base there is nothing to separate from, so
    // the marker leads the file.
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("types.ts");
    let formatter = double_quote_formatter();

    append_long_tail_to_bindings(&path, LONG_TAIL_TS, &formatter).expect("append");
    let first = fs::read_to_string(&path).expect("read back");
    assert!(first.starts_with(LONG_TAIL_MARKER), "file was:\n{first}");

    // And the empty base still round-trips.
    append_long_tail_to_bindings(&path, LONG_TAIL_TS, &formatter).expect("second append");
    assert_eq!(first, fs::read_to_string(&path).expect("read back"));
}

#[test]
fn append_does_not_rewrite_an_unchanged_file() {
    // `write_and_format_ts` writes only on change. Without that, every
    // rebuild bumps the mtime and file watchers (e.g. `tauri dev`) loop.
    let (_dir, path) = seeded_bindings();
    let formatter = double_quote_formatter();
    append_long_tail_to_bindings(&path, LONG_TAIL_TS, &formatter).expect("first append");
    let mtime = fs::metadata(&path).expect("metadata").modified().expect("mtime");

    append_long_tail_to_bindings(&path, LONG_TAIL_TS, &formatter).expect("second append");
    let after = fs::metadata(&path).expect("metadata").modified().expect("mtime");
    assert_eq!(mtime, after, "unchanged rebuild touched the file");
}

#[test]
fn strip_long_tail_handles_a_file_without_the_marker() {
    assert_eq!(strip_long_tail(SCHEMA_KNOWN), SCHEMA_KNOWN.trim_end());
    assert_eq!(strip_long_tail(""), "");
}

// ── pool_extra_roots crate naming (issue #84) ────────────────────────────

/// Lay out `<dir>/<crate_name>/{Cargo.toml,src}` and return the `src` path.
fn sibling_crate(manifest: Option<&str>, dir_name: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let crate_dir = dir.path().join(dir_name);
    let src = crate_dir.join("src");
    fs::create_dir_all(&src).expect("create src");
    if let Some(manifest) = manifest {
        fs::write(crate_dir.join("Cargo.toml"), manifest).expect("write manifest");
    }
    (dir, src)
}

#[test]
fn extra_root_crate_name_prefers_the_manifest_package_name() {
    // The package name is what a consuming crate writes in a `use`, so it's
    // what the sibling's pool keys have to be rooted at. The directory can
    // differ from it.
    let (_dir, src) =
        sibling_crate(Some("[package]\nname = \"vaultpolish-core\"\nversion = \"0.1.0\"\n"), "core-checkout");
    assert_eq!(extra_root_crate_name(&src), "vaultpolish_core");
}

#[test]
fn extra_root_crate_name_falls_back_to_the_directory() {
    // No readable manifest — a non-standard layout. Sharing a namespace with
    // the consuming crate would be worse than a slightly wrong name.
    let (_dir, src) = sibling_crate(None, "vaultpolish-core");
    assert_eq!(extra_root_crate_name(&src), "vaultpolish_core");
}

#[test]
fn package_name_ignores_names_outside_the_package_table() {
    // `name` appears under plenty of other tables; only `[package]` counts.
    let manifest = "\
[dependencies]
name = \"not-the-package\"

[package]
name = \"real-package\"

[[bin]]
name = \"some-binary\"
";
    assert_eq!(package_name_from_manifest(manifest).as_deref(), Some("real-package"));
}

#[test]
fn package_name_absent_yields_none() {
    assert_eq!(package_name_from_manifest("[workspace]\nmembers = [\"a\"]\n"), None);
}
