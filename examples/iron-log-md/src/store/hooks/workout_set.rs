//! Lifecycle hooks for WorkoutSet.
//!
//! This file was scaffolded by ontogen. It is yours to edit.
//! Fill in hook bodies with custom logic (validation, side effects, etc.).
//! This file is NEVER overwritten by the generator.

#![allow(unused_variables, clippy::unnecessary_wraps, clippy::unused_async)]

use crate::schema::{AppError, WorkoutSet};
use crate::store::Store;
use crate::store::generated::workout_set::WorkoutSetUpdate;

/// Called before a workout_set is inserted. Modify the entity or return Err to reject.
pub async fn before_create(_store: &Store, _workout_set: &mut WorkoutSet) -> Result<(), AppError> {
    Ok(())
}

/// Called after a workout_set is successfully created.
pub async fn after_create(_store: &Store, _workout_set: &WorkoutSet) -> Result<(), AppError> {
    Ok(())
}

/// Called before a workout_set is updated. Receives current state and pending changes.
pub async fn before_update(_store: &Store, _current: &WorkoutSet, _updates: &WorkoutSetUpdate) -> Result<(), AppError> {
    Ok(())
}

/// Called after a workout_set is successfully updated.
pub async fn after_update(_store: &Store, _workout_set: &WorkoutSet) -> Result<(), AppError> {
    Ok(())
}

/// Called before a workout_set is deleted.
pub async fn before_delete(_store: &Store, _id: &str) -> Result<(), AppError> {
    Ok(())
}

/// Called after a workout_set is successfully deleted.
pub async fn after_delete(_store: &Store, _id: &str) -> Result<(), AppError> {
    Ok(())
}
