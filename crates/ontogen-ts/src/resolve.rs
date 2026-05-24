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

/// Re-export chains can in principle loop (`a` re-exports from `b`, `b` from
/// `a`). Bound the `use`-chain walk so a pathological cycle terminates.
const MAX_IMPORT_DEPTH: u8 = 16;

/// Outcome of resolving a type reference against the pool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    /// Resolved to exactly one pool key.
    Resolved(TypePath),
    /// The reference points outside the pool — a primitive, an external
    /// crate, or an otherwise unresolvable path. No edge / not a root.
    NotInPool,
    /// A bare reference with no disambiguating `use` matched more than one
    /// pool key by terminal segment. The caller decides what to do: a closure
    /// edge ignores it (no mislink), a long-tail root errors (the consuming
    /// crate must qualify or rename).
    Ambiguous(Vec<TypePath>),
}

/// Resolve a type reference — `segments` are the reference's path idents with
/// generic args already stripped (`["BackupManifest"]`,
/// `["crate","models","Workout"]`) — written in `module` (the referencing
/// item's canonical module path, i.e. its pool key minus the terminal).
///
/// A bare single-segment reference resolves in priority order: the referencing
/// module's `use` table (authoritative, followed across re-export chains),
/// then a same-module sibling, then a unique terminal-segment match. A
/// multi-segment reference matches only on an exact pool key. See the
/// [`crate::order`] module docs for the rationale.
pub fn resolve_reference(
    segments: &[String],
    module: &[String],
    pool: &BTreeMap<TypePath, syn::Item>,
    imports: &ModuleImports,
) -> Resolution {
    // Strip a leading `crate::`; pool keys are crate-relative.
    let canonical: &[String] =
        if segments.first().map(String::as_str) == Some("crate") { &segments[1..] } else { segments };
    match canonical {
        [] => Resolution::NotInPool,
        [only] => resolve_bare_ident(only, module, pool, imports, 0),
        multi => match TypePath::new(multi.to_vec()) {
            Ok(tp) if pool.contains_key(&tp) => Resolution::Resolved(tp),
            // A qualified path we don't have as a definition key — external,
            // or a re-export path we don't follow for multi-segment refs.
            _ => Resolution::NotInPool,
        },
    }
}

/// Resolve a bare ident written in `module`. `depth` bounds re-export hops.
fn resolve_bare_ident(
    ident: &str,
    module: &[String],
    pool: &BTreeMap<TypePath, syn::Item>,
    imports: &ModuleImports,
    depth: u8,
) -> Resolution {
    // 1. The module's `use` table — authoritative (an explicit `use` wins in
    //    Rust name resolution).
    if let Some(file_imports) = imports.get(module)
        && let Some(target) = file_imports.resolve_ident(ident)
    {
        return resolve_import_target(target.segments(), module, pool, imports, depth);
    }
    // 2. A sibling defined in the same module, referenced without a `use`.
    let mut same_module = module.to_vec();
    same_module.push(ident.to_string());
    if let Ok(path) = TypePath::new(same_module)
        && pool.contains_key(&path)
    {
        return Resolution::Resolved(path);
    }
    // 3. A unique terminal-segment match across the whole pool.
    terminal_resolution(ident, pool)
}

/// Resolve the path a `use` points at (`target`, as written in `in_module`)
/// to a pool key, following one re-export hop if it lands on another module's
/// re-export rather than a definition.
fn resolve_import_target(
    target: &[String],
    in_module: &[String],
    pool: &BTreeMap<TypePath, syn::Item>,
    imports: &ModuleImports,
    depth: u8,
) -> Resolution {
    // The import's terminal — the type name itself — is the fallback key.
    let Some(leaf) = target.last().cloned() else {
        return Resolution::NotInPool;
    };
    // Normalize the `use` path to an absolute crate-relative path. `None`
    // means it's rooted at another crate (`use chrono::DateTime`, or a
    // `pool_extra_roots` sibling like `use pumice_config::ThemePreference`).
    // We can't tell a true-external crate from an in-pool sibling — the
    // extra-root scan strips the crate name, so a sibling's types are keyed
    // under their own module path (`ui::ThemePreference`, not
    // `pumice_config::ui::ThemePreference`). The only available resolution is
    // a unique terminal match: it finds the sibling type and leaves a genuine
    // external (no pool key with that terminal) as `NotInPool`.
    let Some(abs) = absolutize(target, in_module, pool, imports) else {
        return terminal_resolution(&leaf, pool);
    };
    let Ok(abs_path) = TypePath::new(abs.clone()) else {
        return Resolution::NotInPool;
    };
    // Direct hit: the import names the definition's own module path.
    if pool.contains_key(&abs_path) {
        return Resolution::Resolved(abs_path);
    }
    // Crate-internal but not a definition key — the named module is
    // re-exporting it (`pub use`). Follow the chain one hop further.
    if depth < MAX_IMPORT_DEPTH && abs.len() >= 2 {
        let reexport_module = &abs[..abs.len() - 1];
        match resolve_bare_ident(&leaf, reexport_module, pool, imports, depth + 1) {
            // The re-exporting module names `leaf` explicitly: take its answer.
            resolved @ (Resolution::Resolved(_) | Resolution::Ambiguous(_)) => return resolved,
            // It doesn't (a glob re-export, say) — fall through to terminal.
            Resolution::NotInPool => {}
        }
    }
    terminal_resolution(&leaf, pool)
}

