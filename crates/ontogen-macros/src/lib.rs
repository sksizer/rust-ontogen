//! No-op proc-macros that make ontogen attributes legal Rust.
//!
//! All actual interpretation of these attributes happens in `build.rs` via `syn`.
//! This crate never contains logic - it just passes the annotated item through
//! unchanged (modulo stripping the ontogen-specific attribute itself, where
//! the macro is the attribute).

#![forbid(unsafe_code)]

use proc_macro::TokenStream;

/// Derive macro that does nothing except make `#[ontology(...)]` attributes
/// legal on both the struct and its fields.
///
/// Usage:
/// ```ignore
/// #[derive(OntologyEntity)]
/// #[ontology(entity, directory = "nodes", table = "nodes")]
/// pub struct Node {
///     #[ontology(id)]
///     pub id: String,
///     #[ontology(relation(contains, target = "Node"))]
///     pub contains: Vec<String>,
/// }
/// ```
///
/// The derive expands to nothing. `build.rs` reads the source file with `syn`
/// and interprets the `#[ontology(...)]` attributes for codegen.
#[proc_macro_derive(OntologyEntity, attributes(ontology))]
pub fn derive_ontology_entity(_input: TokenStream) -> TokenStream {
    TokenStream::new()
}

/// Attribute macro that marks a `pub fn` in an API module as stateless, i.e.
/// not threading the configured state/store type as its first parameter.
///
/// The attribute itself expands to a no-op pass-through of the annotated
/// item; the ontogen parser (`servers::parse`) reads the attribute via `syn`
/// during build-time scanning and emits a handler shape that omits the
/// state/store extractor.
///
/// Usage:
/// ```ignore
/// #[ontogen::stateless]
/// pub fn copy(text: &str) -> Result<(), AppError> {
///     pumice_desktop::clipboard::copy_text(text.to_string()).map_err(AppError::DbError)
/// }
/// ```
///
/// Without this marker, a `pub fn` whose first parameter is not the
/// configured state or store type is dropped from the generated transports
/// (with a `cargo:warning=` per skip).
#[proc_macro_attribute]
pub fn stateless(_attr: TokenStream, item: TokenStream) -> TokenStream {
    item
}

/// Attribute macro that forces an annotated `pub fn` to classify as
/// `OpKind::CustomPost`, overriding the auto-classifier.
///
/// Canonical user-facing path is `#[ontogen::http::post]` — the `http`
/// namespace exists in the consumer-side `ontogen` crate (which
/// re-exports `pub mod http { pub use ontogen_macros::post; }`) to
/// separate HTTP-method-shape attributes from routing-shape-agnostic
/// markers (`stateless`, `rename`, `skip`).
///
/// The attribute itself expands to a no-op pass-through of the annotated
/// item; the ontogen parser (`servers::parse`) reads the attribute via `syn`
/// during build-time scanning and stamps
/// `ApiFn::force_method = Some(ForcedMethod::Post)`. The classifier
/// consults that field before running its heuristic and returns
/// `OpKind::CustomPost` unconditionally when set.
///
/// Use this on action-verb functions whose zero-user-param shape would
/// otherwise route as GET — e.g. `pause(state)`, `resume(state)`,
/// `reset_all(state)` — even though they mutate state. The classifier
/// can't tell these apart from genuine read-shaped zero-param fns; the
/// attribute is the user-driven escape hatch.
///
/// Usage:
/// ```ignore
/// use ontogen::http::post;
///
/// // Without the attribute, `pause(state)` would emit as `get(...)`
/// // because it has zero user-input params after the state strip.
/// #[post]
/// pub async fn pause(state: &AppState) -> Result<(), AppError> {
///     // ...
/// }
/// ```
///
/// Or with the fully-qualified path inline (no `use` needed):
/// ```ignore
/// #[ontogen::http::post]
/// pub async fn pause(state: &AppState) -> Result<(), AppError> { /* ... */ }
/// ```
///
/// Note: proc-macros must be defined at the crate root (Rust limitation),
/// so this function lives at `ontogen_macros::post`. The `http::*`
/// namespace is achieved via the consumer-side re-export in
/// `ontogen::http::post`. The parser matches on the final path segment,
/// so `#[ontogen::http::post]`, `#[ontogen_macros::post]`, and the bare
/// `#[post]` (after a `use`) all resolve to the same `ForcedMethod::Post`
/// classification.
///
/// Without this marker, the existing auto-classifier applies (zero-user-
/// param fns route as `CustomGet`, named-CRUD operations route by name,
/// `get_*` with a body-carrying first param routes as `CustomPost`, etc.).
#[proc_macro_attribute]
pub fn post(_attr: TokenStream, item: TokenStream) -> TokenStream {
    item
}

