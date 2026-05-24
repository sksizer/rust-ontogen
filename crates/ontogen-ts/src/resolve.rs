//! Per-file `use`-resolution + canonical path normalization.
//!
//! When the pool walker encounters a reference to a type in some struct/enum
//! field, the reference is typically one segment (`DateTime`) or
//! crate-relative (`crate::models::Workout`). The pool, on the other hand,
//! is keyed by canonical paths derived from where each item was defined
//! (`["models", "Workout"]`). Bridging the two requires reading the source
//! file's `use` declarations and turning them into a lookup table that
//! one-segment references can consult.
//!
//! Rules (matching the OF-015 design pass's "Use-resolution / path
//! canonicalization" decision):
//!
//! - One-segment ref (`DateTime`): consult the file's imports. If the ident
//!   has a `use` entry, the entry's canonical path wins. If not, fall back
//!   to "single-segment ident under the current module" — the pool may have
//!   a matching local key.
//! - Multi-segment ref (`chrono::DateTime`, `crate::models::Workout`):
//!   take as-qualified. `crate::` prefix is stripped before pool lookup so
//!   keys stay project-relative.
//! - Glob imports (`use chrono::*`) — recorded but raise
//!   [`EmitError::UnresolvedReference`] when a one-segment ref needs them
//!   for resolution, since walking the imported crate's source is out of
//!   phase-1 scope.
//!
//! `#[allow(dead_code)]` is module-wide. As of the closure-edge fix,
//! [`ModuleImports`] / [`collect_module_imports`] / [`FileImports::resolve_ident`]
//! ARE wired into the production dep extractor (`order::DepCollector`): a
//! bare single-segment reference is resolved through its module's `use`
//! table before any terminal-segment guessing, so a type imported from one
//! of several same-terminal modules links to the right pool key. The
//! render-side resolver (`emit::emit_type`'s fall-through) and the
//! [`canonicalize`] / glob-hint helpers remain staged for a later pass; the
//! module-wide allow covers those still-unused surfaces.

#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};

use syn::{Item, Path, UseTree};

use crate::types::{EmitError, TypePath};

/// Per-file lookup table built from `use` declarations.
#[derive(Debug, Clone, Default)]
pub(crate) struct FileImports {
    /// `Ident` → canonical path. Populated from `use foo::Bar`, `use foo::Bar as Baz`,
    /// and `use foo::{Bar, Baz}` declarations.
    pub(crate) simple: BTreeMap<String, TypePath>,
    /// Prefixes brought in by glob imports (`use chrono::*`). Stored as
    /// canonical paths whose terminal segment is `*` semantically (we keep
    /// only the prefix here). Used to surface a helpful hint when a
    /// one-segment ref can't be resolved.
    pub(crate) globs: BTreeSet<TypePath>,
}

impl FileImports {
    /// Resolve a one-segment ident through the imports table.
    /// Returns `Some(canonical_path)` if found.
    pub(crate) fn resolve_ident(&self, ident: &str) -> Option<TypePath> {
        self.simple.get(ident).cloned()
    }
}

/// Walk a parsed `syn::File`'s top-level `use` declarations and build the
/// imports table.
pub(crate) fn parse_imports(file: &syn::File) -> FileImports {
    let mut out = FileImports::default();
    imports_from_items(&file.items, &mut out);
    out
}

/// Accumulate the `use` declarations directly contained in `items` into
/// `out`. Does not descend into inline `mod` blocks — those define their own
/// scope (see [`collect_module_imports`]).
fn imports_from_items(items: &[Item], out: &mut FileImports) {
    for item in items {
        if let Item::Use(item_use) = item {
            walk_use_tree(&item_use.tree, &mut Vec::new(), out);
        }
    }
}

/// Per-module `use` tables for a scanned source tree, keyed by the module's
/// canonical path segments (empty = crate root). The keys mirror the type
/// pool's key prefixes, so a referencing item's module — the pool key with
/// its terminal dropped — looks up directly.
#[derive(Debug, Clone, Default)]
pub struct ModuleImports {
    by_module: BTreeMap<Vec<String>, FileImports>,
}

