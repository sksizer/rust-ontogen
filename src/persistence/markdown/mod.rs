//! Markdown filesystem persistence generator.
//!
//! Emits one `{Entity}Frontmatter` module per entity — the typed boundary
//! between schema entities and on-disk YAML frontmatter, built on the
//! `markdown-store` runtime crate. This replaced the former emit-everything
//! model (hand-rolled YAML writers, a `serde_yaml_ng` parser dispatcher and
//! `fs_ops` helpers bound to module paths nothing generated): file I/O,
//! atomic writes, walking, and the lossless `Document` round-trip are the
//! runtime crate's job now; the generated code is a thin typed shim. The
//! old vault-scan dispatcher (`OntologyElement`/`ParseResult`) had no
//! generic consumer and was retired with it — bulk scanning returns as a
//! follow-up on top of `VaultHandle` if a consumer earns it.

pub mod gen_frontmatter;
pub mod okf;

use crate::ir::{MarkdownEntityMeta, MarkdownIoOutput, SchemaOutput};
use crate::{CodegenError, MarkdownIoConfig};

/// Generate the markdown I/O code: per-entity `{Entity}Frontmatter` modules.
///
/// Checks every frontmatter key against the keys OKF reserves first (see
/// [`okf::check_frontmatter_keys`]): an error fails generation before
/// anything is written, and each warning is printed as a `cargo:warning=`
/// line.
///
/// Returns the [`MarkdownIoOutput`] metadata `gen_store` consumes when the
/// store backend is [`crate::ir::Backend::Markdown`] (ADR 0001): the vault
/// configuration verbatim from `config`, plus one [`MarkdownEntityMeta`]
/// row per entity derived from the schema IR.
pub fn generate(schema: &SchemaOutput, config: &MarkdownIoConfig) -> Result<MarkdownIoOutput, CodegenError> {
    let entities = &schema.entities;
    let diagnostics = okf::check_frontmatter_keys(entities, &schema.enums);
    if !diagnostics.errors.is_empty() {
        return Err(CodegenError::Persistence(diagnostics.errors.join("\n")));
    }
    for warning in &diagnostics.warnings {
        println!("cargo:warning={warning}");
    }

    gen_frontmatter::generate(entities, &config.output_dir).map_err(CodegenError::Persistence)?;

    let entity_meta = entities
        .iter()
        .map(|entity| MarkdownEntityMeta {
            entity_name: entity.name.clone(),
            type_name: entity.type_name.clone(),
            dir_segment: entity.directory.clone(),
            body_field: entity.body_field().map(|f| f.name.clone()),
            // v1 contract: every many_to_many field is authoritative on the
            // declaring side; the reverse view is a derived walk.
            authoritative_m2m: entity.junction_relations().map(|(field, _)| field.name.clone()).collect(),
        })
        .collect();

    Ok(MarkdownIoOutput {
        vault_root: config.vault_root.clone(),
        layout: config.layout,
        id_strategy: config.id_strategy.clone(),
        list_cap: config.list_cap,
        module_path: "crate::persistence::markdown::generated".to_string(),
        entities: entity_meta,
    })
}
