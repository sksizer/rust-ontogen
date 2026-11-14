//! Lifecycle hooks for ActivityKind.
//!
//! This file was scaffolded by ontogen. It is yours to edit.
//! Fill in hook bodies with custom logic (validation, side effects, etc.).
//! This file is NEVER overwritten by the generator.

#![allow(unused_variables, clippy::unnecessary_wraps, clippy::unused_async)]

use crate::schema::{ActivityKind, AppError};
use crate::store::Store;
use crate::store::generated::activity_kind::ActivityKindUpdate;

/// Called before a activity_kind is inserted. Modify the entity or return Err to reject.
pub async fn before_create(_store: &Store, _activity_kind: &mut ActivityKind) -> Result<(), AppError> {
    Ok(())
}

/// Called after a activity_kind is successfully created.
pub async fn after_create(_store: &Store, _activity_kind: &ActivityKind) -> Result<(), AppError> {
    Ok(())
}

/// Called before a activity_kind is updated. Receives current state and pending changes.
pub async fn before_update(
    _store: &Store,
    _current: &ActivityKind,
    _updates: &ActivityKindUpdate,
) -> Result<(), AppError> {
    Ok(())
}

/// Called after a activity_kind is successfully updated.
pub async fn after_update(_store: &Store, _activity_kind: &ActivityKind) -> Result<(), AppError> {
    Ok(())
}

/// Called before a activity_kind is deleted.
pub async fn before_delete(_store: &Store, _id: &str) -> Result<(), AppError> {
    Ok(())
}

/// Called after a activity_kind is successfully deleted.
pub async fn after_delete(_store: &Store, _id: &str) -> Result<(), AppError> {
    Ok(())
}
