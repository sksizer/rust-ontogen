//! Rust names in generated code, and the names a schema may not take.
//!
//! Every emitter follows one convention, so that a schema's names never
//! collide with the generator's own:
//!
//! - Generated Rust names nothing bare but the consumer's own types and API
//!   modules, and the few names listed below that it cannot avoid: the
//!   prelude, the consumer contract (the store, state and error types) and
//!   public items 0.8.0 consumers already name.
//! - A transport, which imports the API modules by name, writes
//!   extern-crate paths rooted (`::serde::Serialize`), so a module named
//!   after a crate cannot shadow it. The few crates other output names
//!   unrooted beside entity modules are refused as module names
//!   ([`SHADOWED_CRATES`]).
//! - Runtime types are imported under an `Ontogen` alias
//!   (`State as OntogenState`) or written by path.
//! - The `Ontogen` (types) and `ontogen_` (bindings, fns) prefixes are
//!   reserved for generated items: a schema or API name never starts with
//!   one, so a generated helper never meets a consumer's name.
//! - A schema-derived name written as Rust code (a module, a binding, a
//!   field) goes through [`rust_ident`] (`Match` → `r#match`); file names,
//!   wire keys and string literals use the bare name.
//!
//! What the convention cannot avoid is refused. The lists below say which
//! names a schema or an API module may not take and why; the stage that
//! knows enough to tell (schema parse, the store, api and servers stages,
//! the API scan) refuses them with an error naming the item and the fix.

/// Strict and reserved keywords of every edition up to 2024, all of which
/// can be written as raw identifiers.
const KEYWORDS: &[&str] = &[
    "abstract", "as", "async", "await", "become", "box", "break", "const", "continue", "do", "dyn", "else", "enum",
    "extern", "false", "final", "fn", "for", "gen", "if", "impl", "in", "let", "loop", "macro", "match", "mod", "move",
    "mut", "override", "priv", "pub", "ref", "return", "static", "struct", "trait", "true", "try", "type", "typeof",
    "unsafe", "unsized", "use", "virtual", "where", "while", "yield",
];

/// Keywords that have no raw form (`r#crate` is not an identifier). An
/// entity whose snake_case name is one of these could name no module, so
/// it is refused at schema parse. `Self` is listed for completeness: no
/// snake_case name equals it.
pub(crate) const UNRAWABLE: &[&str] = &["crate", "self", "super", "Self"];

/// Prelude items the generated code names bare: `Result<T, AppError>`,
/// `Option`, `Vec`, `String`, `Box::pin`, `impl From<…>`,
/// `Default::default()`, `T: Clone + Send + Sync`, `impl Fn(…)`. An entity
/// of the same name, imported beside them, shadows the prelude item.
/// Refused at schema parse.
pub(crate) const PRELUDE: &[&str] =
    &["Box", "Clone", "Default", "Fn", "From", "Option", "Result", "Send", "String", "Sync", "Vec"];

/// The consumer contract the store and API layers import bare: the error,
/// change and kind types from the schema module, the `Store` from
/// `crate::store`, and the runtime's `OrderBy`. An entity of the same name
/// is imported beside them. Refused when the store is generated.
pub(crate) const STORE_CONTRACT: &[&str] = &["AppError", "ChangeOp", "EntityKind", "OrderBy", "Store"];

/// The store contract's modules, `crate::store::generated` and
/// `crate::store::hooks`. An API forwarder reaches an entity's store module
/// as `crate::store::{entity}` through the glob re-export of `generated`,
/// which a module of the same name hides. Refused (as an entity's
/// snake_case name) when API forwarders over the store are generated.
pub(crate) const STORE_MODULES: &[&str] = &["generated", "hooks"];

/// Crates the output names by unrooted path in a scope that also holds
/// entity modules: schemars' `JsonSchema` derive expands to `schemars::…`
/// and `std::…` paths (the MCP server derives it beside the API modules it
/// imports), the SeaORM store imports its entity module by name beside
/// `sea_query` and `std::cmp`, and the markdown vault module declares one
/// module per entity beside `markdown_store` and `std::path`. An entity
/// whose snake_case name is one of these shadows the crate. Refused at
/// schema parse.
pub(crate) const SHADOWED_CRATES: &[&str] = &["markdown_store", "schemars", "sea_query", "std"];

/// The crates of [`SHADOWED_CRATES`] a derive expands to. A hand-written
/// API module of that name is refused when the MCP server is generated.
pub(crate) const DERIVE_CRATES: &[&str] = &["schemars", "std"];

/// Public items of the generated MCP server that 0.8.0 consumers name, so
/// they keep their names. Refused as an entity or API type name when the
/// MCP server is generated.
pub(crate) const MCP_ITEMS: &[&str] = &["ByIntIdInput", "EmptyInput", "GetByIdInput", "McpToolDef", "SimpleToolDef"];

/// Public items of the generated IPC server that 0.8.0 consumers name.
/// Refused as an entity or API type name when the IPC server emits a
/// paginated command.
pub(crate) const IPC_ITEMS: &[&str] = &["PaginatedResult"];

/// The prefix of every generated type outside the 0.8.0 surface
/// (`OntogenListParams`, `OntogenDocResourceAttributes`). Refused (see
/// [`has_type_prefix`]) for entity names at schema parse and for API type
/// names when servers are generated.
pub(crate) const TYPE_PREFIX: &str = "Ontogen";

