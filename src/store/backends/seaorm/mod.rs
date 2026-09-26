//! SeaORM store backend: the emission previously hardwired into the store
//! generator, now behind the [`StoreBackend`] seam. Pure relocation — the
//! generated output is byte-identical to the pre-lift generator, which the
//! snapshot suite enforces.

pub(crate) mod gen_crud;

use super::StoreBackend;
use crate::schema::model::EntityDef;
use crate::store::helpers::to_snake_case;

pub(crate) struct SeaormBackend;

impl StoreBackend for SeaormBackend {
    fn emit_preamble(&self, code: &mut String, entity: &EntityDef) {
        let snake = to_snake_case(&entity.name);

        // `QueryOrder` backs the `order_by_asc` a list emits to make its page
        // deterministic. An entity with no primary key emits no ordering, and an
        // unused import would fail the `--deny warnings` clippy gate, so it is
        // imported only where it is used.
        let order_import = if entity.id_field().is_some() { ", QueryOrder" } else { "" };
        code.push_str(&format!(
            "use sea_orm::{{ActiveModelTrait, EntityTrait, PaginatorTrait{order_import}, QuerySelect}};\n\n"
        ));

        // Additional imports for entities with has_many relations
        if entity.has_many_relations().next().is_some() {
            code.push_str("use sea_orm::{ColumnTrait, QueryFilter};\n");
        }

        code.push_str(&format!("use crate::persistence::db::entities::{snake};\n"));
    }

    fn emit_crud_impl(&self, code: &mut String, entity: &EntityDef) {
        gen_crud::generate_crud_impl(code, entity);
    }

    fn wikilink_policy(&self) -> super::WikilinkPolicy {
        // SQL stores never carried wikilink-shaped ids; the historical strip
        // calls (and the no-op consumer stubs they forced) were a markdown
        // concern leaking across backends.
        super::WikilinkPolicy::Passthrough
    }
}
