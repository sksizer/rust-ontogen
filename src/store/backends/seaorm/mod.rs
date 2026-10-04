//! SeaORM store backend: CRUD bodies against SeaORM, behind the
//! [`StoreBackend`] seam.

pub(crate) mod gen_crud;

use super::StoreBackend;
use crate::ir::IdStrategy;
use crate::schema::model::{EntityDef, EnumDef};
use crate::store::helpers::to_snake_case;

pub(crate) struct SeaormBackend;

impl StoreBackend for SeaormBackend {
    fn emit_preamble(&self, code: &mut String, entity: &EntityDef) {
        let snake = to_snake_case(&entity.name);

        // `QueryOrder`, `Select` and the `sea_query` pair back
        // `order_{plural}_query`, which every module has, so these imports
        // are always used — no condition here to keep in lockstep with
        // gen_crud.
        code.push_str("use sea_orm::sea_query::{NullOrdering, Order};\n");
        code.push_str(
            "use sea_orm::{ActiveModelTrait, EntityTrait, PaginatorTrait, QueryOrder, QuerySelect, Select};\n\n",
        );

        // Additional imports for entities with has_many relations
        if entity.has_many_relations().next().is_some() {
            code.push_str("use sea_orm::{ColumnTrait, QueryFilter};\n");
        }

        code.push_str(&format!("use crate::persistence::db::entities::{snake};\n"));
    }

    fn emit_crud_impl(&self, code: &mut String, entity: &EntityDef, enums: &[EnumDef], id_strategy: &IdStrategy) {
        gen_crud::generate_crud_impl(code, entity, enums, id_strategy);
    }

    fn wikilink_policy(&self) -> super::WikilinkPolicy {
        // SQL stores never carried wikilink-shaped ids; the historical strip
        // calls (and the no-op consumer stubs they forced) were a markdown
        // concern leaking across backends.
        super::WikilinkPolicy::Passthrough
    }
}