impl ModuleImports {
    /// The `use` table in scope for `module`, if any were recorded.
    pub(crate) fn get(&self, module: &[String]) -> Option<&FileImports> {
        self.by_module.get(module)
    }

    /// Fold another tree's tables in. On a module-path collision the existing
    /// entry wins, matching the pool's "first root wins" merge policy in
    /// `src/clients/mod.rs`.
    pub fn merge(&mut self, other: ModuleImports) {
        for (module, imports) in other.by_module {
            self.by_module.entry(module).or_insert(imports);
        }
    }
}

/// Walk a parsed file's `use` declarations — including those inside inline
/// `mod foo { ... }` blocks — into `out`, keyed by module path. `prefix` is
/// the canonical path of the file's own module (empty at the crate root),
/// matching the pool walker's `module_prefix`.
pub(crate) fn collect_module_imports(file: &syn::File, prefix: &[String], out: &mut ModuleImports) {
    collect_items_into(&file.items, prefix, out);
}

fn collect_items_into(items: &[Item], prefix: &[String], out: &mut ModuleImports) {
    let entry = out.by_module.entry(prefix.to_vec()).or_default();
    imports_from_items(items, entry);
    for item in items {
        if let Item::Mod(m) = item
            && let Some((_, inner)) = &m.content
        {
            let mut sub = prefix.to_vec();
            sub.push(m.ident.to_string());
            collect_items_into(inner, &sub, out);
        }
    }
}

/// Recursive walker over `syn::UseTree` — the shape `use a::{b, c::d as e, f::*}`
/// builds up.
fn walk_use_tree(tree: &UseTree, prefix: &mut Vec<String>, out: &mut FileImports) {
    match tree {
        UseTree::Path(p) => {
            prefix.push(p.ident.to_string());
            walk_use_tree(&p.tree, prefix, out);
            prefix.pop();
        }
        UseTree::Name(name) => {
            // `use foo::Bar;` — `name.ident == "Bar"`; prefix is `["foo"]`.
            let ident = name.ident.to_string();
            let mut segments = prefix.clone();
            segments.push(ident.clone());
            if let Ok(path) = TypePath::new(segments) {
                out.simple.insert(ident, path);
            }
        }
        UseTree::Rename(rename) => {
            // `use foo::Bar as Baz;` — local ident is `Baz`, canonical is
            // `prefix::Bar`.
            let canonical_ident = rename.ident.to_string();
            let local_ident = rename.rename.to_string();
            let mut segments = prefix.clone();
            segments.push(canonical_ident);
            if let Ok(path) = TypePath::new(segments) {
                out.simple.insert(local_ident, path);
            }
        }
        UseTree::Glob(_) => {
            // `use foo::bar::*;` — record the prefix; one-segment refs hit
            // `UnresolvedReference` with a hint that this glob may be the
            // missing source.
            if !prefix.is_empty()
                && let Ok(path) = TypePath::new(prefix.clone())
            {
                out.globs.insert(path);
            }
        }
        UseTree::Group(group) => {
            for inner in &group.items {
                walk_use_tree(inner, prefix, out);
            }
        }
    }
}

/// Strip generic args from a [`syn::Path`] and return the segment idents as
/// a vector. `Path<A, B>` → `["Path"]`; `foo::bar::Baz<u32>` →
/// `["foo", "bar", "Baz"]`.
fn path_segments(path: &Path) -> Vec<String> {
    path.segments.iter().map(|seg| seg.ident.to_string()).collect()
}

