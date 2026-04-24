//! No-op proc-macro crate that makes `#[ontology(...)]` attributes legal Rust.
//!
//! All actual interpretation of these attributes happens in `build.rs` via `syn`.
//! This crate never contains logic — it just passes the annotated item through unchanged.

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
