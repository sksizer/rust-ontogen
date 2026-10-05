//! Lifecycle hooks for Relationship.
//!
//! This file was scaffolded by ontogen. It is yours to edit.
//! Fill in hook bodies with custom logic (validation, side effects, etc.).
//! This file is NEVER overwritten by the generator.

#![allow(unused_variables, clippy::unnecessary_wraps, clippy::unused_async)]

use crate::schema::{AppError, Relationship};
use crate::store::Store;
use crate::store::generated::relationship::RelationshipUpdate;

/// Called before a relationship is inserted. Modify the entity or return Err to reject.
pub async fn before_create(_store: &Store, _relationship: &mut Relationship) -> Result<(), AppError> {
    Ok(())
}

/// Called after a relationship is successfully created.
pub async fn after_create(_store: &Store, _relationship: &Relationship) -> Result<(), AppError> {
    Ok(())
}

/// Called before a relationship is updated. Receives current state and pending changes.
pub async fn before_update(
    _store: &Store,
    _current: &Relationship,
    _updates: &RelationshipUpdate,
) -> Result<(), AppError> {
    Ok(())
}

/// Called after a relationship is successfully updated.
pub async fn after_update(_store: &Store, _relationship: &Relationship) -> Result<(), AppError> {
    Ok(())
}

/// Called before a relationship is deleted.
pub async fn before_delete(_store: &Store, _id: &str) -> Result<(), AppError> {
    Ok(())
}

/// Called after a relationship is successfully deleted.
pub async fn after_delete(_store: &Store, _id: &str) -> Result<(), AppError> {
    Ok(())
}
