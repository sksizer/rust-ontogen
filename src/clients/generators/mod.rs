//! Code generators for the client-side TypeScript surface plus the admin registry.
//!
//! Companion to [`crate::servers::generators`], which owns the server-side
//! Rust transport generators (Axum, Tauri IPC, MCP).

pub mod admin;
pub(crate) mod jsonapi;
pub mod transport;
pub mod ts_bindings;
pub mod ts_client;

use std::path::PathBuf;

use crate::clients::config::Config;
use crate::servers::parse::{ApiFn, Param};
use crate::servers::types::{extract_input_type, rust_type_to_ts, strip_ref, ts_param};

/// Derive the IPC/TS command name for any function.
///
/// [`crate::servers::generators::ipc::command_name`] keyed off the
/// client-side [`Config`].
///
/// Uses entity-first naming with singular entity prefix:
///   CRUD:     `{entity}_list`, `{entity}_get_by_id`, `{entity}_create`, etc.
///   Junction: `{entity}_add_role`, `{entity}_list_skills`, etc.
///   Custom:   `{entity}_publish`, `{entity}_refresh`, etc.
///
/// Source-side `#[ontogen(rename = "...")]` (recorded as
/// [`ApiFn::command_override`]) wins over the default scheme.
pub(crate) fn command_name(module: &str, f: &ApiFn, config: &Config) -> String {
    crate::servers::generators::ipc::command_name_in(&config.naming, module, f)
}

/// `f`'s parameters whose Rust types the TypeScript surface names: every one
/// but a `list`'s order, which a client sends as sort keys.
pub(crate) fn typed_params(f: &ApiFn) -> impl Iterator<Item = &Param> {
    f.params.iter().filter(|p| p.order_sort_field().is_none())
}

/// The TypeScript parameter list for a custom fn, in Rust declaration order.
///
/// Every generated surface — the `Transport` interface, its HTTP and IPC
/// impls, and the HTTP-only client — has to agree on positions, and the IPC
/// impl forwards `f.params` verbatim, so declaration order is the only order
/// all four can share. Grouping the `Option<_>` query params ahead of the body
/// ones, as the HTTP-side emitters used to, left a POST with an optional param
/// after a required one uncallable: no Rust signature satisfied every
/// transport at once.
pub(crate) fn ts_params_in_declaration_order(f: &ApiFn) -> Vec<String> {
    f.params
        .iter()
        .map(|p| {
            if p.is_input() {
                format!("input: {}", rust_type_to_ts(&extract_input_type(&p.ty)))
            } else if p.ty.starts_with("Option<") {
                // Type from the Option's inner type: `Option<u64>` → `number |
                // null`, matching what the IPC handler deserializes.
                format!("{}: {}", ts_param(&p.name), rust_type_to_ts(&p.ty))
            } else {
                format!("{}: {}", ts_param(&p.name), rust_type_to_ts(&strip_ref(&p.ty)))
            }
        })
        .collect()
}

/// A type that the TypeScript transport / client emitter could not resolve
/// against the configured `bindings.ts` file and fell back to
/// `Record<string, unknown>` for.
///
/// Post-OF-015 this is a defensive backstop: `ontogen-ts` populates
/// `bindings.ts` with the long-tail closure of types reachable from the
/// configured API root set, and ontogen-ts itself hard-errors on
/// unsupported shapes. In the happy path the transport / client emitter
/// finds every type it needs in `bindings.ts` and produces zero records.
/// The path still fires when: `bindings.ts` is hand-edited, ontogen-ts's
/// root-set derivation doesn't reach a type the transport surface
/// references (e.g. types pulled in by signature-only metadata), or the
/// build observes a stale `bindings.ts` mid-pipeline.
///
/// The TS surface still compiles cleanly when this happens but loses type
/// safety on the affected calls. Callers (notably
/// [`crate::clients::generate`]) drain these records and emit one
/// `cargo:warning=` per occurrence so the silent untyping surfaces at
/// build time.
#[derive(Debug, Clone)]
pub struct FallbackRecord {
    /// Output `.ts` file the placeholder type was emitted into.
    pub output: PathBuf,
    /// `bindings.ts` file the emitter consulted.
    pub bindings_path: PathBuf,
    /// The type name that fell back to `Record<string, unknown>`.
    pub type_name: String,
}

impl std::fmt::Display for FallbackRecord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let bindings = self.bindings_path.display();
        let output = self.output.display();
        write!(
            f,
            "ontogen: type '{}' not found in `{bindings}` - using `Record<string, unknown>` placeholder in `{output}`",
            self.type_name,
        )
    }
}
