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

/// The backend the generator is currently wired to.
///
/// Interim: hardcoded to SeaORM until `StoreConfig` carries the `Backend`
/// discriminant (the cutover PR), at which point this becomes a match on
/// that enum.
pub(crate) fn active() -> &'static dyn StoreBackend {
    &seaorm::SeaormBackend
}
