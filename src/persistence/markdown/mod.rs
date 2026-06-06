//! Markdown filesystem persistence generator - writers, parser dispatch, fs helpers.

pub mod gen_fs_ops;
pub mod gen_parser;
pub mod gen_writer;

use crate::ir::{MarkdownEntityMeta, MarkdownIoOutput};
use crate::schema::EntityDef;
use crate::{CodegenError, MarkdownIoConfig};

/// Generate all markdown I/O code: writers, parser dispatch, and fs_ops.
///
/// Returns the [`MarkdownIoOutput`] metadata `gen_store` consumes when the
/// store backend is [`crate::ir::Backend::Markdown`] (ADR 0001): the vault
/// configuration verbatim from `config`, plus one [`MarkdownEntityMeta`]
/// row per entity derived from the schema IR.
pub fn generate(entities: &[EntityDef], config: &MarkdownIoConfig) -> Result<MarkdownIoOutput, CodegenError> {
    gen_fs_ops::generate(entities, &config.output_dir).map_err(CodegenError::Persistence)?;
    gen_writer::generate(entities, &config.output_dir).map_err(CodegenError::Persistence)?;
    gen_parser::generate(entities, &config.output_dir).map_err(CodegenError::Persistence)?;

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
