//! Lifecycle hooks for SeaQuery.
//!
//! This file was scaffolded by ontogen. It is yours to edit.
//! Fill in hook bodies with custom logic (validation, side effects, etc.).
//! This file is NEVER overwritten by the generator.

#![allow(unused_variables, clippy::unnecessary_wraps, clippy::unused_async)]

use crate::schema::{AppError, SeaQuery};
use crate::store::Store;
use crate::store::generated::sea_query::SeaQueryUpdate;

/// Called before a sea_query is inserted. Modify the entity or return Err to reject.
pub async fn before_create(_store: &Store, _sea_query: &mut SeaQuery) -> Result<(), AppError> {
    Ok(())
}

/// Called after a sea_query is successfully created.
pub async fn after_create(_store: &Store, _sea_query: &SeaQuery) -> Result<(), AppError> {
    Ok(())
}

/// Called before a sea_query is updated. Receives current state and pending changes.
pub async fn before_update(_store: &Store, _current: &SeaQuery, _updates: &SeaQueryUpdate) -> Result<(), AppError> {
    Ok(())
}

/// Called after a sea_query is successfully updated.
pub async fn after_update(_store: &Store, _sea_query: &SeaQuery) -> Result<(), AppError> {
    Ok(())
}

/// Called before a sea_query is deleted.
pub async fn before_delete(_store: &Store, _id: &str) -> Result<(), AppError> {
    Ok(())
}

/// Called after a sea_query is successfully deleted.
pub async fn after_delete(_store: &Store, _id: &str) -> Result<(), AppError> {
    Ok(())
}
