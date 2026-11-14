//! Lifecycle hooks for ExerciseFamily.
//!
//! This file was scaffolded by ontogen. It is yours to edit.
//! Fill in hook bodies with custom logic (validation, side effects, etc.).
//! This file is NEVER overwritten by the generator.

#![allow(unused_variables, clippy::unnecessary_wraps, clippy::unused_async)]

use crate::schema::{AppError, ExerciseFamily};
use crate::store::Store;
use crate::store::generated::exercise_family::ExerciseFamilyUpdate;

/// Called before a exercise_family is inserted. Modify the entity or return Err to reject.
pub async fn before_create(_store: &Store, _exercise_family: &mut ExerciseFamily) -> Result<(), AppError> {
    Ok(())
}

/// Called after a exercise_family is successfully created.
pub async fn after_create(_store: &Store, _exercise_family: &ExerciseFamily) -> Result<(), AppError> {
    Ok(())
}

/// Called before a exercise_family is updated. Receives current state and pending changes.
pub async fn before_update(
    _store: &Store,
    _current: &ExerciseFamily,
    _updates: &ExerciseFamilyUpdate,
) -> Result<(), AppError> {
    Ok(())
}

/// Called after a exercise_family is successfully updated.
pub async fn after_update(_store: &Store, _exercise_family: &ExerciseFamily) -> Result<(), AppError> {
    Ok(())
}

/// Called before a exercise_family is deleted.
pub async fn before_delete(_store: &Store, _id: &str) -> Result<(), AppError> {
    Ok(())
}

/// Called after a exercise_family is successfully deleted.
pub async fn after_delete(_store: &Store, _id: &str) -> Result<(), AppError> {
    Ok(())
}