/// Normalize a `use` path written in `in_module` to an absolute crate-relative
/// segment vector. Returns `None` when the path is rooted at an external crate
/// (not `crate`/`self`/`super`, and not a submodule of `in_module`).
fn absolutize(
    target: &[String],
    in_module: &[String],
    pool: &BTreeMap<TypePath, syn::Item>,
    imports: &ModuleImports,
) -> Option<Vec<String>> {
    let (first, rest) = target.split_first()?;
    match first.as_str() {
        "crate" => Some(rest.to_vec()),
        "self" => Some([in_module, rest].concat()),
        "super" => {
            // `use super::X` — parent of the current module, then the rest.
            let parent = in_module.split_last().map(|(_, p)| p)?;
            Some([parent, rest].concat())
        }
        _ => {
            // A bare first segment is either a submodule of `in_module` (the
            // 2018-edition relative form, `pub use vault::X` inside `schema`)
            // or an external crate. Prefer relative when the submodule exists.
            let mut candidate_module = in_module.to_vec();
            candidate_module.push(first.clone());
            if is_known_module(&candidate_module, pool, imports) { Some([in_module, target].concat()) } else { None }
        }
    }
}

/// True when `prefix` names a module that the pool or imports know about —
/// i.e. some pool key has it as a strict ancestor, or it has a `use` table.
fn is_known_module(prefix: &[String], pool: &BTreeMap<TypePath, syn::Item>, imports: &ModuleImports) -> bool {
    if imports.get(prefix).is_some() {
        return true;
    }
    pool.keys().any(|k| {
        let segs = k.segments();
        segs.len() > prefix.len() && &segs[..prefix.len()] == prefix
    })
}

