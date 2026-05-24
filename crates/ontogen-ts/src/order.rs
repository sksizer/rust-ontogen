//! Dependency graph + topological ordering for the type pool.
//!
//! Given a pool keyed by canonical [`TypePath`], extract each item's
//! same-pool references and produce a deterministic topological order so
//! emitted TypeScript declares types before they're referenced (where the
//! pool's transitive shape allows — cycles are co-emitted as a group at
//! the cycle's topo level, since TS type aliases accept forward references
//! freely).
//!
//! Edge-extraction strategy (phase 1):
//!
//! - For each pool item, recursively walk its fields/variants via
//!   [`syn::visit::Visit`].
//! - For each `syn::Type::Path` encountered, drop generic args, strip a
//!   leading `crate::` segment, and synthesize a candidate [`TypePath`]
//!   from the remaining segments.
//! - Match candidates against pool keys. A multi-segment candidate matches
//!   only on an exact pool key. A single-segment candidate (`BackupManifest`,
//!   typically brought in via `use`) matches an exact top-level key first,
//!   then falls back to any pool entry whose terminal segment matches — the
//!   same resolution the long-tail *root* resolver uses in
//!   `src/clients/mod.rs`. Anything still unmatched (primitives, external
//!   types) is ignored here; the per-type emitter handles those at render
//!   time.
//!
//! The single-segment fallback matters for **correctness**, not just
//! ordering: [`reachable_from`] walks this same graph to decide *which*
//! types get emitted. A nested-only type — one never named directly in an
//! API signature, only reached through a sibling field by bare ident — would
//! otherwise be dropped from the closure entirely and emitted as an
//! undefined reference. (Earlier phase-1 revisions resolved only exact pool
//! keys here, on the assumption that a missing edge merely produced a
//! forward TS reference; that assumption held for ordering but not for the
//! reachable set.)

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use syn::visit::{self, Visit};

use crate::types::TypePath;

/// Build the dependency graph from a pool. Each node is a pool key; each
/// edge `a → b` means item `a` references item `b` in its fields/variants.
///
/// The graph is restricted to in-pool edges only: an item that references
/// an external type (e.g. `chrono::DateTime`) or an unresolved
/// one-segment ident doesn't get an edge to it.
pub(crate) fn dependency_graph(pool: &BTreeMap<TypePath, syn::Item>) -> BTreeMap<TypePath, BTreeSet<TypePath>> {
    let mut graph: BTreeMap<TypePath, BTreeSet<TypePath>> = BTreeMap::new();
    for (key, item) in pool {
        let mut visitor = DepCollector::new(pool);
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

/// `syn::visit::Visit` that records every in-pool type referenced by an item.
struct DepCollector<'a> {
    pool: &'a BTreeMap<TypePath, syn::Item>,
    deps: BTreeSet<TypePath>,
}

impl<'a> DepCollector<'a> {
    fn new(pool: &'a BTreeMap<TypePath, syn::Item>) -> Self {
        Self { pool, deps: BTreeSet::new() }
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
                        self.deps.insert(path);
                    } else if canonical.len() == 1 {
                        // Single-segment reference (`BackupManifest`) that
                        // isn't itself a top-level pool key. The referenced
                        // type may be defined in a nested module — pool key
                        // `["schema", "backup", "BackupManifest"]` — and reach
                        // this field via a `use`. Fall back to terminal-segment
                        // matching: the same resolution the long-tail *root*
                        // resolver already uses (`src/clients/mod.rs`).
                        //
                        // This edge matters for correctness, not just ordering:
                        // `reachable_from` walks this same graph to decide
                        // *which* types get emitted. A missing edge to a
                        // nested-only type (one never named directly in an API
                        // signature, only reached through a sibling field) drops
                        // it from the closure entirely, so it's referenced in
                        // the emitted output but never declared.
                        let ident = canonical[0].as_str();
                        if let Some(matched) = self.pool.keys().find(|p| p.terminal() == ident).cloned() {
                            self.deps.insert(matched);
                        }
                    }
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
