//! Dependency graph + topological ordering for the type pool.
//!
//! Given a pool keyed by canonical [`TypePath`], extract each item's
//! same-pool references and produce a deterministic topological order so
//! emitted TypeScript declares types before they're referenced (where the
//! pool's transitive shape allows — cycles are co-emitted as a group at
//! the cycle's topo level, since TS type aliases accept forward references
//! freely).
//!
//! Edge-extraction strategy:
//!
//! - For each pool item, recursively walk its fields/variants via
//!   [`syn::visit::Visit`].
//! - For each `syn::Type::Path` encountered, drop generic args, strip a
//!   leading `crate::` segment, and synthesize a candidate [`TypePath`]
//!   from the remaining segments.
//! - A multi-segment candidate matches only on an exact pool key.
//! - A single-segment candidate (`BackupManifest`, typically brought in via
//!   `use`) is resolved in priority order:
//!     1. **The referencing module's `use` table.** If a `use` names the
//!        ident, that import is authoritative — Rust name resolution gives it
//!        precedence — so we link to exactly that path (and to nothing, if
//!        the import points outside the pool). This is what disambiguates
//!        `BackupManifest` when both `a::BackupManifest` and
//!        `b::BackupManifest` exist: the `use` says which one.
//!     2. **A same-module sibling** (`module + [ident]`), for types defined
//!        alongside the referrer with no `use` needed.
//!     3. **A unique terminal-segment match** across the whole pool, used
//!        only when exactly one pool key ends in `ident`. With zero or more
//!        than one candidate we record no edge rather than guess — a wrong
//!        edge would emit the wrong type's body under the shared TS name.
//! - Anything still unmatched (primitives, external types) is ignored here;
//!   the per-type emitter handles those at render time.
//!
//! Single-segment resolution matters for **correctness**, not just ordering:
//! [`reachable_from`] walks this same graph to decide *which* types get
//! emitted. A nested-only type — one never named directly in an API
//! signature, only reached through a sibling field by bare ident — would
//! otherwise be dropped from the closure entirely and emitted as an undefined
//! reference. (Earlier revisions resolved only exact pool keys here, on the
//! assumption that a missing edge merely produced a forward TS reference;
//! that assumption held for ordering but not for the reachable set.)
//!
//! The `use`-aware path requires per-module imports ([`ModuleImports`], from
//! [`crate::pool::scan_src_dir_with_imports`]). The bare [`dependency_graph`]
//! entry point passes none and so relies on the same-module and
//! unique-terminal rules — still safe (it never mislinks), just blind to
//! cross-module `use` imports; [`dependency_graph_with_imports`] is the
//! import-aware production entry point.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use syn::visit::{self, Visit};

use crate::resolve::ModuleImports;
use crate::types::TypePath;

/// Build the dependency graph from a pool without per-module import tables.
///
/// Single-segment references resolve via same-module and unique-terminal
/// matching only (see the module docs); cross-module `use` imports aren't
/// consulted. Production always has imports and goes through
/// [`dependency_graph_with_imports`]; this no-imports wrapper exists for the
/// unit tests that build synthetic pools with no `use` context.
#[cfg(test)]
pub(crate) fn dependency_graph(pool: &BTreeMap<TypePath, syn::Item>) -> BTreeMap<TypePath, BTreeSet<TypePath>> {
    dependency_graph_with_imports(pool, &ModuleImports::default())
}

/// Build the dependency graph from a pool, resolving bare single-segment
/// references through each referencing module's `use` table (`imports`).
///
/// Each node is a pool key; each edge `a → b` means item `a` references item
/// `b` in its fields/variants. The graph is restricted to in-pool edges
/// only: a reference to an external type (e.g. `chrono::DateTime`) or an
/// unresolvable ident doesn't get an edge.
pub(crate) fn dependency_graph_with_imports(
    pool: &BTreeMap<TypePath, syn::Item>,
    imports: &ModuleImports,
) -> BTreeMap<TypePath, BTreeSet<TypePath>> {
    let mut graph: BTreeMap<TypePath, BTreeSet<TypePath>> = BTreeMap::new();
    for (key, item) in pool {
        // The referencing item's module is its pool key with the terminal
        // (the type's own ident) dropped.
        let module = &key.segments()[..key.segments().len() - 1];
        let mut visitor = DepCollector::new(pool, module, imports);
        visitor.visit_item(item);
        graph.insert(key.clone(), visitor.deps);
    }
    graph
}

