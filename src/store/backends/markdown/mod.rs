//! Markdown store backend (ADR 0001): emits CRUD bodies against the
//! `markdown-store` runtime crate through the per-entity
//! `{Entity}Frontmatter` boundary that `gen_markdown_io` generates.

pub(crate) mod gen_crud;

use super::{StoreBackend, WikilinkPolicy};
use crate::ir::{IdStrategy, MarkdownIoOutput};
use crate::schema::model::EntityDef;
use crate::store::helpers::to_snake_case;

pub(crate) struct MarkdownBackend {
    pub(crate) md: MarkdownIoOutput,
    pub(crate) id_strategy: IdStrategy,
}

impl StoreBackend for MarkdownBackend {
    fn emit_preamble(&self, code: &mut String, entity: &EntityDef) {
        let name = &entity.name;
        let snake = to_snake_case(name);
        let shout = snake.to_uppercase();
        let module = &self.md.module_path;

        code.push_str(&format!("use {module}::{snake}::{{{shout}_FM_FIELDS, {name}Frontmatter}};\n"));
    }

    fn emit_declarations(&self, code: &mut String, entity: &EntityDef) {
        let snake = to_snake_case(&entity.name);
        let meta = self.md.entities.iter().find(|m| m.entity_name == entity.name);
        let dir_segment = meta.map(|m| m.dir_segment.as_str()).unwrap_or(&entity.directory);
        let type_name = meta.map(|m| m.type_name.as_str()).unwrap_or(&entity.type_name);

        code.push_str(&format!("const {}: &str = \"{dir_segment}\";\n", gen_crud::dir_const(&snake)));
        code.push_str(&format!("const {}: &str = {type_name:?};\n\n", gen_crud::type_const(&snake)));
    }

    fn emit_crud_impl(&self, code: &mut String, entity: &EntityDef) {
        gen_crud::generate_crud_impl(code, entity, &self.id_strategy);
    }

    fn wikilink_policy(&self) -> WikilinkPolicy {
        WikilinkPolicy::Strip
    }
}
