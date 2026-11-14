//! Lifecycle hooks for ExerciseMuscle.
//!
//! This file was scaffolded by ontogen. It is yours to edit.
//! Fill in hook bodies with custom logic (validation, side effects, etc.).
//! This file is NEVER overwritten by the generator.

#![allow(unused_variables, clippy::unnecessary_wraps, clippy::unused_async)]

use crate::schema::{AppError, ExerciseMuscle};
use crate::store::Store;
use crate::store::generated::exercise_muscle::ExerciseMuscleUpdate;

/// Called before a exercise_muscle is inserted. Modify the entity or return Err to reject.
pub async fn before_create(_store: &Store, _exercise_muscle: &mut ExerciseMuscle) -> Result<(), AppError> {
    Ok(())
}

/// Called after a exercise_muscle is successfully created.
pub async fn after_create(_store: &Store, _exercise_muscle: &ExerciseMuscle) -> Result<(), AppError> {
    Ok(())
}

/// Called before a exercise_muscle is updated. Receives current state and pending changes.
pub async fn before_update(
    _store: &Store,
    _current: &ExerciseMuscle,
    _updates: &ExerciseMuscleUpdate,
) -> Result<(), AppError> {
    Ok(())
}

/// Called after a exercise_muscle is successfully updated.
pub async fn after_update(_store: &Store, _exercise_muscle: &ExerciseMuscle) -> Result<(), AppError> {
    Ok(())
}

/// Called before a exercise_muscle is deleted.
pub async fn before_delete(_store: &Store, _id: &str) -> Result<(), AppError> {
    Ok(())
}

/// Called after a exercise_muscle is successfully deleted.
pub async fn after_delete(_store: &Store, _id: &str) -> Result<(), AppError> {
    Ok(())
}
