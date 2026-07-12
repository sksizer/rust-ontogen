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

        code.push_str("use sea_orm::{ActiveModelTrait, EntityTrait, QuerySelect};\n\n");

        // Additional imports for entities with has_many relations
        if entity.has_many_relations().next().is_some() {
            code.push_str("use sea_orm::{ColumnTrait, QueryFilter};\n");
        }

        code.push_str(&format!("use crate::persistence::db::entities::{snake};\n"));
    }

    fn emit_crud_impl(&self, code: &mut String, entity: &EntityDef) {
        gen_crud::generate_crud_impl(code, entity);
    }
}
