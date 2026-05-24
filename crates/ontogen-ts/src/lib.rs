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
//! # PR series state (PR 4 of 8)
//!
//! PR 1-3 landed the crate scaffold, per-type emission, serde rename
//! family, type-pool walker, use-resolution, external-types table, and
//! topological ordering. PR 4 wires them together: the top-level [`emit`]
//! function now composes collection → name resolution → collision
//! detection → topological ordering → per-type emission → error
//! aggregation, all in one pass. The `#[ts_opaque(target = "...")]` and
//! `#[ts_name = "..."]` proc-macro attrs (shipped in `ontogen-macros`)
//! are read here to short-circuit emission for opaque types and override
//! TS names. External-types lookup is wired into `emit_type`'s
//! fall-through so types like `chrono::DateTime` resolve to `string`
//! per the shipped defaults.
//!
//! PR 5 wires `ontogen` itself to call [`emit`] instead of the side-car
//! emitter.

mod attr;
mod emit;
mod external;
mod order;
mod pool;
mod rename;
mod resolve;
mod types;

pub use emit::{emit, emit_with_imports};
pub use pool::{ScanError, scan_src_dir, scan_src_dir_with_imports};
pub use resolve::ModuleImports;
pub use types::{BigIntBehavior, EmitConfig, EmitError, RenameAll, TypePath, TypePathError};
