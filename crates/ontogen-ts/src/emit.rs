//! Per-type and top-level emission entry points.
//!
//! PR 1 lands the per-type emission machinery (`emit_type`, `emit_struct`,
//! `emit_enum`) for the phase-1 supported subset. The top-level [`emit`]
//! entry point's body is a `todo!()` stub — PR 4 wires type collection,
//! validation, and ordering through it.

use std::collections::BTreeMap;

use crate::types::{EmitConfig, EmitError, TypePath};

/// Emit TypeScript source for `roots` and everything they transitively reach
/// in `type_pool`, honoring `config`.
///
/// PR 1 leaves the body as `todo!()` — the full composition (collection →
/// validation → ordering → emission → aggregation) lands in PR 4 (AC-8).
/// The signature is fixed today so downstream consumers (and the rest of
/// this PR series) compile against a stable shape.
pub fn emit(
    _roots: &[TypePath],
    _type_pool: &BTreeMap<TypePath, syn::Item>,
    _config: &EmitConfig,
) -> Result<String, Vec<EmitError>> {
    todo!("PR 4 implements the top-level emit composition (OF-015 AC-8)")
}
