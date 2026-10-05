//! Generate the Store layer: CRUD methods with lifecycle hooks, Update structs,
//! From impls, and populate_relations helpers.
//!
//! For each entity, generates a `store/generated/{entity}.rs` containing:
//! - `{Entity}Update` struct with `apply()` method
//! - `From<Create{Entity}Input> for {Entity}` impl
//! - `From<Update{Entity}Input> for {Entity}Update` impl
//! - `{Entity}SortField` and `sort_{plural}`, the order `list_{plural}` takes
//!   (ADR 0006), identical on both backends
//! - `impl Store` block with list, count, get, create, update, delete (the
//!   writes calling hooks)
//! - `populate_{entity}_relations()` for entities with junction/has_many fields
//!
//! Also scaffolds `store/hooks/{entity}.rs` with no-op lifecycle hook functions:
//! - `before_create`, `after_create`, `before_update`, `after_update`,
//!   `before_delete`, `after_delete`
//!
//! Hook files are written once and never overwritten - fill in your custom logic.
//!
//! Generated bodies bind the record as `record` and never under a name taken
//! from the schema: an entity or field named like one of the generator's own
//! locals (`Doc` beside the markdown store's `doc`) would shadow it, and a
//! keyword entity (`Match`) has no plain binding. Runtime items are imported
//! unnamed (`as _`) or written by path; the few names the store imports bare
//! (`OrderBy` and the consumer contract) are refused as entity names, see
//! [`check_entity_names`].

mod backends;
mod gen_hooks;
mod gen_order;
mod gen_update;
mod has_many;
pub(crate) mod helpers;
mod int_range;
mod linked_ids;
mod nan;
#[cfg(test)]
mod tests;

use std::fs;

use crate::ident::rust_ident;
use crate::ir::{
    CrudOp, IdStrategy, ParamMeta, ScaffoldMeta, SchemaOutput, Source, StoreMethodKind, StoreMethodMeta, StoreOutput,
};
use crate::schema::model::{EntityDef, EnumDef, FieldType};
use crate::{CodegenError, StoreConfig};

// ─── Public API ──────────────────────────────────────────────────────────────

/// Refuses an entity named like a type the store imports bare
/// ([`crate::ident::STORE_CONTRACT`]).
pub(crate) fn check_entity_names(entities: &[EntityDef]) -> Result<(), String> {
    for entity in entities {
        let name = &entity.name;
        if crate::ident::STORE_CONTRACT.contains(&name.as_str()) {
            return Err(format!(
                "ontogen: entity `{name}` cannot have a store: the generated store and API import the consumer \
                 contract's `{name}` bare beside the entity types, so the two would clash. Rename the entity (e.g. \
                 `{name}Item`)."
            ));
        }
    }
    Ok(())
}

/// Generate store layer code for the schema's entities.
///
/// Writes generated CRUD files to `config.output_dir` and scaffolds hook files
/// in `config.hooks_dir`. Returns `StoreOutput` metadata for downstream
/// generators (gen_api, gen_servers). The persistence backend is selected by
/// `config.backend` (ADR 0001).
pub fn generate(schema: &SchemaOutput, config: &StoreConfig) -> Result<StoreOutput, CodegenError> {
    let entities = &schema.entities[..];
    // Resolve and validate up front so misconfiguration fails loudly before
    // any files are written.
    check_entity_names(entities).map_err(CodegenError::Store)?;
    validate_id_fields(entities).map_err(CodegenError::Store)?;
    validate_id_strategies(entities, &config.id_strategy).map_err(CodegenError::Store)?;
    has_many::validate_targets(entities).map_err(CodegenError::Store)?;
    let backend = backends::for_backend(&config.backend)?;
    backend.validate(entities).map_err(CodegenError::Store)?;

    let output_dir = &config.output_dir;
    fs::create_dir_all(output_dir)
        .map_err(|e| CodegenError::Store(format!("Failed to create {}: {e}", output_dir.display())))?;

    // Clean stale files from previous entity names
    let expected: std::collections::HashSet<String> = entities
        .iter()
        .map(|e| format!("{}.rs", helpers::to_snake_case(&e.name)))
        .chain(std::iter::once("mod.rs".to_string()))
        .collect();
    crate::clean_generated_dir(output_dir, &expected);

    let mut all_methods: Vec<StoreMethodMeta> = Vec::new();
    let mut mod_names: Vec<String> = Vec::new();

    for entity in entities {
        let snake = helpers::to_snake_case(&entity.name);

        // Generate the entity's store module
        let code = generate_entity_store(&*backend, entity, entities, &schema.enums, config);

        let path = output_dir.join(format!("{snake}.rs"));
        crate::write_and_format(&path, &code)?;

        // Collect method metadata for downstream consumers
        let methods = collect_method_meta(entity);
        all_methods.extend(methods);
        mod_names.push(snake);
    }

    // Write mod.rs
    mod_names.sort();
    let mod_rs = generate_mod_rs(&mod_names);
    let path = output_dir.join("mod.rs");
    crate::write_and_format(&path, &mod_rs)?;

    // Scaffold hook files (only creates files that don't exist yet)
    let mut scaffolded_hooks = Vec::new();
    if let Some(hooks_dir) = &config.hooks_dir {
        let created =
            gen_hooks::scaffold_hooks(entities, hooks_dir, &config.schema_module_path).map_err(CodegenError::Store)?;

        for entity in entities {
            let snake = helpers::to_snake_case(&entity.name);
            let was_created = created.contains(&snake);
            if was_created {
                scaffolded_hooks.push(ScaffoldMeta {
                    entity_name: entity.name.clone(),
                    file_path: hooks_dir.join(format!("{snake}.rs")),
                    functions: vec![
                        "before_create".into(),
                        "after_create".into(),
                        "before_update".into(),
                        "after_update".into(),
                        "before_delete".into(),
                        "after_delete".into(),
                    ],
                });
            }
        }
    }

    Ok(StoreOutput { methods: all_methods, scaffolded_hooks, change_channels: Vec::new() })
}

