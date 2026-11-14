//! Lifecycle hooks for LiftingFacet.
//!
//! This file was scaffolded by ontogen. It is yours to edit.
//! Fill in hook bodies with custom logic (validation, side effects, etc.).
//! This file is NEVER overwritten by the generator.

#![allow(unused_variables, clippy::unnecessary_wraps, clippy::unused_async)]

use crate::schema::{AppError, LiftingFacet};
use crate::store::Store;
use crate::store::generated::lifting_facet::LiftingFacetUpdate;

/// Called before a lifting_facet is inserted. Modify the entity or return Err to reject.
pub async fn before_create(_store: &Store, _lifting_facet: &mut LiftingFacet) -> Result<(), AppError> {
    Ok(())
}

/// Called after a lifting_facet is successfully created.
pub async fn after_create(_store: &Store, _lifting_facet: &LiftingFacet) -> Result<(), AppError> {
    Ok(())
}

/// Called before a lifting_facet is updated. Receives current state and pending changes.
pub async fn before_update(
    _store: &Store,
    _current: &LiftingFacet,
    _updates: &LiftingFacetUpdate,
) -> Result<(), AppError> {
    Ok(())
}

/// Called after a lifting_facet is successfully updated.
pub async fn after_update(_store: &Store, _lifting_facet: &LiftingFacet) -> Result<(), AppError> {
    Ok(())
}

/// Called before a lifting_facet is deleted.
pub async fn before_delete(_store: &Store, _id: &str) -> Result<(), AppError> {
    Ok(())
}

/// Called after a lifting_facet is successfully deleted.
pub async fn after_delete(_store: &Store, _id: &str) -> Result<(), AppError> {
    Ok(())
}
