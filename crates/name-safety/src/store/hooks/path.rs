//! Lifecycle hooks for Path.
//!
//! This file was scaffolded by ontogen. It is yours to edit.
//! Fill in hook bodies with custom logic (validation, side effects, etc.).
//! This file is NEVER overwritten by the generator.

#![allow(unused_variables, clippy::unnecessary_wraps, clippy::unused_async)]

use crate::schema::{AppError, Path};
use crate::store::Store;
use crate::store::generated::path::PathUpdate;

/// Called before a path is inserted. Modify the entity or return Err to reject.
pub async fn before_create(_store: &Store, _path: &mut Path) -> Result<(), AppError> {
    Ok(())
}

/// Called after a path is successfully created.
pub async fn after_create(_store: &Store, _path: &Path) -> Result<(), AppError> {
    Ok(())
}

/// Called before a path is updated. Receives current state and pending changes.
pub async fn before_update(_store: &Store, _current: &Path, _updates: &PathUpdate) -> Result<(), AppError> {
    Ok(())
}

/// Called after a path is successfully updated.
pub async fn after_update(_store: &Store, _path: &Path) -> Result<(), AppError> {
    Ok(())
}

/// Called before a path is deleted.
pub async fn before_delete(_store: &Store, _id: &str) -> Result<(), AppError> {
    Ok(())
}

/// Called after a path is successfully deleted.
pub async fn after_delete(_store: &Store, _id: &str) -> Result<(), AppError> {
    Ok(())
}