/// Compute the transitive closure of `roots` against the graph — every
/// node reachable from a root via zero or more edges.
pub(crate) fn reachable_from(roots: &[TypePath], graph: &BTreeMap<TypePath, BTreeSet<TypePath>>) -> BTreeSet<TypePath> {
    let mut visited: BTreeSet<TypePath> = BTreeSet::new();
    let mut stack: Vec<TypePath> = roots.to_vec();
    while let Some(node) = stack.pop() {
        if !visited.insert(node.clone()) {
            continue;
        }
        if let Some(edges) = graph.get(&node) {
            for dep in edges {
                if !visited.contains(dep) {
                    stack.push(dep.clone());
                }
            }
        }
    }
    visited
}

/// Topologically order `reachable` against the dependency graph using
/// Kahn's algorithm. Within each topo level, BTreeSet iteration gives the
/// alphabetical-by-canonical-path tiebreaker for free.
///
/// Cycles: nodes still in the graph after Kahn's terminates are appended
/// to the output in alphabetical order. TS type aliases accept forward
/// references, so co-emitting cycle members works without further special
/// handling.
pub(crate) fn topo_order(
    graph: &BTreeMap<TypePath, BTreeSet<TypePath>>,
    reachable: &BTreeSet<TypePath>,
) -> Vec<TypePath> {
    // Build restricted graph: only edges where both endpoints are in
    // `reachable`. Outgoing edges per node, incoming edge count per node.
    let mut in_edges: BTreeMap<TypePath, BTreeSet<TypePath>> = BTreeMap::new();
    let mut out_edges: BTreeMap<TypePath, BTreeSet<TypePath>> = BTreeMap::new();

    for node in reachable {
        in_edges.entry(node.clone()).or_default();
        out_edges.entry(node.clone()).or_default();
    }

    for node in reachable {
        let deps = match graph.get(node) {
            Some(set) => set,
            None => continue,
        };
        for dep in deps {
            if !reachable.contains(dep) || dep == node {
                continue;
            }
            // Direction: a node depends on its deps, so deps must come
            // first. We model this as `dep → node` (dep is required before
            // node). Then Kahn's starts with nodes that have no incoming
            // dependencies (nothing they depend on).
            out_edges.entry(dep.clone()).or_default().insert(node.clone());
            in_edges.entry(node.clone()).or_default().insert(dep.clone());
        }
    }

    // Kahn's: nodes with zero in-edges first, alphabetical (BTreeSet
    // iteration order gives this for free).
    let mut queue: VecDeque<TypePath> = VecDeque::new();
    for (node, deps) in &in_edges {
        if deps.is_empty() {
            queue.push_back(node.clone());
        }
    }

    let mut output: Vec<TypePath> = Vec::with_capacity(reachable.len());
    while let Some(node) = queue.pop_front() {
        output.push(node.clone());
        let successors = out_edges.get(&node).cloned().unwrap_or_default();
        for succ in successors {
            if let Some(incoming) = in_edges.get_mut(&succ) {
                incoming.remove(&node);
                if incoming.is_empty() {
                    queue.push_back(succ);
                }
            }
        }
    }

    // Cycle members — anything in `reachable` not yet emitted. Append in
    // alphabetical order (BTreeSet gives that automatically when we
    // collect).
    if output.len() < reachable.len() {
        let emitted: BTreeSet<&TypePath> = output.iter().collect();
        let remaining: BTreeSet<&TypePath> = reachable.iter().filter(|n| !emitted.contains(n)).collect();
        for node in remaining {
            output.push(node.clone());
        }
    }

    output
}

/// Re-export chains can in principle loop (`a` re-exports from `b`, `b` from
/// `a`). Bound the `use`-chain walk so a pathological cycle terminates.
const MAX_IMPORT_DEPTH: u8 = 16;

/// `syn::visit::Visit` that records every in-pool type referenced by an item.
struct DepCollector<'a> {
    pool: &'a BTreeMap<TypePath, syn::Item>,
    /// Canonical path of the module the referencing item lives in (its pool
    /// key with the terminal dropped). Roots the `use`-table and same-module
    /// lookups for bare single-segment references.
    module: &'a [String],
    /// Per-module `use` tables for the whole scanned tree. Empty when the
    /// caller went through the bare [`dependency_graph`] entry point.
    imports: &'a ModuleImports,
    deps: BTreeSet<TypePath>,
}

