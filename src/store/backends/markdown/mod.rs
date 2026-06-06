//! Markdown store backend (ADR 0001): emits CRUD bodies against the
//! `markdown-store` runtime crate through the per-entity
//! `{Entity}Frontmatter` boundary that `gen_markdown_io` generates.

pub(crate) mod gen_crud;

use super::{StoreBackend, WikilinkPolicy};
use crate::ir::{IdStrategy, MarkdownIoOutput};
use crate::schema::model::{EntityDef, FieldType};
use crate::store::helpers::to_snake_case;

pub(crate) struct MarkdownBackend {
    pub(crate) md: MarkdownIoOutput,
}

impl MarkdownBackend {
    /// The slug-source field for `create_record_derived`, from the configured
    /// id strategy. Existence/type validation happened in [`Self::validate`].
    fn slug_source(&self) -> Option<&str> {
        match &self.md.id_strategy {
            IdStrategy::SlugFromField(field) => Some(field.as_str()),
            IdStrategy::Provided | IdStrategy::Uuid => None,
        }
    }
}

impl StoreBackend for MarkdownBackend {
    fn validate(&self, entities: &[EntityDef]) -> Result<(), String> {
        // SlugFromField must name a non-optional String field on EVERY
        // entity — generated create code reads `{entity}.{field}.as_str()`.
        // Failing at generation time beats a per-create runtime surprise.
        if let IdStrategy::SlugFromField(field) = &self.md.id_strategy {
            for entity in entities {
                match entity.fields.iter().find(|f| &f.name == field) {
                    Some(f) if f.field_type == FieldType::String => {}
                    Some(f) => {
                        return Err(format!(
                            "IdStrategy::SlugFromField({field:?}): field `{field}` on entity `{}` \
                             must be a plain String, found {:?}",
                            entity.name, f.field_type
                        ));
                    }
                    None => {
                        return Err(format!(
                            "IdStrategy::SlugFromField({field:?}): entity `{}` has no field `{field}` \
                             to derive ids from",
                            entity.name
                        ));
                    }
                }
            }
        }
        Ok(())
    }

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

        code.push_str(&format!("const {}: &str = \"{dir_segment}\";\n\n", gen_crud::dir_const(&snake)));
    }

    fn emit_crud_impl(&self, code: &mut String, entity: &EntityDef) {
        gen_crud::generate_crud_impl(code, entity, self.slug_source());
    }

    fn wikilink_policy(&self) -> WikilinkPolicy {
        WikilinkPolicy::Strip
    }
}