/// The prefix of every generated binding and helper fn (`ontogen_state`,
/// `ontogen_args`, `ontogen_doc_as_resource`). Refused for entity fields at
/// schema parse and for API fn arguments in the API scan.
pub(crate) const BINDING_PREFIX: &str = "ontogen_";

/// `name` as Rust code: a keyword gets its `r#` prefix, anything else
/// (including a name already written `r#…`) is returned unchanged.
pub(crate) fn rust_ident(name: &str) -> String {
    if KEYWORDS.contains(&name) { format!("r#{name}") } else { name.to_string() }
}

/// Whether `name` starts with the reserved [`TYPE_PREFIX`] as a word:
/// `OntogenWidget` and `Ontogen` do, `Ontogeny` does not.
pub(crate) fn has_type_prefix(name: &str) -> bool {
    name.strip_prefix(TYPE_PREFIX).is_some_and(|rest| !rest.starts_with(|c: char| c.is_ascii_lowercase()))
}

/// A rename to suggest for a refused type name: without the reserved
/// prefix (`OntogenWidget` → `Widget`), or with a suffix (`Result` →
/// `ResultItem`).
pub(crate) fn suggested_rename(name: &str) -> String {
    match name.strip_prefix(TYPE_PREFIX) {
        Some(rest) if has_type_prefix(name) && !rest.is_empty() => rest.to_string(),
        _ if has_type_prefix(name) => "Item".to_string(),
        _ => format!("{name}Item"),
    }
}

/// Why no pipeline can generate an entity named `name` (`snake` is its
/// snake_case name), or `None`. The reason says what the name would clash
/// with.
pub(crate) fn refused_entity_name(name: &str, snake: &str) -> Option<String> {
    if UNRAWABLE.contains(&snake) {
        return Some(format!(
            "its snake_case name `{snake}` is a Rust keyword that cannot name a module, not even as a raw identifier"
        ));
    }
    if PRELUDE.contains(&name) {
        return Some(format!(
            "the generated code names the prelude's `{name}` bare, and the entity type, imported beside it, would \
             shadow it"
        ));
    }
    if has_type_prefix(name) {
        return Some(format!("the `{TYPE_PREFIX}` prefix is reserved for the types ontogen generates"));
    }
    if SHADOWED_CRATES.contains(&snake) {
        return Some(format!(
            "its module `{snake}` would shadow the crate `{snake}`, which the generated code names beside the entity \
             modules"
        ));
    }
    None
}

/// Why no pipeline can generate an entity field or API fn argument named
/// `name`, or `None`.
pub(crate) fn refused_binding_name(name: &str) -> Option<String> {
    let bare = name.strip_prefix("r#").unwrap_or(name);
    bare.starts_with(BINDING_PREFIX)
        .then(|| format!("the `{BINDING_PREFIX}` prefix is reserved for the bindings ontogen generates"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keywords_are_raw_and_other_names_unchanged() {
        assert_eq!(rust_ident("match"), "r#match");
        assert_eq!(rust_ident("type"), "r#type");
        assert_eq!(rust_ident("gen"), "r#gen");
        assert_eq!(rust_ident("r#loop"), "r#loop");
        assert_eq!(rust_ident("matches"), "matches");
        assert_eq!(rust_ident("union"), "union");
    }

    #[test]
    fn entity_names_the_output_cannot_take_are_refused() {
        for (name, snake) in [
            ("Super", "super"),
            ("Result", "result"),
            ("Option", "option"),
            ("Vec", "vec"),
            ("Box", "box"),
            ("String", "string"),
            ("From", "from"),
            ("Default", "default"),
            ("Clone", "clone"),
            ("Send", "send"),
            ("Sync", "sync"),
            ("Fn", "fn"),
            ("OntogenListParams", "ontogen_list_params"),
            ("Std", "std"),
            ("Schemars", "schemars"),
            ("SeaQuery", "sea_query"),
            ("MarkdownStore", "markdown_store"),
        ] {
            assert!(refused_entity_name(name, snake).is_some(), "{name} should be refused");
        }
        // Measured to compile: prelude names the output never writes bare,
        // keyword entities (raw modules) and crates it roots or never names.
        for (name, snake) in [
            ("Document", "document"),
            ("Match", "match"),
            ("Iterator", "iterator"),
            ("Into", "into"),
            ("Ok", "ok"),
            ("Core", "core"),
            ("Ontogeny", "ontogeny"),
        ] {
            assert_eq!(refused_entity_name(name, snake), None, "{name} should be accepted");
        }
    }

    #[test]
    fn a_suggested_rename_is_not_refused_again() {
        assert_eq!(suggested_rename("Result"), "ResultItem");
        assert_eq!(suggested_rename("OntogenWidget"), "Widget");
        assert_eq!(suggested_rename("Ontogen"), "Item");
        assert_eq!(suggested_rename("Ontogeny"), "OntogenyItem");
    }

    #[test]
    fn bindings_with_the_reserved_prefix_are_refused() {
        assert!(refused_binding_name("ontogen_state").is_some());
        assert!(refused_binding_name("r#ontogen_type").is_some());
        assert_eq!(refused_binding_name("ontogeny"), None);
        assert_eq!(refused_binding_name("state"), None);
    }
}
