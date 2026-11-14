//! Lifecycle hooks for DistanceFacet.
//!
//! This file was scaffolded by ontogen. It is yours to edit.
//! Fill in hook bodies with custom logic (validation, side effects, etc.).
//! This file is NEVER overwritten by the generator.

#![allow(unused_variables, clippy::unnecessary_wraps, clippy::unused_async)]

use crate::schema::{AppError, DistanceFacet};
use crate::store::Store;
use crate::store::generated::distance_facet::DistanceFacetUpdate;

/// Called before a distance_facet is inserted. Modify the entity or return Err to reject.
pub async fn before_create(_store: &Store, _distance_facet: &mut DistanceFacet) -> Result<(), AppError> {
    Ok(())
}

/// Called after a distance_facet is successfully created.
pub async fn after_create(_store: &Store, _distance_facet: &DistanceFacet) -> Result<(), AppError> {
    Ok(())
}

/// Called before a distance_facet is updated. Receives current state and pending changes.
pub async fn before_update(
    _store: &Store,
    _current: &DistanceFacet,
    _updates: &DistanceFacetUpdate,
) -> Result<(), AppError> {
    Ok(())
}

/// Called after a distance_facet is successfully updated.
pub async fn after_update(_store: &Store, _distance_facet: &DistanceFacet) -> Result<(), AppError> {
    Ok(())
}

/// Called before a distance_facet is deleted.
pub async fn before_delete(_store: &Store, _id: &str) -> Result<(), AppError> {
    Ok(())
}

/// Called after a distance_facet is successfully deleted.
pub async fn after_delete(_store: &Store, _id: &str) -> Result<(), AppError> {
    Ok(())
}
