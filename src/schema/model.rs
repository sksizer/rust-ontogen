// TODO: review — replaced with re-export wrapper during ontogen-core extraction
//! Re-exports entity model types from `ontogen_core`.
//!
//! The canonical definitions live in `ontogen_core::model`. This module
//! re-exports them so existing `crate::schema::model::*` imports continue to work.

pub use ontogen_core::model::*;