impl<'a> DepCollector<'a> {
    fn new(pool: &'a BTreeMap<TypePath, syn::Item>, module: &'a [String], imports: &'a ModuleImports) -> Self {
        Self { pool, module, imports, deps: BTreeSet::new() }
    }

    /// Resolve a bare single-segment reference (`BackupManifest`) to a pool
    /// key, recording an edge if one is found. Resolution order matches Rust
    /// name resolution closely enough for wire types: an explicit `use`
    /// wins outright, then a same-module sibling, then a unique
    /// terminal-segment match.
    fn resolve_single_segment(&mut self, ident: &str) {
        // 1. The referencing module's `use` table (authoritative).
        match self.resolve_import_chain(self.module, ident, 0) {
            // `use` names the ident and it lands on a pool key.
            Some(Some(key)) => {
                self.deps.insert(key);
                return;
            }
            // `use` names the ident but it resolves outside the pool (an
            // external crate, or an unresolvable/ambiguous re-export). The
            // import is authoritative — Rust would never look past it — so we
            // do NOT fall through to same-module or terminal guessing.
            Some(None) => return,
            // No `use` for this ident here; keep going.
            None => {}
        }

        // 2. A sibling defined in the same module, referenced without a `use`.
        let mut same_module = self.module.to_vec();
        same_module.push(ident.to_string());
        if let Ok(path) = TypePath::new(same_module)
            && self.pool.contains_key(&path)
        {
            self.deps.insert(path);
            return;
        }

        // 3. A unique terminal-segment match across the whole pool — the
        //    nested-only-type case with no `use` recorded (e.g. the bare
        //    [`dependency_graph`] path, or a glob import). Only when exactly
        //    one pool key ends in `ident`: with zero we leave it to the
        //    emitter (external type), and with more than one we refuse to
        //    guess rather than mislink to the wrong same-terminal type.
        if let Some(key) = self.unique_terminal(ident) {
            self.deps.insert(key);
        }
    }

    /// Resolve `ident` through `module`'s `use` table, following re-export
    /// chains across modules (`use crate::facade::Foo` where `facade` does
    /// `pub use crate::core::Foo`) until the trail lands on a pool key.
    ///
    /// Returns:
    /// - `None` — `module` has no `use` bringing `ident` into scope.
    /// - `Some(None)` — `ident` IS imported, but the trail leads outside the
    ///   pool (external crate, or an ambiguous/unresolvable re-export).
    /// - `Some(Some(key))` — `ident` is imported and resolves to pool `key`.
    fn resolve_import_chain(&self, module: &[String], ident: &str, depth: u8) -> Option<Option<TypePath>> {
        let file_imports = self.imports.get(module)?;
        let target = file_imports.resolve_ident(ident)?;
        Some(self.resolve_import_target(&target, depth))
    }

    /// Resolve the path a `use` points at (`target`) to a pool key, following
    /// one more hop of re-export if needed.
    fn resolve_import_target(&self, target: &TypePath, depth: u8) -> Option<TypePath> {
        // Only crate-rooted paths (`use crate::a::Foo`) can name a pool type.
        // A path led by an external crate (`use chrono::DateTime`) — or by the
        // relative `self`/`super`, which we don't track precisely — resolves
        // outside the pool. The import is authoritative, so return `None`
        // (no edge) WITHOUT a terminal-fallback guess that could mislink to a
        // same-named pool type.
        if target.segments().first().map(String::as_str) != Some("crate") {
            return None;
        }
        // Strip the leading `crate`; pool keys are crate-relative.
        let canonical = TypePath::new(target.segments()[1..].to_vec()).ok()?;

        // Direct hit: the import names the definition's own module path.
        if self.pool.contains_key(&canonical) {
            return Some(canonical);
        }

        // Not a definition key, but crate-internal — the named module must be
        // re-exporting it. Follow the chain one hop further (`crate::facade::Foo`
        // where `facade` does `pub use crate::core::Foo`).
        let leaf = canonical.terminal();
        if depth < MAX_IMPORT_DEPTH && canonical.segments().len() >= 2 {
            let reexport_module = &canonical.segments()[..canonical.segments().len() - 1];
            if let Some(resolved) = self.resolve_import_chain(reexport_module, leaf, depth + 1) {
                return resolved;
            }
        }

        // The re-exporting module doesn't name `leaf` with an explicit `use`
        // (a glob re-export, say) or we ran out of hops. Last resort, and only
        // because the path was crate-internal: a unique terminal match.
        self.unique_terminal(leaf)
    }

