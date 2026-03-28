// TODO: review — replaced with re-export wrapper during ontogen-core extraction
//! Re-exports naming utilities from `ontogen_core`.
//!
//! The canonical definitions live in `ontogen_core::naming`. This module
//! re-exports them so existing `crate::store::helpers::*` imports continue to work.

pub use ontogen_core::naming::*;