/// Canonicalize a referenced `syn::Path` against the file's imports.
///
/// `referenced_by` is the type whose field carries this reference — included
/// in errors for context.
///
/// Returns the canonical [`TypePath`] suitable for lookup in either the
/// pool (project-relative) or the external-types table (full canonical
/// path).
pub(crate) fn canonicalize(
    path: &Path,
    imports: &FileImports,
    referenced_by: &TypePath,
) -> Result<TypePath, EmitError> {
    let mut segments = path_segments(path);

    if segments.is_empty() {
        return Err(EmitError::UnresolvedReference {
            name: "<empty path>".to_string(),
            referenced_by: referenced_by.clone(),
        });
    }

    // Multi-segment path: take as-qualified, strip `crate::` for pool lookup.
    if segments.len() > 1 {
        if segments.first().map(String::as_str) == Some("crate") {
            segments.remove(0);
        }
        return TypePath::new(segments).map_err(|_| EmitError::UnresolvedReference {
            name: "<empty after crate:: stripped>".to_string(),
            referenced_by: referenced_by.clone(),
        });
    }

    // One-segment ident: consult imports.
    let ident = &segments[0];
    if let Some(path) = imports.resolve_ident(ident) {
        return Ok(path);
    }

    // Not in imports. If any glob imports are present, surface a hint —
    // the ident may live in one of those globs and we can't tell without
    // walking the imported crate's source.
    if !imports.globs.is_empty() {
        let globs_rendered: Vec<String> =
            imports.globs.iter().map(|p| format!("use {}::*;", p.segments().join("::"))).collect();
        return Err(EmitError::UnresolvedReference {
            name: format!(
                "`{ident}` (may come from {}; qualify the reference (e.g., chrono::{ident}) or replace the glob with \
                 an explicit `use`)",
                globs_rendered.join(", ")
            ),
            referenced_by: referenced_by.clone(),
        });
    }

    // Bare one-segment ident with no matching `use`: treat as a local type
    // at the crate root. The pool walker may have it; the lookup happens
    // at the call site. If neither pool nor external-types match, the
    // emitter surfaces `UnresolvedReference` later.
    TypePath::new(vec![ident.clone()])
        .map_err(|_| EmitError::UnresolvedReference { name: ident.clone(), referenced_by: referenced_by.clone() })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_file(src: &str) -> syn::File {
        syn::parse_str(src).expect("parse file")
    }

    fn tp(segments: &[&str]) -> TypePath {
        TypePath::new(segments.iter().map(|s| (*s).to_string()).collect()).expect("non-empty")
    }

    fn parse_path(src: &str) -> Path {
        syn::parse_str(src).expect("parse path")
    }

    // ── parse_imports ─────────────────────────────────────────────────────

    #[test]
    fn parse_simple_use() {
        let f = parse_file("use chrono::DateTime;");
        let imports = parse_imports(&f);
        assert_eq!(imports.simple.get("DateTime"), Some(&tp(&["chrono", "DateTime"])));
    }

    #[test]
    fn parse_use_with_rename() {
        let f = parse_file("use chrono::DateTime as Moment;");
        let imports = parse_imports(&f);
        assert_eq!(imports.simple.get("Moment"), Some(&tp(&["chrono", "DateTime"])));
        // The original ident isn't re-mapped.
        assert!(imports.simple.get("DateTime").is_none());
    }

    #[test]
    fn parse_use_with_group() {
        let f = parse_file("use chrono::{DateTime, NaiveDate, NaiveTime};");
        let imports = parse_imports(&f);
        assert_eq!(imports.simple.get("DateTime"), Some(&tp(&["chrono", "DateTime"])));
        assert_eq!(imports.simple.get("NaiveDate"), Some(&tp(&["chrono", "NaiveDate"])));
        assert_eq!(imports.simple.get("NaiveTime"), Some(&tp(&["chrono", "NaiveTime"])));
    }

    #[test]
    fn parse_nested_group() {
        let f = parse_file("use foo::{bar::Baz, qux::{Quux, Quuux as Q}};");
        let imports = parse_imports(&f);
        assert_eq!(imports.simple.get("Baz"), Some(&tp(&["foo", "bar", "Baz"])));
        assert_eq!(imports.simple.get("Quux"), Some(&tp(&["foo", "qux", "Quux"])));
        assert_eq!(imports.simple.get("Q"), Some(&tp(&["foo", "qux", "Quuux"])));
    }

    #[test]
    fn parse_glob_import() {
        let f = parse_file("use chrono::*;");
        let imports = parse_imports(&f);
        assert!(imports.globs.contains(&tp(&["chrono"])));
        assert!(imports.simple.is_empty());
    }

    #[test]
    fn parse_multiple_glob_imports() {
        let f = parse_file("use chrono::*; use uuid::*;");
        let imports = parse_imports(&f);
        assert!(imports.globs.contains(&tp(&["chrono"])));
        assert!(imports.globs.contains(&tp(&["uuid"])));
    }

    // ── canonicalize ──────────────────────────────────────────────────────

    #[test]
    fn canonicalize_single_segment_via_imports() {
        let f = parse_file("use chrono::DateTime;");
        let imports = parse_imports(&f);
        let path = parse_path("DateTime");
        let resolved = canonicalize(&path, &imports, &tp(&["Foo"])).unwrap();
        assert_eq!(resolved, tp(&["chrono", "DateTime"]));
    }

    #[test]
    fn canonicalize_single_segment_via_rename() {
        let f = parse_file("use chrono::DateTime as Moment;");
        let imports = parse_imports(&f);
        let path = parse_path("Moment");
        let resolved = canonicalize(&path, &imports, &tp(&["Foo"])).unwrap();
        assert_eq!(resolved, tp(&["chrono", "DateTime"]));
    }

    #[test]
    fn canonicalize_unresolved_single_segment_falls_through() {
        // No imports, no globs — treated as a bare local ident.
        let f = parse_file("");
        let imports = parse_imports(&f);
        let path = parse_path("MyWorkout");
        let resolved = canonicalize(&path, &imports, &tp(&["Foo"])).unwrap();
        assert_eq!(resolved, tp(&["MyWorkout"]));
    }

    #[test]
    fn canonicalize_unresolved_with_glob_emits_hint() {
        let f = parse_file("use chrono::*;");
        let imports = parse_imports(&f);
        let path = parse_path("DateTime");
        let err = canonicalize(&path, &imports, &tp(&["Foo"])).unwrap_err();
        match err {
            EmitError::UnresolvedReference { name, .. } => {
                assert!(name.contains("DateTime"), "name was: {name}");
                assert!(name.contains("chrono"), "name was: {name}");
                assert!(name.contains("glob") || name.contains("qualify"), "hint missing: {name}");
            }
            other => panic!("expected UnresolvedReference, got {other:?}"),
        }
    }

    #[test]
    fn canonicalize_multi_segment_taken_as_qualified() {
        let f = parse_file("");
        let imports = parse_imports(&f);
        let path = parse_path("chrono::DateTime");
        let resolved = canonicalize(&path, &imports, &tp(&["Foo"])).unwrap();
        assert_eq!(resolved, tp(&["chrono", "DateTime"]));
    }

    #[test]
    fn canonicalize_strips_crate_prefix() {
        let f = parse_file("");
        let imports = parse_imports(&f);
        let path = parse_path("crate::models::Workout");
        let resolved = canonicalize(&path, &imports, &tp(&["Foo"])).unwrap();
        // `crate::` stripped — pool keys are crate-relative.
        assert_eq!(resolved, tp(&["models", "Workout"]));
    }

    #[test]
    fn canonicalize_strips_generic_args() {
        let f = parse_file("");
        let imports = parse_imports(&f);
        let path = parse_path("chrono::DateTime<Utc>");
        let resolved = canonicalize(&path, &imports, &tp(&["Foo"])).unwrap();
        // Generic args don't affect the canonical name.
        assert_eq!(resolved, tp(&["chrono", "DateTime"]));
    }
}
