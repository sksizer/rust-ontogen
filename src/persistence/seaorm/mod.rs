//! SeaORM persistence generator — entity models, relations, and conversions.

pub mod gen_conversion;
pub mod gen_entity;

use crate::ir::SeaOrmOutput;
use crate::schema::EntityDef;
use crate::{CodegenError, SeaOrmConfig};

/// Generate SeaORM entities and conversions, returning metadata for downstream use.
pub fn generate(entities: &[EntityDef], config: &SeaOrmConfig) -> Result<SeaOrmOutput, CodegenError> {
    gen_entity::generate(entities, &config.entity_output).map_err(CodegenError::Persistence)?;
    gen_conversion::generate(entities, &config.conversion_output, &config.skip_conversions)
        .map_err(CodegenError::Persistence)?;

    // Build output metadata for downstream consumers
    let entity_tables = entities
        .iter()
        .map(|e| {
            let snake = gen_entity::to_snake_case(&e.name);
            crate::ir::EntityTableMeta {
                entity_name: e.name.clone(),
                table_name: e.table.clone(),
                module_path: format!("crate::persistence::db::entities::generated::{snake}"),
                columns: vec![], // TODO: populate from field metadata
            }
        })
        .collect();

    let junction_tables = entities
        .iter()
        .flat_map(|e| {
            e.junction_relations().map(move |(field, info)| {
                let source_snake = gen_entity::to_snake_case(&e.name);
                let junction_name = info.junction.clone().unwrap_or_else(|| format!("{source_snake}_{}", field.name));
                crate::ir::JunctionMeta {
                    table_name: junction_name,
                    source_entity: e.name.clone(),
                    target_entity: info.target.clone(),
                    source_fk: format!("{source_snake}_id"),
                    target_fk: format!("{}_id", gen_entity::to_snake_case(&info.target)),
                }
            })
        })
        .collect();

    let conversion_fns = entities
        .iter()
        .filter(|e| !config.skip_conversions.contains(&e.name))
        .map(|e| {
            let snake = gen_entity::to_snake_case(&e.name);
            crate::ir::ConversionMeta {
                entity_name: e.name.clone(),
                module_path: format!("crate::persistence::db::conversions::generated::{snake}"),
            }
        })
        .collect();

    Ok(SeaOrmOutput { entity_tables, junction_tables, conversion_fns })
}