/// The id strategy `entity`'s create uses: its own
/// `#[ontology(entity, id = "...")]`, else the store-wide default.
fn effective_id_strategy<'a>(entity: &'a EntityDef, default: &'a IdStrategy) -> &'a IdStrategy {
    entity.id_strategy.as_ref().unwrap_or(default)
}

/// Every generated store method reads the record's id, and the id is the
/// final key of every order (ADR 0006 §3).
fn validate_id_fields(entities: &[EntityDef]) -> Result<(), String> {
    match entities.iter().find(|e| e.id_field().is_none()) {
        Some(entity) => {
            Err(format!("entity `{}` has no `#[ontology(id)]` field; the generated store needs one", entity.name))
        }
        None => Ok(()),
    }
}

/// `SlugFromField` must name a non-optional `String` field on every entity it
/// applies to: the generated create reads `{entity}.{field}` on either
/// backend. The default applies only to entities without an override.
fn validate_id_strategies(entities: &[EntityDef], default: &IdStrategy) -> Result<(), String> {
    for entity in entities {
        let IdStrategy::SlugFromField(field) = effective_id_strategy(entity, default) else {
            continue;
        };
        let problem = match entity.fields.iter().find(|f| &f.name == field) {
            Some(f) if f.field_type == FieldType::String => continue,
            Some(f) => format!("field `{field}` must be a plain String to derive ids from, found {:?}", f.field_type),
            None => format!("the entity has no field `{field}` to derive ids from"),
        };
        let name = &entity.name;
        let (source, remedy) = if entity.id_strategy.is_some() {
            (
                format!("`#[ontology(entity, id = \"slug({field})\")]` on entity `{name}`"),
                format!(
                    "name a plain String field of `{name}` in `slug(...)`, or use `id = \"provided\"` or `id = \"uuid\"`"
                ),
            )
        } else {
            (
                format!("the store default IdStrategy::SlugFromField({field:?}), which applies to entity `{name}`"),
                format!(
                    "give `{name}` its own `#[ontology(entity, id = \"provided\" | \"uuid\" | \"slug(<field>)\")]`, \
                     or choose a store default that fits every entity without one"
                ),
            )
        };
        return Err(format!("{source}: {problem}; {remedy}"));
    }
    Ok(())
}

// ─── Per-entity generation ───────────────────────────────────────────────────

/// Generate the complete store module source for one entity.
///
/// The persistence-specific parts (preamble imports, CRUD bodies) come from
/// the active [`backends::StoreBackend`]; everything emitted here directly
/// is backend-agnostic and MUST stay that way — together with
/// [`collect_method_meta`] never branching on backend, that is what makes
/// the downstream `gen_api`/`gen_servers`/`gen_clients` output byte-identical
/// across backends (ADR 0001, contract item 5).
fn generate_entity_store(
    backend: &dyn backends::StoreBackend,
    entity: &EntityDef,
    entities: &[EntityDef],
    enums: &[EnumDef],
    config: &StoreConfig,
) -> String {
    let snake = helpers::to_snake_case(&entity.name);
    let schema_path = &config.schema_module_path;
    let mut code = String::with_capacity(4096);

    code.push_str("//! Generated by ontogen. DO NOT EDIT.\n\n");
    code.push_str("use ontogen_core::order::OrderBy;\n\n");
    backend.emit_preamble(&mut code, entity);

    // Shared imports (sorted alphabetically for rustfmt compliance)
    code.push_str(&format!("use {schema_path}::{};\n", entity.name));
    code.push_str(&format!("use {schema_path}::{{AppError, ChangeOp, EntityKind}};\n\n"));
    code.push_str(&format!("use crate::store::hooks::{} as hooks;\n", rust_ident(&snake)));
    code.push_str("use crate::store::Store;\n\n");

    // Backend-specific module-level declarations (e.g. the markdown
    // entity-directory constant) — nothing for SeaORM.
    backend.emit_declarations(&mut code, entity);

    // Update struct + apply + From impls. The struct/apply shape is shared
    // across backends; the From impls' wikilink handling follows the
    // backend's policy (markdown strips `[[id]]` at the boundary, SQL
    // backends pass ids through untouched) unless the config overrides it —
    // a SQL-backed store can opt into stripping when its wire contract
    // still accepts wikilinked ids.
    let policy = config.wikilink_policy.unwrap_or_else(|| backend.wikilink_policy());
    gen_update::generate_update_struct(&mut code, entity);
    gen_update::generate_apply_method(&mut code, entity);
    gen_update::generate_from_update_input(&mut code, entity, policy);
    gen_update::generate_from_create_input(&mut code, entity, policy);

    // The sort field and in-memory sort, identical on both backends.
    gen_order::generate_order_block(&mut code, entity, enums, schema_path);

    // CRUD impl block (with hook calls) — backend-specific bodies
    backend.emit_crud_impl(&mut code, entity, entities, enums, effective_id_strategy(entity, &config.id_strategy));

    code
}

