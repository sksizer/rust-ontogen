//! Code generators for multiple transport protocols.

pub mod admin;
pub mod http;
pub mod ipc;
pub mod mcp;
pub mod transport;
pub mod ts_bindings;
pub mod ts_client;

use std::path::PathBuf;

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
/// safety on the affected calls. Callers (notably `generate_transport` in
/// `src/servers/mod.rs`) drain these records and emit one `cargo:warning=`
/// per occurrence so the silent untyping surfaces at build time.
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
