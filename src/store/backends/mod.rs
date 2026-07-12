//! The store generator's backend seam (ADR 0001).
//!
//! `gen_store` emits one CRUD module per entity; everything persistence-
//! specific in that module — the imports of the persistence layer and the
//! bodies of the CRUD methods — comes from a [`StoreBackend`]. Everything
//! else (the shared schema/hooks/Store imports, the `{Entity}Update` struct
//! and DTO `From` impls, hook scaffolding, `mod.rs` orchestration, and
//! crucially the `StoreMethodMeta` collection that downstream `gen_api`
//! consumes) is backend-agnostic and lives outside this module. That split
//! is what makes the ADR's byte-identical-downstream invariant mechanical:
//! a backend can only vary what a backend is allowed to vary.
//!
//! The seam is deliberately `pub(crate)`: ADR 0001 (alternative C) rejects
//! an out-of-tree backend trait because this internal emit contract is not
//! stable enough to promise anything about. Backends are added by upstream
//! PR, not by implementing a public trait.

pub(crate) mod markdown;
pub(crate) mod seaorm;

use crate::schema::model::EntityDef;

/// One persistence backend's contribution to a generated store module.
pub(crate) trait StoreBackend {
    /// Validate the schema against backend constraints before anything is
    /// written (e.g. the markdown slug strategy requires its source field on
    /// every entity). Default: no constraints.
    fn validate(&self, entities: &[EntityDef]) -> Result<(), String> {
        let _ = entities;
        Ok(())
    }

    /// Emit the backend-specific file preamble: the persistence-layer `use`
    /// lines (and nothing else — shared imports are the orchestrator's).
    fn emit_preamble(&self, code: &mut String, entity: &EntityDef);

    /// Emit backend-specific module-level declarations (after the shared
    /// imports, before the Update struct) — e.g. the markdown backend's
    /// entity-directory constant. Default: nothing.
    fn emit_declarations(&self, code: &mut String, entity: &EntityDef) {
        let _ = (code, entity);
    }

    /// Emit the complete `impl Store { ... }` block: CRUD methods, relation
    /// population, and any backend-specific helpers (e.g. `set_*_parent`).
    /// Method names, signatures, hook call sites, and `emit_change` points
    /// must match across backends — that contract is enforced by the
    /// backend-parity test, not by this trait.
    fn emit_crud_impl(&self, code: &mut String, entity: &EntityDef);

    /// How the shared DTO `From` impls treat wikilink-shaped relation ids.
    /// Wikilinks are a markdown-vault concern: that backend strips `[[id]]`
    /// down to `id` at its typed boundary, while SQL-backed stores pass
    /// relation ids through untouched (they never carried brackets).
    fn wikilink_policy(&self) -> WikilinkPolicy;
}

/// Whether DTO `From` impls strip wikilink syntax from relation id fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WikilinkPolicy {
    /// Strip `[[id]]` → `id` on every relation field (markdown backend).
    Strip,
    /// Pass relation ids through untouched (SQL backends).
    Passthrough,
}

/// Resolve the emitter for a configured [`crate::ir::Backend`].
pub(crate) fn for_backend(backend: &crate::ir::Backend) -> Result<Box<dyn StoreBackend>, crate::CodegenError> {
    match backend {
        crate::ir::Backend::Seaorm(_) => Ok(Box::new(seaorm::SeaormBackend)),
        crate::ir::Backend::Markdown(md) => Ok(Box::new(markdown::MarkdownBackend { md: md.clone() })),
    }
}
