//! Lifecycle hooks for State.
//!
//! This file was scaffolded by ontogen. It is yours to edit.
//! Fill in hook bodies with custom logic (validation, side effects, etc.).
//! This file is NEVER overwritten by the generator.

#![allow(unused_variables, clippy::unnecessary_wraps, clippy::unused_async)]

use crate::schema::{AppError, State};
use crate::store::Store;
use crate::store::generated::state::StateUpdate;

/// Called before a state is inserted. Modify the entity or return Err to reject.
pub async fn before_create(_store: &Store, _state: &mut State) -> Result<(), AppError> {
    Ok(())
}

/// Called after a state is successfully created.
pub async fn after_create(_store: &Store, _state: &State) -> Result<(), AppError> {
    Ok(())
}

/// Called before a state is updated. Receives current state and pending changes.
pub async fn before_update(_store: &Store, _current: &State, _updates: &StateUpdate) -> Result<(), AppError> {
    Ok(())
}

/// Called after a state is successfully updated.
pub async fn after_update(_store: &Store, _state: &State) -> Result<(), AppError> {
    Ok(())
}

/// Called before a state is deleted.
pub async fn before_delete(_store: &Store, _id: &str) -> Result<(), AppError> {
    Ok(())
}

/// Called after a state is successfully deleted.
pub async fn after_delete(_store: &Store, _id: &str) -> Result<(), AppError> {
    Ok(())
}
