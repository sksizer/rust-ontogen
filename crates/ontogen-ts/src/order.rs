//! Dependency graph + topological ordering for the type pool.
//!
//! Given a pool keyed by canonical [`TypePath`], extract each item's
//! same-pool references and produce a deterministic topological order so
//! emitted TypeScript declares types before they're referenced (where the
//! pool's transitive shape allows — cycles are co-emitted as a group at
//! the cycle's topo level, since TS type aliases accept forward references
//! freely).
//!
//! Edge-extraction strategy (phase 1, intentionally simple):
//!
//! - For each pool item, recursively walk its fields/variants via
//!   [`syn::visit::Visit`].
//! - For each `syn::Type::Path` encountered, drop generic args, strip a
//!   leading `crate::` segment, and synthesize a candidate [`TypePath`]
//!   from the remaining segments.
//! - Filter candidates against pool keys — only edges to types we already
//!   know about become part of the graph. Anything else (primitives,
//!   external types, unresolved one-segment idents) is silently ignored
//!   here; the per-type emitter handles those at render time.
//!
//! This is a deliberately conservative dep extractor: any one-segment
//! ident in a user file that's actually resolved via `use foo::Bar` won't
//! be recognized as a dep on `foo::Bar` without consulting that file's
//! imports table. PR 4 will tighten this when it composes pool + resolve
//! into the top-level `emit()`. For now, missing edges in the order graph
//! degrade readability (a forward TS reference) but not correctness — TS
//! type aliases accept forward references.

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
                if let Ok(path) = TypePath::new(canonical)
                    && self.pool.contains_key(&path)
                {
                    self.deps.insert(path);
                }
                // Also try the un-prefixed form for single-segment idents
                // — covers `use crate::models::Workout` then ref'd as
                // `Workout`. If the pool has `["Workout"]` it's already
                // matched; if pool has `["models", "Workout"]` we miss
                // this edge, see module docs.
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
