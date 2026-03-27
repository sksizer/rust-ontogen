//! Markdown filesystem persistence generator — writers, parser dispatch, fs helpers.

pub mod gen_fs_ops;
pub mod gen_parser;
pub mod gen_writer;

use crate::schema::EntityDef;
use crate::{CodegenError, MarkdownIoConfig};

/// Generate all markdown I/O code: writers, parser dispatch, and fs_ops.
pub fn generate(entities: &[EntityDef], config: &MarkdownIoConfig) -> Result<(), CodegenError> {
    gen_fs_ops::generate(entities, &config.output_dir).map_err(CodegenError::Persistence)?;
    gen_writer::generate(entities, &config.output_dir).map_err(CodegenError::Persistence)?;
    gen_parser::generate(entities, &config.output_dir).map_err(CodegenError::Persistence)?;
    Ok(())
}