// ─── Metadata collection ─────────────────────────────────────────────────────

/// Collect StoreMethodMeta for all generated CRUD methods on this entity.
///
/// Parameter shapes mirror the signatures emitted by `gen_crud`:
///   `list_{plural}(order: &[OrderBy<{Name}SortField>], limit: Option<u64>, offset: Option<u64>)`
///   `count_{plural}()`
///   `get_{snake}(id: &str)`
///   `create_{snake}(record: {Name})`
///   `update_{snake}(id: &str, updates: {Name}Update)`
///   `delete_{snake}(id: &str)`
fn collect_method_meta(entity: &EntityDef) -> Vec<StoreMethodMeta> {
    let snake = helpers::to_snake_case(&entity.name);
    let plural = helpers::pluralize(&snake);
    let source = Source::Generated { module_path: format!("crate::store::generated::{}", rust_ident(&snake)) };
    let id_param = || ParamMeta { name: "id".to_string(), param_type: "&str".to_string() };

    vec![
        StoreMethodMeta {
            entity_name: entity.name.clone(),
            name: format!("list_{plural}"),
            kind: StoreMethodKind::Crud(CrudOp::List),
            params: vec![
                ParamMeta {
                    name: "order".to_string(),
                    param_type: format!("&[OrderBy<{}>]", gen_order::sort_field_type(entity)),
                },
                ParamMeta { name: "limit".to_string(), param_type: "Option<u64>".to_string() },
                ParamMeta { name: "offset".to_string(), param_type: "Option<u64>".to_string() },
            ],
            return_type: format!("Vec<{}>", entity.name),
            source: source.clone(),
        },
        StoreMethodMeta {
            entity_name: entity.name.clone(),
            name: format!("count_{plural}"),
            kind: StoreMethodKind::Crud(CrudOp::Count),
            params: Vec::new(),
            return_type: "u64".to_string(),
            source: source.clone(),
        },
        StoreMethodMeta {
            entity_name: entity.name.clone(),
            name: format!("get_{snake}"),
            kind: StoreMethodKind::Crud(CrudOp::Get),
            params: vec![id_param()],
            return_type: entity.name.clone(),
            source: source.clone(),
        },
        StoreMethodMeta {
            entity_name: entity.name.clone(),
            name: format!("create_{snake}"),
            kind: StoreMethodKind::Crud(CrudOp::Create),
            params: vec![ParamMeta { name: "record".to_string(), param_type: entity.name.clone() }],
            return_type: entity.name.clone(),
            source: source.clone(),
        },
        StoreMethodMeta {
            entity_name: entity.name.clone(),
            name: format!("update_{snake}"),
            kind: StoreMethodKind::Crud(CrudOp::Update),
            params: vec![
                id_param(),
                ParamMeta { name: "updates".to_string(), param_type: format!("{}Update", entity.name) },
            ],
            return_type: entity.name.clone(),
            source: source.clone(),
        },
        StoreMethodMeta {
            entity_name: entity.name.clone(),
            name: format!("delete_{snake}"),
            kind: StoreMethodKind::Crud(CrudOp::Delete),
            params: vec![id_param()],
            return_type: "()".to_string(),
            source,
        },
    ]
}

// ─── mod.rs generation ───────────────────────────────────────────────────────

fn generate_mod_rs(mod_names: &[String]) -> String {
    let mut code = String::new();
    code.push_str("//! Generated by ontogen. DO NOT EDIT.\n\n");
    for name in mod_names {
        code.push_str(&format!("pub mod {};\n", rust_ident(name)));
    }
    code
}
