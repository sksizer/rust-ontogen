//! Lifecycle hooks for SetGroup.
//!
//! This file was scaffolded by ontogen. It is yours to edit.
//! Fill in hook bodies with custom logic (validation, side effects, etc.).
//! This file is NEVER overwritten by the generator.

#![allow(unused_variables, clippy::unnecessary_wraps, clippy::unused_async)]

use crate::schema::{AppError, SetGroup};
use crate::store::Store;
use crate::store::generated::set_group::SetGroupUpdate;

/// Called before a set_group is inserted. Modify the entity or return Err to reject.
pub async fn before_create(_store: &Store, _set_group: &mut SetGroup) -> Result<(), AppError> {
    Ok(())
}

/// Called after a set_group is successfully created.
pub async fn after_create(_store: &Store, _set_group: &SetGroup) -> Result<(), AppError> {
    Ok(())
}

/// Called before a set_group is updated. Receives current state and pending changes.
pub async fn before_update(_store: &Store, _current: &SetGroup, _updates: &SetGroupUpdate) -> Result<(), AppError> {
    Ok(())
}

/// Called after a set_group is successfully updated.
pub async fn after_update(_store: &Store, _set_group: &SetGroup) -> Result<(), AppError> {
    Ok(())
}

/// Called before a set_group is deleted.
pub async fn before_delete(_store: &Store, _id: &str) -> Result<(), AppError> {
    Ok(())
}

/// Called after a set_group is successfully deleted.
pub async fn after_delete(_store: &Store, _id: &str) -> Result<(), AppError> {
    Ok(())
}
