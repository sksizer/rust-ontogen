//! Lifecycle hooks for MarkdownStore.
//!
//! This file was scaffolded by ontogen. It is yours to edit.
//! Fill in hook bodies with custom logic (validation, side effects, etc.).
//! This file is NEVER overwritten by the generator.

#![allow(unused_variables, clippy::unnecessary_wraps, clippy::unused_async)]

use crate::schema::{AppError, MarkdownStore};
use crate::store::Store;
use crate::store::generated::markdown_store::MarkdownStoreUpdate;

/// Called before a markdown_store is inserted. Modify the entity or return Err to reject.
pub async fn before_create(_store: &Store, _markdown_store: &mut MarkdownStore) -> Result<(), AppError> {
    Ok(())
}

/// Called after a markdown_store is successfully created.
pub async fn after_create(_store: &Store, _markdown_store: &MarkdownStore) -> Result<(), AppError> {
    Ok(())
}

/// Called before a markdown_store is updated. Receives current state and pending changes.
pub async fn before_update(
    _store: &Store,
    _current: &MarkdownStore,
    _updates: &MarkdownStoreUpdate,
) -> Result<(), AppError> {
    Ok(())
}

/// Called after a markdown_store is successfully updated.
pub async fn after_update(_store: &Store, _markdown_store: &MarkdownStore) -> Result<(), AppError> {
    Ok(())
}

/// Called before a markdown_store is deleted.
pub async fn before_delete(_store: &Store, _id: &str) -> Result<(), AppError> {
    Ok(())
}

/// Called after a markdown_store is successfully deleted.
pub async fn after_delete(_store: &Store, _id: &str) -> Result<(), AppError> {
    Ok(())
}
