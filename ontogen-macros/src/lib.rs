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