    /// The single pool key whose terminal segment equals `ident`, or `None`
    /// when there are zero or more than one — i.e. only when the match is
    /// unambiguous.
    fn unique_terminal(&self, ident: &str) -> Option<TypePath> {
        let mut matches = self.pool.keys().filter(|p| p.terminal() == ident);
        let first = matches.next()?;
        match matches.next() {
            Some(_) => None,
            None => Some(first.clone()),
        }
    }
}

impl<'ast> Visit<'ast> for DepCollector<'_> {
    fn visit_type_path(&mut self, node: &'ast syn::TypePath) {
        if node.qself.is_none() {
            let segments: Vec<String> = node.path.segments.iter().map(|s| s.ident.to_string()).collect();
            if !segments.is_empty() {
                // Strip leading `crate::` so the key matches pool conventions.
                let mut canonical = segments.clone();
                if canonical.first().map(String::as_str) == Some("crate") {
                    canonical.remove(0);
                }
                if let Ok(path) = TypePath::new(canonical.clone()) {
                    if self.pool.contains_key(&path) {
                        // Exact pool key — an in-module type or a fully
                        // qualified multi-segment reference.
                        self.deps.insert(path);
                    } else if canonical.len() == 1 {
                        // A bare ident: resolve through imports / same-module /
                        // unique-terminal (see `resolve_single_segment`).
                        self.resolve_single_segment(&canonical[0]);
                    }
                    // Multi-segment non-pool paths (e.g. `chrono::DateTime`)
                    // are external — no edge; the emitter renders them.
                }
            }
        }
        // Recurse into generic args so `Vec<Workout>` records the
        // `Workout` dep.
        visit::visit_type_path(self, node);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tp(segments: &[&str]) -> TypePath {
        TypePath::new(segments.iter().map(|s| (*s).to_string()).collect()).expect("non-empty")
    }

    fn parse_item(src: &str) -> syn::Item {
        syn::parse_str(src).expect("parse item")
    }

    fn pool_from(entries: Vec<(TypePath, &str)>) -> BTreeMap<TypePath, syn::Item> {
        entries.into_iter().map(|(k, src)| (k, parse_item(src))).collect()
    }

    /// Build per-module `use` tables from `(module_path, source)` pairs,
    /// running the real `collect_module_imports` parser so the tests exercise
    /// the production import-extraction path.
    fn imports_from(entries: &[(&[&str], &str)]) -> ModuleImports {
        let mut imports = ModuleImports::default();
        for (module, src) in entries {
            let file: syn::File = syn::parse_str(src).expect("parse module source");
            let prefix: Vec<String> = module.iter().map(|s| (*s).to_string()).collect();
            crate::resolve::collect_module_imports(&file, &prefix, &mut imports);
        }
        imports
    }

    // ── dependency_graph ──────────────────────────────────────────────────

    #[test]
    fn no_deps_for_primitive_only_struct() {
        let pool = pool_from(vec![(tp(&["Foo"]), "pub struct Foo { pub x: u32 }")]);
        let graph = dependency_graph(&pool);
        assert_eq!(graph.get(&tp(&["Foo"])), Some(&BTreeSet::new()));
    }

    #[test]
    fn struct_field_referencing_pool_type_creates_edge() {
        let pool = pool_from(vec![
            (tp(&["Foo"]), "pub struct Foo { pub bar: Bar }"),
            (tp(&["Bar"]), "pub struct Bar { pub x: u32 }"),
        ]);
        let graph = dependency_graph(&pool);
        let foo_deps = graph.get(&tp(&["Foo"])).expect("Foo edges");
        assert!(foo_deps.contains(&tp(&["Bar"])));
    }

    #[test]
    fn external_type_ref_creates_no_edge() {
        let pool = pool_from(vec![(tp(&["Foo"]), "pub struct Foo { pub when: chrono::DateTime<Utc> }")]);
        let graph = dependency_graph(&pool);
        // chrono::DateTime isn't in the pool → no edge.
        assert!(graph.get(&tp(&["Foo"])).unwrap().is_empty());
    }

    #[test]
    fn enum_variant_payload_creates_edge() {
        let pool = pool_from(vec![
            (tp(&["Msg"]), "pub enum Msg { Click(Click), Hover }"),
            (tp(&["Click"]), "pub struct Click { pub x: i32 }"),
        ]);
        let graph = dependency_graph(&pool);
        assert!(graph.get(&tp(&["Msg"])).unwrap().contains(&tp(&["Click"])));
    }

    #[test]
    fn vec_of_pool_type_records_inner_dep() {
        let pool = pool_from(vec![
            (tp(&["Folder"]), "pub struct Folder { pub items: Vec<Item> }"),
            (tp(&["Item"]), "pub struct Item { pub n: u32 }"),
        ]);
        let graph = dependency_graph(&pool);
        assert!(graph.get(&tp(&["Folder"])).unwrap().contains(&tp(&["Item"])));
    }

    #[test]
    fn crate_prefix_stripped_for_pool_lookup() {
        let pool = pool_from(vec![
            (tp(&["models", "Workout"]), "pub struct Workout { pub id: u64 }"),
            // Foo references crate::models::Workout.
            (tp(&["Foo"]), "pub struct Foo { pub w: crate::models::Workout }"),
        ]);
        let graph = dependency_graph(&pool);
        assert!(graph.get(&tp(&["Foo"])).unwrap().contains(&tp(&["models", "Workout"])));
    }

    #[test]
    fn single_segment_ref_resolves_to_nested_module_key() {
        // `RestoreCandidate` references `BackupManifest` by bare ident (the
        // type is `use`d, not written with a module path), but the pool keys
        // `BackupManifest` under its defining module. The exact-key lookup
        // (`["BackupManifest"]`) misses; the terminal-segment fallback finds
        // `["schema", "backup", "BackupManifest"]`.
        let pool = pool_from(vec![
            (tp(&["schema", "backup", "BackupManifest"]), "pub struct BackupManifest { pub version: u32 }"),
            (
                tp(&["schema", "backup", "RestoreCandidate"]),
                "pub struct RestoreCandidate { pub manifest: Option<BackupManifest> }",
            ),
        ]);
        let graph = dependency_graph(&pool);
        let deps = graph.get(&tp(&["schema", "backup", "RestoreCandidate"])).expect("RestoreCandidate edges");
        assert!(
            deps.contains(&tp(&["schema", "backup", "BackupManifest"])),
            "expected a terminal-segment-matched edge to the nested BackupManifest key, got {deps:?}"
        );
    }

    #[test]
    fn single_segment_ref_with_no_terminal_match_creates_no_edge() {
        // A bare ident that matches no pool key's terminal segment must not
        // fabricate an edge — `MysteryType` isn't in the pool at all.
        let pool = pool_from(vec![(tp(&["Foo"]), "pub struct Foo { pub x: MysteryType }")]);
        let graph = dependency_graph(&pool);
        assert!(graph.get(&tp(&["Foo"])).unwrap().is_empty());
    }

    #[test]
    fn ambiguous_terminal_with_no_imports_creates_no_edge() {
        // Two modules define `Manifest`. A bare `Manifest` reference with no
        // `use` table to disambiguate must NOT silently pick one — the wrong
        // pick would emit the wrong type's body under the shared TS name.
        let pool = pool_from(vec![
            (tp(&["a", "Manifest"]), "pub struct Manifest { pub v: u32 }"),
            (tp(&["b", "Manifest"]), "pub struct Manifest { pub w: u32 }"),
            (tp(&["c", "RestoreCandidate"]), "pub struct RestoreCandidate { pub m: Manifest }"),
        ]);
        let graph = dependency_graph(&pool);
        let deps = graph.get(&tp(&["c", "RestoreCandidate"])).expect("RestoreCandidate edges");
        assert!(
            deps.is_empty(),
            "ambiguous bare `Manifest` must resolve to no edge without a disambiguating `use`, got {deps:?}"
        );
    }

    #[test]
    fn import_disambiguates_between_same_terminal_types() {
        // Same ambiguous pool as above, but module `c` has an explicit
        // `use crate::b::Manifest;`. The import is authoritative: the edge
        // must point at `b::Manifest`, never `a::Manifest`.
        let pool = pool_from(vec![
            (tp(&["a", "Manifest"]), "pub struct Manifest { pub v: u32 }"),
            (tp(&["b", "Manifest"]), "pub struct Manifest { pub w: u32 }"),
            (tp(&["c", "RestoreCandidate"]), "pub struct RestoreCandidate { pub m: Manifest }"),
        ]);
        let imports = imports_from(&[(&["c"], "use crate::b::Manifest;")]);
        let graph = dependency_graph_with_imports(&pool, &imports);
        let deps = graph.get(&tp(&["c", "RestoreCandidate"])).expect("RestoreCandidate edges");
        assert!(deps.contains(&tp(&["b", "Manifest"])), "expected edge to b::Manifest, got {deps:?}");
        assert!(!deps.contains(&tp(&["a", "Manifest"])), "must not link to a::Manifest, got {deps:?}");
    }

    #[test]
    fn import_to_external_crate_creates_no_edge_and_blocks_fallback() {
        // `c` does `use chrono::Manifest;` (hypothetical external). Even
        // though a pool `Manifest` exists, the import is authoritative and
        // points outside the pool → no edge, and no terminal-fallback guess.
        let pool = pool_from(vec![
            (tp(&["a", "Manifest"]), "pub struct Manifest { pub v: u32 }"),
            (tp(&["c", "Thing"]), "pub struct Thing { pub m: Manifest }"),
        ]);
        let imports = imports_from(&[(&["c"], "use chrono::Manifest;")]);
        let graph = dependency_graph_with_imports(&pool, &imports);
        let deps = graph.get(&tp(&["c", "Thing"])).expect("Thing edges");
        assert!(deps.is_empty(), "external import must block the pool edge, got {deps:?}");
    }

    #[test]
    fn multi_level_reexport_chain_resolves_to_definition_key() {
        // `c` imports `Foo` from a facade module that itself re-exports it
        // from where it's defined:
        //   c:      use crate::facade::Foo;
        //   facade: pub use crate::core::Foo;   (defines no `Foo` of its own)
        //   core:   pub struct Foo { ... }       (the only real definition)
        // The resolver must follow the chain c → facade → core and edge to
        // `core::Foo`.
        let pool = pool_from(vec![
            (tp(&["core", "Foo"]), "pub struct Foo { pub x: u32 }"),
            (tp(&["c", "User"]), "pub struct User { pub f: Foo }"),
        ]);
        let imports = imports_from(&[(&["c"], "use crate::facade::Foo;"), (&["facade"], "pub use crate::core::Foo;")]);
        let graph = dependency_graph_with_imports(&pool, &imports);
        let deps = graph.get(&tp(&["c", "User"])).expect("User edges");
        assert!(deps.contains(&tp(&["core", "Foo"])), "multi-level re-export must resolve to core::Foo, got {deps:?}");
    }

    #[test]
    fn aliased_import_resolves_to_canonical_key() {
        // `use crate::backup::Manifest as Mani;` then a field typed `Mani`.
        // The alias resolves to the canonical pool key.
        let pool = pool_from(vec![
            (tp(&["backup", "Manifest"]), "pub struct Manifest { pub v: u32 }"),
            (tp(&["c", "Thing"]), "pub struct Thing { pub m: Mani }"),
        ]);
        let imports = imports_from(&[(&["c"], "use crate::backup::Manifest as Mani;")]);
        let graph = dependency_graph_with_imports(&pool, &imports);
        let deps = graph.get(&tp(&["c", "Thing"])).expect("Thing edges");
        assert!(deps.contains(&tp(&["backup", "Manifest"])), "alias must resolve to backup::Manifest, got {deps:?}");
    }

    // ── reachable_from ───────────────────────────────────────────────────

    #[test]
    fn reachable_finds_transitive_closure() {
        let pool = pool_from(vec![
            (tp(&["A"]), "pub struct A { pub b: B }"),
            (tp(&["B"]), "pub struct B { pub c: C }"),
            (tp(&["C"]), "pub struct C { pub x: u32 }"),
            (tp(&["Unrelated"]), "pub struct Unrelated { pub x: u32 }"),
        ]);
        let graph = dependency_graph(&pool);
        let reach = reachable_from(&[tp(&["A"])], &graph);
        assert!(reach.contains(&tp(&["A"])));
        assert!(reach.contains(&tp(&["B"])));
        assert!(reach.contains(&tp(&["C"])));
        assert!(!reach.contains(&tp(&["Unrelated"])));
    }

    #[test]
    fn nested_only_type_is_reachable_from_root_via_terminal_match() {
        // Regression guard: the root (`RestoreCandidate`) reaches a
        // nested-only type (`BackupManifest`) solely through a bare-ident
        // field reference. Before the single-segment terminal-segment
        // fallback, the missing graph edge dropped `BackupManifest` from the
        // reachable set — so it was referenced in the emitted TS but never
        // declared (a `TS2304: Cannot find name` at the consumer).
        let pool = pool_from(vec![
            (tp(&["schema", "backup", "BackupManifest"]), "pub struct BackupManifest { pub version: u32 }"),
            (
                tp(&["schema", "backup", "RestoreCandidate"]),
                "pub struct RestoreCandidate { pub manifest: Option<BackupManifest> }",
            ),
        ]);
        let graph = dependency_graph(&pool);
        let reach = reachable_from(&[tp(&["schema", "backup", "RestoreCandidate"])], &graph);
        assert!(
            reach.contains(&tp(&["schema", "backup", "BackupManifest"])),
            "BackupManifest must be reachable from RestoreCandidate so it gets emitted"
        );
    }

    // ── topo_order ───────────────────────────────────────────────────────

    #[test]
    fn topo_order_emits_deps_before_dependents() {
        let pool = pool_from(vec![
            (tp(&["A"]), "pub struct A { pub b: B }"),
            (tp(&["B"]), "pub struct B { pub c: C }"),
            (tp(&["C"]), "pub struct C { pub x: u32 }"),
        ]);
        let graph = dependency_graph(&pool);
        let reach = reachable_from(&[tp(&["A"])], &graph);
        let order = topo_order(&graph, &reach);

        let pos: BTreeMap<_, _> = order.iter().enumerate().map(|(i, t)| (t.clone(), i)).collect();
        // C must precede B; B must precede A.
        assert!(pos[&tp(&["C"])] < pos[&tp(&["B"])]);
        assert!(pos[&tp(&["B"])] < pos[&tp(&["A"])]);
    }

    #[test]
    fn topo_order_breaks_ties_alphabetically_by_canonical_path() {
        let pool = pool_from(vec![
            (tp(&["A"]), "pub struct A { pub x: u32 }"),
            (tp(&["B"]), "pub struct B { pub x: u32 }"),
            (tp(&["C"]), "pub struct C { pub x: u32 }"),
        ]);
        let graph = dependency_graph(&pool);
        let reach: BTreeSet<_> = pool.keys().cloned().collect();
        let order = topo_order(&graph, &reach);
        assert_eq!(order, vec![tp(&["A"]), tp(&["B"]), tp(&["C"])]);
    }

    #[test]
    fn topo_order_handles_cycles_by_appending_remaining() {
        let pool = pool_from(vec![
            (tp(&["Node"]), "pub struct Node { pub child: Vec<Node> }"),
            (tp(&["A"]), "pub struct A { pub b: B }"),
            (tp(&["B"]), "pub struct B { pub a: A }"),
        ]);
        let graph = dependency_graph(&pool);
        let reach: BTreeSet<_> = pool.keys().cloned().collect();
        let order = topo_order(&graph, &reach);
        // All three nodes appear in the output.
        assert_eq!(order.len(), 3);
        assert!(order.contains(&tp(&["A"])));
        assert!(order.contains(&tp(&["B"])));
        assert!(order.contains(&tp(&["Node"])));
    }

    #[test]
    fn topo_order_is_deterministic_across_runs() {
        let pool = pool_from(vec![
            (tp(&["Z"]), "pub struct Z { pub a: A }"),
            (tp(&["A"]), "pub struct A { pub x: u32 }"),
            (tp(&["M"]), "pub struct M { pub a: A }"),
        ]);
        let graph = dependency_graph(&pool);
        let reach: BTreeSet<_> = pool.keys().cloned().collect();
        let order1 = topo_order(&graph, &reach);
        let order2 = topo_order(&graph, &reach);
        assert_eq!(order1, order2);
        // And the order is deterministic across multiple pool constructions
        // (BTreeMap → BTreeMap, no HashMap leakage).
        assert_eq!(order1[0], tp(&["A"])); // A first (no deps).
        // M and Z both depend on A — appear alphabetically.
        assert_eq!(order1[1], tp(&["M"]));
        assert_eq!(order1[2], tp(&["Z"]));
    }
}