/// Pass-through attribute macro for per-function ontogen directives.
///
/// The macro itself is a no-op - the annotated item is returned unchanged so the
/// source remains legal Rust and rust-analyzer / rustdoc still see the original
/// signature. The build script (which calls into `ontogen`) parses the
/// attribute via `syn` and feeds the directives into the codegen pipeline.
///
/// Today only the `rename` directive is interpreted: it overrides the emitted
/// IPC command / TS method name for a single function. The HTTP route path,
/// the underlying Rust function name, and the generated query/body struct
/// names are unchanged.
///
/// Usage:
/// ```ignore
/// use ontogen::ontogen;
///
/// #[ontogen(rename = "tag_get_history")]
/// pub fn get_tag_history(store: &Store, tag: &str) -> Result<Vec<HistoryEntry>, Error> {
///     // ... unchanged
/// }
/// ```
///
/// Future per-function directives can slot into the same `#[ontogen(...)]`
/// umbrella.
#[proc_macro_attribute]
pub fn ontogen(_args: TokenStream, input: TokenStream) -> TokenStream {
    input
}

/// Mark a type as opaque from ontogen-ts's perspective. The walker treats
/// the annotated type as terminal — it doesn't recurse into the type's
/// fields and emits the supplied `target` string verbatim at every
/// reference site.
///
/// Usage:
/// ```ignore
/// use ontogen_macros::ts_opaque;
///
/// #[ts_opaque(target = "Date")]
/// pub struct EpochSeconds(pub i64);
/// ```
///
/// The macro itself is a no-op — the annotated item passes through
/// unchanged at Rust compile time, and `ontogen-ts` reads the attribute
/// via `syn` during build-time scanning. The expected argument shape is
/// `target = "<ts rendering>"` (one mandatory key, value is a string
/// literal); other forms cause a Rust compile error.
#[proc_macro_attribute]
pub fn ts_opaque(args: TokenStream, item: TokenStream) -> TokenStream {
    // Parse + validate args at Rust compile time so a malformed attr
    // surfaces here, not deep inside ontogen-ts's scanner.
    let parsed: syn::Result<TsOpaqueArgs> = syn::parse(args);
    match parsed {
        Ok(_) => item,
        Err(err) => err.to_compile_error().into(),
    }
}

/// Override the TypeScript name emitted for an annotated type. The
/// underlying JSON wire shape is unaffected (serde never sees this attr);
/// only ontogen-ts's TS output uses the override.
///
/// Usage:
/// ```ignore
/// use ontogen_macros::ts_name;
///
/// #[ts_name = "FooStats"]
/// pub struct FooStatistics {
///     pub count: u64,
/// }
/// ```
///
/// The macro itself is a no-op — the annotated item passes through
/// unchanged at Rust compile time. The expected argument shape is a
/// single bare string literal (`= "FooStats"`).
#[proc_macro_attribute]
pub fn ts_name(args: TokenStream, item: TokenStream) -> TokenStream {
    let parsed: syn::Result<syn::LitStr> = syn::parse(args);
    match parsed {
        Ok(_) => item,
        Err(err) => err.to_compile_error().into(),
    }
}

/// Internal: parser for `target = "..."` in `#[ts_opaque(...)]`.
struct TsOpaqueArgs {
    _target: syn::LitStr,
}

impl syn::parse::Parse for TsOpaqueArgs {
    fn parse(input: syn::parse::ParseStream<'_>) -> syn::Result<Self> {
        let ident: syn::Ident = input.parse()?;
        if ident != "target" {
            return Err(syn::Error::new(ident.span(), "ts_opaque expects `target = \"...\"`; unknown argument key"));
        }
        let _eq: syn::Token![=] = input.parse()?;
        let value: syn::LitStr = input.parse()?;
        if !input.is_empty() {
            return Err(syn::Error::new(
                input.span(),
                "ts_opaque accepts a single `target = \"...\"` argument; trailing tokens not allowed",
            ));
        }
        Ok(Self { _target: value })
    }
}
