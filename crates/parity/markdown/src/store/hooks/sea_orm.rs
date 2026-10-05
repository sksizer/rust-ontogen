//! Lifecycle hooks for SeaOrm.
//!
//! This file was scaffolded by ontogen. It is yours to edit.
//! Fill in hook bodies with custom logic (validation, side effects, etc.).
//! This file is NEVER overwritten by the generator.

#![allow(unused_variables, clippy::unnecessary_wraps, clippy::unused_async)]

use crate::schema::{AppError, SeaOrm};
use crate::store::Store;
use crate::store::generated::sea_orm::SeaOrmUpdate;

/// Called before a sea_orm is inserted. Modify the entity or return Err to reject.
pub async fn before_create(_store: &Store, _sea_orm: &mut SeaOrm) -> Result<(), AppError> {
    Ok(())
}

/// Called after a sea_orm is successfully created.
pub async fn after_create(_store: &Store, _sea_orm: &SeaOrm) -> Result<(), AppError> {
    Ok(())
}

/// Called before a sea_orm is updated. Receives current state and pending changes.
pub async fn before_update(_store: &Store, _current: &SeaOrm, _updates: &SeaOrmUpdate) -> Result<(), AppError> {
    Ok(())
}

/// Called after a sea_orm is successfully updated.
pub async fn after_update(_store: &Store, _sea_orm: &SeaOrm) -> Result<(), AppError> {
    Ok(())
}

/// Called before a sea_orm is deleted.
pub async fn before_delete(_store: &Store, _id: &str) -> Result<(), AppError> {
    Ok(())
}

/// Called after a sea_orm is successfully deleted.
pub async fn after_delete(_store: &Store, _id: &str) -> Result<(), AppError> {
    Ok(())
}