/// The pool key whose terminal segment equals `ident`: `Resolved` when exactly
/// one matches, `NotInPool` for none, `Ambiguous` for more than one.
fn terminal_resolution(ident: &str, pool: &BTreeMap<TypePath, syn::Item>) -> Resolution {
    let matches: Vec<TypePath> = pool.keys().filter(|p| p.terminal() == ident).cloned().collect();
    match matches.len() {
        0 => Resolution::NotInPool,
        1 => Resolution::Resolved(matches.into_iter().next().expect("len checked")),
        _ => Resolution::Ambiguous(matches),
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

    fn pool_from(entries: &[(&[&str], &str)]) -> BTreeMap<TypePath, syn::Item> {
        entries
            .iter()
            .map(|(segs, src)| {
                let key = TypePath::new(segs.iter().map(|s| (*s).to_string()).collect()).expect("non-empty");
                (key, syn::parse_str::<syn::Item>(src).expect("parse item"))
            })
            .collect()
    }

    fn imports_from(entries: &[(&[&str], &str)]) -> ModuleImports {
        let mut imports = ModuleImports::default();
        for (module, src) in entries {
            let file = parse_file(src);
            let prefix: Vec<String> = module.iter().map(|s| (*s).to_string()).collect();
            collect_module_imports(&file, &prefix, &mut imports);
        }
        imports
    }

    fn seg(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| (*s).to_string()).collect()
    }

    // ── resolve_reference ─────────────────────────────────────────────────

    #[test]
    fn reference_resolves_relative_reexport_chain() {
        // The Pumice `VaultConfig` shape, which broke the first cut:
        //   api::v1::vault   `use crate::schema::VaultConfig;`        (the API site)
        //   schema (mod.rs)  `pub use vault::VaultConfig;`            (RELATIVE re-export)
        //   schema::vault    `pub struct VaultConfig { … }`           (the definition)
        //   vault            `pub struct VaultConfig { … }`           (an unrelated same-name type)
        // The bare `VaultConfig` referenced in api::v1::vault must resolve to
        // schema::vault::VaultConfig — through the `use` + relative re-export —
        // NOT to the sibling `vault::VaultConfig`.
        let pool = pool_from(&[
            (&["schema", "vault", "VaultConfig"], "pub struct VaultConfig { pub template: String }"),
            (&["vault", "VaultConfig"], "pub struct VaultConfig { pub enabled: bool }"),
        ]);
        let imports = imports_from(&[
            (&["api", "v1", "vault"], "use crate::schema::VaultConfig;"),
            (&["schema"], "pub use vault::VaultConfig;"),
        ]);
        let r = resolve_reference(&seg(&["VaultConfig"]), &seg(&["api", "v1", "vault"]), &pool, &imports);
        assert_eq!(r, Resolution::Resolved(tp(&["schema", "vault", "VaultConfig"])), "got {r:?}");
    }

    #[test]
    fn reference_without_disambiguating_use_is_ambiguous() {
        // Same colliding pool, but the referencing module has no `use` for
        // `VaultConfig` — the resolver must report Ambiguous, never guess.
        let pool = pool_from(&[
            (&["schema", "vault", "VaultConfig"], "pub struct VaultConfig { pub template: String }"),
            (&["vault", "VaultConfig"], "pub struct VaultConfig { pub enabled: bool }"),
        ]);
        let imports = ModuleImports::default();
        let r = resolve_reference(&seg(&["VaultConfig"]), &seg(&["api", "v1", "vault"]), &pool, &imports);
        match r {
            Resolution::Ambiguous(cands) => assert_eq!(cands.len(), 2, "got {cands:?}"),
            other => panic!("expected Ambiguous, got {other:?}"),
        }
    }

    #[test]
    fn reference_through_crate_absolute_reexport_chain() {
        // Same as the relative case but the facade re-exports with an absolute
        // `pub use crate::core::Foo;`.
        let pool = pool_from(&[(&["core", "Foo"], "pub struct Foo { pub x: u32 }")]);
        let imports = imports_from(&[(&["c"], "use crate::facade::Foo;"), (&["facade"], "pub use crate::core::Foo;")]);
        let r = resolve_reference(&seg(&["Foo"]), &seg(&["c"]), &pool, &imports);
        assert_eq!(r, Resolution::Resolved(tp(&["core", "Foo"])), "got {r:?}");
    }

    #[test]
    fn cross_crate_use_resolves_via_unique_terminal() {
        // `use pumice_config::ThemePreference;` — `pumice_config` is a
        // `pool_extra_roots` sibling crate, so its type is in the pool keyed
        // under its own module (`ui::ThemePreference`), with the crate name
        // stripped. The flat pool can't distinguish this from a true-external
        // crate, so it resolves via a unique terminal match.
        let pool = pool_from(&[(&["ui", "ThemePreference"], "pub enum ThemePreference { Light, Dark }")]);
        let imports = imports_from(&[(&["schema", "settings"], "use pumice_config::ThemePreference;")]);
        let r = resolve_reference(&seg(&["ThemePreference"]), &seg(&["schema", "settings"]), &pool, &imports);
        assert_eq!(r, Resolution::Resolved(tp(&["ui", "ThemePreference"])), "got {r:?}");
    }

    #[test]
    fn external_use_with_no_pool_match_is_not_in_pool() {
        // `use chrono::DateTime;` with no pool type sharing the terminal —
        // genuinely external, so no resolution.
        let pool = pool_from(&[(&["models", "Workout"], "pub struct Workout { pub id: u64 }")]);
        let imports = imports_from(&[(&["c"], "use chrono::DateTime;")]);
        let r = resolve_reference(&seg(&["DateTime"]), &seg(&["c"]), &pool, &imports);
        assert_eq!(r, Resolution::NotInPool, "got {r:?}");
    }

    #[test]
    fn reference_unique_terminal_without_imports_resolves() {
        // No imports, bare ident, exactly one pool key with that terminal.
        let pool = pool_from(&[(&["schema", "backup", "BackupManifest"], "pub struct BackupManifest { pub v: u32 }")]);
        let r = resolve_reference(&seg(&["BackupManifest"]), &seg(&["api"]), &pool, &ModuleImports::default());
        assert_eq!(r, Resolution::Resolved(tp(&["schema", "backup", "BackupManifest"])), "got {r:?}");
    }

    #[test]
    fn reference_qualified_crate_path_matches_exact_key() {
        let pool = pool_from(&[(&["models", "Workout"], "pub struct Workout { pub id: u64 }")]);
        let r =
            resolve_reference(&seg(&["crate", "models", "Workout"]), &seg(&["api"]), &pool, &ModuleImports::default());
        assert_eq!(r, Resolution::Resolved(tp(&["models", "Workout"])), "got {r:?}");
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
