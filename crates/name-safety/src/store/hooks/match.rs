//! Lifecycle hooks for Match.
//!
//! This file was scaffolded by ontogen. It is yours to edit.
//! Fill in hook bodies with custom logic (validation, side effects, etc.).
//! This file is NEVER overwritten by the generator.

#![allow(unused_variables, clippy::unnecessary_wraps, clippy::unused_async)]

use crate::schema::{Match, AppError};
use crate::store::Store;
use crate::store::generated::match::MatchUpdate;

/// Called before a match is inserted. Modify the entity or return Err to reject.
pub async fn before_create(_store: &Store, _match: &mut Match) -> Result<(), AppError> {
    Ok(())
}

/// Called after a match is successfully created.
pub async fn after_create(_store: &Store, _match: &Match) -> Result<(), AppError> {
    Ok(())
}

/// Called before a match is updated. Receives current state and pending changes.
pub async fn before_update(
    _store: &Store,
    _current: &Match,
    _updates: &MatchUpdate,
) -> Result<(), AppError> {
    Ok(())
}

/// Called after a match is successfully updated.
pub async fn after_update(_store: &Store, _match: &Match) -> Result<(), AppError> {
    Ok(())
}

/// Called before a match is deleted.
pub async fn before_delete(_store: &Store, _id: &str) -> Result<(), AppError> {
    Ok(())
}

/// Called after a match is successfully deleted.
pub async fn after_delete(_store: &Store, _id: &str) -> Result<(), AppError> {
    Ok(())
}
