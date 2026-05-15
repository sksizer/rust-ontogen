#![forbid(unsafe_code)]
#![allow(clippy::doc_markdown)]

//! Rust AST → TypeScript emitter for ontogen's long-tail type bindings.
//!
//! This crate is the build-time replacement for the OF-014 spike's `specta`
//! side-car. Given a set of root types, a pool of candidate type definitions
//! (`syn::Item` keyed by canonical [`TypePath`]), and an [`EmitConfig`], it
//! produces TypeScript source covering the supported subset documented in
//! [`OF-015`](https://github.com/sksizer/rust-ontogen/blob/main/docs/tasks/OF-015-productionize-typescript-generation.md):
//!
//! - Named structs over primitive / container / smart-pointer / reference
//!   field types
//! - C-style enums (and tagged enums where the tag is implicit from variant
//!   idents)
//! - `Vec<T>`, `Option<T>`, `HashMap<K, V>`, `BTreeMap<K, V>`
//! - Primitives: `bool`, all integer types, `f32`/`f64`, `String`, `&str`
//! - Smart-pointer wrappers (`Box`, `Rc`, `Arc`, `Cow`, `Pin`) peeled
//!   silently
//! - External types via [`EmitConfig::external_types`]
//!
//! See `docs/tasks/OF-015-productionize-typescript-generation.md` for the
//! full design pass.
//!
//! # PR series state (PR 1 of 8)
//!
//! PR 1 lands the crate scaffold, public API types, and per-type emission
//! (`emit_type`, `emit_struct`, `emit_enum`). The top-level [`emit`] entry
//! point's body is `todo!()`; PRs 2-4 wire it. Serde renames (PR 2), type
//! collection / use-resolution / external-types lookup (PR 3), and
//! `#[ontogen::ts_opaque]` / `#[ontogen::ts_name]` proc-macro attrs (PR 4)
//! are not implemented here.

mod emit;
mod types;

pub use emit::emit;
pub use types::{BigIntBehavior, EmitConfig, EmitError, RenameAll, TypePath, TypePathError};
