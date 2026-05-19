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
//! # PR series state (PR 3 of 8)
//!
//! PR 1 landed the crate scaffold, public API types, and per-type emission.
//! PR 2 added the serde rename family (rename / rename_all / skip across
//! containers, fields, variants). PR 3 adds the type-pool walker, per-file
//! `use`-resolution + canonical-path normalization, the external-types
//! table (with shipped defaults + user override merge), and topological
//! ordering over the dependency graph via Kahn's algorithm — every map and
//! set used along the way is a `BTreeMap` / `BTreeSet` so emission order is
//! deterministic by construction.
//!
//! The top-level [`emit`] entry point's body is still `todo!()`; PR 4 wires
//! the pieces together. `#[ontogen::ts_opaque]` / `#[ontogen::ts_name]`
//! proc-macro attrs (PR 4) are not implemented here.

mod attr;
mod emit;
mod external;
mod order;
mod pool;
mod rename;
mod resolve;
mod types;

pub use emit::emit;
pub use types::{BigIntBehavior, EmitConfig, EmitError, RenameAll, TypePath, TypePathError};
