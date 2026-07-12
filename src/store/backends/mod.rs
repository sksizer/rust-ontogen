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

pub(crate) mod seaorm;

use crate::schema::model::EntityDef;

/// One persistence backend's contribution to a generated store module.
pub(crate) trait StoreBackend {
    /// Emit the backend-specific file preamble: the persistence-layer `use`
    /// lines (and nothing else — shared imports are the orchestrator's).
    fn emit_preamble(&self, code: &mut String, entity: &EntityDef);

    /// Emit the complete `impl Store { ... }` block: CRUD methods, relation
    /// population, and any backend-specific helpers (e.g. `set_*_parent`).
    /// Method names, signatures, hook call sites, and `emit_change` points
    /// must match across backends — that contract is enforced by the
    /// backend-parity test, not by this trait.
    fn emit_crud_impl(&self, code: &mut String, entity: &EntityDef);
}

/// Resolve the emitter for a configured [`crate::ir::Backend`].
///
/// The markdown arm is wired but deliberately unimplemented until the
/// emitter PR of the ADR-0001 campaign lands — failing here, before any
/// files are written, beats emitting half a backend.
pub(crate) fn for_backend(backend: &crate::ir::Backend) -> Result<&'static dyn StoreBackend, crate::CodegenError> {
    match backend {
        crate::ir::Backend::Seaorm(_) => Ok(&seaorm::SeaormBackend),
        crate::ir::Backend::Markdown(_) => Err(crate::CodegenError::Store(
            "Backend::Markdown is wired but its CRUD emitter has not landed yet \
             (ADR-0001 campaign, markdown-codegen PR); use Backend::Seaorm meanwhile"
                .into(),
        )),
    }
}
