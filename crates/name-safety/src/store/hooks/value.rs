//! Lifecycle hooks for Value.
//!
//! This file was scaffolded by ontogen. It is yours to edit.
//! Fill in hook bodies with custom logic (validation, side effects, etc.).
//! This file is NEVER overwritten by the generator.

#![allow(unused_variables, clippy::unnecessary_wraps, clippy::unused_async)]

use crate::schema::{AppError, Value};
use crate::store::Store;
use crate::store::generated::value::ValueUpdate;

/// Called before a value is inserted. Modify the entity or return Err to reject.
pub async fn before_create(_store: &Store, _value: &mut Value) -> Result<(), AppError> {
    Ok(())
}

/// Called after a value is successfully created.
pub async fn after_create(_store: &Store, _value: &Value) -> Result<(), AppError> {
    Ok(())
}

/// Called before a value is updated. Receives current state and pending changes.
pub async fn before_update(_store: &Store, _current: &Value, _updates: &ValueUpdate) -> Result<(), AppError> {
    Ok(())
}

/// Called after a value is successfully updated.
pub async fn after_update(_store: &Store, _value: &Value) -> Result<(), AppError> {
    Ok(())
}

/// Called before a value is deleted.
pub async fn before_delete(_store: &Store, _id: &str) -> Result<(), AppError> {
    Ok(())
}

/// Called after a value is successfully deleted.
pub async fn after_delete(_store: &Store, _id: &str) -> Result<(), AppError> {
    Ok(())
}
