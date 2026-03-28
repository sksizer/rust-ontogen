#![allow(clippy::doc_markdown)]

//! Shared types and utilities for the ontogen code generation pipeline.
//!
//! This crate provides:
//! - Entity schema model types (`EntityDef`, `FieldDef`, etc.)
//! - Intermediate representation types that flow between generators
//! - Naming utilities (`to_snake_case`, `to_pascal_case`, `pluralize`)
//! - Build-time utilities (`rustfmt`, `prettier`, `clean_generated_dir`)
//! - The shared `CodegenError` type

pub mod ir;
pub mod model;
pub mod naming;
pub mod utils;

// Re-export key types at the crate root for ergonomic use
pub use ir::*;
pub use model::{EntityDef, FieldDef, FieldRole, FieldType, RelationInfo, RelationKind};
pub use naming::{pluralize, to_pascal_case, to_snake_case};
pub use utils::{clean_generated_dir, emit_rerun_directives, prettier, rustfmt};

// ── Error type ──────────────────────────────────────────────────────

/// Codegen error with layer context.
#[derive(Debug)]
pub enum CodegenError {
    Schema(String),
    Persistence(String),
    Store(String),
    Api(String),
    Server(String),
    Client(String),
}

impl std::fmt::Display for CodegenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Schema(e) => write!(f, "schema codegen error: {e}"),
            Self::Persistence(e) => write!(f, "persistence codegen error: {e}"),
            Self::Store(e) => write!(f, "store codegen error: {e}"),
            Self::Api(e) => write!(f, "api codegen error: {e}"),
            Self::Server(e) => write!(f, "server codegen error: {e}"),
            Self::Client(e) => write!(f, "client codegen error: {e}"),
        }
    }
}

impl std::error::Error for CodegenError {}
