//! SeaORM store backend: CRUD bodies against SeaORM, behind the
//! [`StoreBackend`] seam.

pub(crate) mod gen_crud;

use super::StoreBackend;
use crate::ir::IdStrategy;
use crate::schema::model::EntityDef;
use crate::store::helpers::to_snake_case;

pub(crate) struct SeaormBackend {
    pub(crate) id_strategy: IdStrategy,
}

impl StoreBackend for SeaormBackend {
    fn emit_preamble(&self, code: &mut String, entity: &EntityDef) {
        let snake = to_snake_case(&entity.name);

        // `QueryOrder` backs the `order_by_asc` every multi-row SELECT emits.
        // `list_*` is always generated and always ordered (see `id_column`), so
        // the import is always used — no condition here to keep in lockstep
        // with the one in gen_crud.
        code.push_str("use sea_orm::{ActiveModelTrait, EntityTrait, PaginatorTrait, QueryOrder, QuerySelect};\n\n");

        // Additional imports for entities with has_many relations
        if entity.has_many_relations().next().is_some() {
            code.push_str("use sea_orm::{ColumnTrait, QueryFilter};\n");
        }

        code.push_str(&format!("use crate::persistence::db::entities::{snake};\n"));
    }

    fn emit_crud_impl(&self, code: &mut String, entity: &EntityDef) {
        gen_crud::generate_crud_impl(code, entity, &self.id_strategy);
    }

    fn wikilink_policy(&self) -> super::WikilinkPolicy {
        // SQL stores never carried wikilink-shaped ids; the historical strip
        // calls (and the no-op consumer stubs they forced) were a markdown
        // concern leaking across backends.
        super::WikilinkPolicy::Passthrough
    }
}
