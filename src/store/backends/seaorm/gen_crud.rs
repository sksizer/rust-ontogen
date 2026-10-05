//! Generate CRUD methods (`list`, `get`, `create`, `update`, `delete`) and
//! `populate_relations` on `impl Store` for each entity.
//!
//! Handles three complexity tiers:
//! - **Simple** (no relations): Direct CRUD without junction sync or relation population
//! - **Junction** (many_to_many / has_many): CRUD + populate_relations + sync_junction
//! - Both tiers emit events via `self.emit_change()`
//!
//! `sync_junction` and `load_junction_ids` are the consumer's. A
//! many_to_many list is in the order it was written on both backends
//! (ADR 0006 §3), so `sync_junction` must insert the ids in list order and
//! `load_junction_ids` must read them back in insertion order
//! (`ORDER BY rowid` on SQLite).
//!
//! `create_*` and `update_*` make every write (the record's row, each
//! `sync_junction`, each `set_*_parent`) in one transaction, committed
//! before the read-back, `emit_change` and the `after_*` hook. A write that
//! fails part-way leaves nothing written. Every statement inside goes
//! through the transaction, which `sync_junction` takes as its first
//! argument: a pool of one connection would wait forever on `self.db()`
//! while the transaction holds it.
//!
//! `create_*` calls `ontogen_core::id` at runtime, so the consumer depends
//! on `ontogen-core`, with its `uuid` feature under `IdStrategy::Uuid`.

use crate::ident::rust_ident;
use crate::ir::IdStrategy;
use crate::resource::member_name;
use crate::schema::model::{EntityDef, EnumDef};
use crate::schema::sort::{SortKind, sort_fields};
use crate::store::gen_order::sort_field_type;
use crate::store::has_many::{self, has_many_writes};
use crate::store::helpers::{
    junction_source_col, junction_table_name, junction_target_col, pluralize, to_pascal_case, to_snake_case,
};
use crate::store::int_range::{IntegerSource, emit_integer_range_checks};
use crate::store::linked_ids::{self, Listed};
use crate::store::nan::{FloatSource, Skipped, emit_nan_checks};

// ─── Public API ──────────────────────────────────────────────────────────────

/// Generate the complete `impl Store { ... }` block with CRUD methods.
pub fn generate_crud_impl(
    code: &mut String,
    entity: &EntityDef,
    entities: &[EntityDef],
    enums: &[EnumDef],
    id_strategy: &IdStrategy,
) {
    let has_relations = entity.junction_relations().next().is_some() || entity.has_many_relations().next().is_some();

    code.push_str("impl Store {\n");

    generate_list(code, entity, has_relations);
    generate_count(code, entity);
    generate_get(code, entity, has_relations);
    generate_create(code, entity, entities, id_strategy);
    generate_update(code, entity, entities, has_relations);
    generate_delete(code, entity);

    if has_relations {
        generate_populate_relations(code, entity);
    }

    generate_try_insert_helper(code, entity);

    // has_many reverse helpers (e.g., set_node_parent)
    for hm in &has_many_writes(entity) {
        generate_set_parent_helper(code, entity, hm.fk, hm.fk_required);
    }
    if linked_ids::needs_exists_helper(entity, entities) {
        generate_exists_helper(code, entity);
    }

    code.push_str("}\n\n");

    generate_order_query(code, entity, enums);
}

// ─── Individual CRUD methods ─────────────────────────────────────────────────

fn generate_list(code: &mut String, entity: &EntityDef, has_relations: bool) {
    let name = &entity.name;
    let snake = to_snake_case(name);
    let module = rust_ident(&snake);
    let plural = pluralize(&snake);
    let sort_field = sort_field_type(entity);

    code.push_str(&format!(
        "    pub async fn list_{plural}(&self, order: &[OrderBy<{sort_field}>], limit: Option<u64>, offset: Option<u64>) -> Result<Vec<{name}>, AppError> {{\n"
    ));
    // The order is applied before the page is cut, or the page is not a
    // slice of that order.
    code.push_str(&format!("        let mut query = order_{plural}_query({module}::Entity::find(), order);\n"));
    // The engine takes `LIMIT` and `OFFSET` as i64, and sea-query panics
    // binding a larger u64. Clamped, an oversized offset is past the end (an
    // empty page) and an oversized limit is every row, as on markdown.
    code.push_str("        let limit = limit.map(|l| l.min(i64::MAX as u64));\n");
    code.push_str("        let offset = offset.map(|o| o.min(i64::MAX as u64));\n");
    code.push_str(
        "        // sqlite-only: SQLite rejects OFFSET without LIMIT, so an offset alone takes i64::MAX rows.\n",
    );
    code.push_str("        if let Some(l) = limit.or(offset.map(|_| i64::MAX as u64)) {\n");
    code.push_str("            query = query.limit(l);\n");
    code.push_str("        }\n");
    code.push_str("        if let Some(o) = offset {\n");
    code.push_str("            query = query.offset(o);\n");
    code.push_str("        }\n");
    code.push_str("        let models = query\n");
    code.push_str("            .all(self.db())\n");
    code.push_str("            .await\n");
    code.push_str("            .map_err(|e| AppError::DbError(e.to_string()))?;\n\n");

    if has_relations {
        code.push_str(&format!(
            "        let mut records: Vec<{name}> = models.iter().map({name}::from_model).collect::<Result<_, _>>()?;\n"
        ));
        code.push_str("        for record in &mut records {\n");
        code.push_str(&format!("            self.populate_{snake}_relations(record).await?;\n"));
        code.push_str("        }\n");
        code.push_str("        Ok(records)\n");
    } else {
        code.push_str(&format!("        models.iter().map({name}::from_model).collect()\n"));
    }

    code.push_str("    }\n\n");
}

/// `order_{plural}_query`: the `ORDER BY` of `list_*`, for a hand-written
/// list that filters in SQL to order by the same rules (ADR 0006 §1.1, §4).
/// A page only means something over a total order: `LIMIT`/`OFFSET` with no
/// `ORDER BY` lets the engine return rows in whatever order it likes, so the
/// same offset can repeat a row and skip another. `effective` ends every
/// order on the id, so the order is total.
///
/// Null ordering is stated for every key, though SQLite's default already
/// puts nulls first ascending and last descending, so the rule is in the
/// generated code rather than implied by the engine. String keys compare
/// bytewise as on markdown only under SQLite's default BINARY collation:
/// generated columns declare no `COLLATE`, and a text collation such as
/// Postgres under `en_US.UTF-8` (`alpha` before `Zeta`) or MySQL's
/// case-insensitive default would not.
fn generate_order_query(code: &mut String, entity: &EntityDef, enums: &[EnumDef]) {
    let name = &entity.name;
    let snake = to_snake_case(name);
    let module = rust_ident(&snake);
    let plural = pluralize(&snake);
    let sort_field = sort_field_type(entity);

    code.push_str(&format!(
        "/// Applies `order` to `query` as `list_{plural}` does: each key with nulls first ascending and last\n"
    ));
    code.push_str("/// descending, then the id. A hand-written list that filters in SQL orders through this.\n");
    code.push_str(&format!(
        "pub fn order_{plural}_query(mut query: ::sea_orm::Select<{module}::Entity>, order: &[OrderBy<{sort_field}>]) -> ::sea_orm::Select<{module}::Entity> {{\n"
    ));
    code.push_str("    for key in ontogen_core::order::effective(order) {\n");
    code.push_str("        let column = match key.field {\n");
    for spec in sort_fields(entity, enums) {
        let column = match spec.kind {
            SortKind::Id => id_column(entity),
            _ => to_pascal_case(member_name(&spec.field.name)),
        };
        code.push_str(&format!("            {sort_field}::{} => {module}::Column::{column},\n", spec.variant));
    }
    code.push_str("        };\n");
    code.push_str("        let (direction, nulls) = match key.direction {\n");
    code.push_str(
        "            ontogen_core::order::Direction::Asc => (::sea_orm::sea_query::Order::Asc, ::sea_orm::sea_query::NullOrdering::First),\n",
    );
    code.push_str("            ontogen_core::order::Direction::Desc => (::sea_orm::sea_query::Order::Desc, ::sea_orm::sea_query::NullOrdering::Last),\n");
    code.push_str("        };\n");
    code.push_str("        // sqlite-only: string keys sort in byte order under SQLite's default BINARY collation.\n");
    code.push_str("        query = query.order_by_with_nulls(column, direction, nulls);\n");
    code.push_str("    }\n");
    code.push_str("    query\n");
    code.push_str("}\n");
}

fn generate_count(code: &mut String, entity: &EntityDef) {
    let snake = to_snake_case(&entity.name);
    let module = rust_ident(&snake);
    let plural = pluralize(&snake);

    code.push_str(&format!("    pub async fn count_{plural}(&self) -> Result<u64, AppError> {{\n"));
    code.push_str(&format!("        {module}::Entity::find()\n"));
    code.push_str("            .count(self.db())\n");
    code.push_str("            .await\n");
    code.push_str("            .map_err(|e| AppError::DbError(e.to_string()))\n");
    code.push_str("    }\n\n");
}

fn generate_get(code: &mut String, entity: &EntityDef, has_relations: bool) {
    let name = &entity.name;
    let snake = to_snake_case(name);
    let module = rust_ident(&snake);
    let not_found = not_found_variant(name);

    code.push_str(&format!("    pub async fn get_{snake}(&self, id: &str) -> Result<{name}, AppError> {{\n"));
    code.push_str(&format!("        let model = {module}::Entity::find_by_id(id)\n"));
    code.push_str("            .one(self.db())\n");
    code.push_str("            .await\n");
    code.push_str("            .map_err(|e| AppError::DbError(e.to_string()))?\n");
    code.push_str(&format!("            .ok_or_else(|| AppError::{not_found}(id.to_string()))?;\n\n"));

    if has_relations {
        code.push_str(&format!("        let mut record = {name}::from_model(&model)?;\n"));
        code.push_str(&format!("        self.populate_{snake}_relations(&mut record).await?;\n"));
        code.push_str("        Ok(record)\n");
    } else {
        code.push_str(&format!("        {name}::from_model(&model)\n"));
    }

    code.push_str("    }\n\n");
}

fn generate_create(code: &mut String, entity: &EntityDef, entities: &[EntityDef], id_strategy: &IdStrategy) {
    let name = &entity.name;
    let snake = to_snake_case(name);
    let entity_kind = entity_kind_variant(name);

    code.push_str(&format!(
        "    pub async fn create_{snake}(&self, mut record: {name}) -> Result<{name}, AppError> {{\n"
    ));

    // Hook: before_create (can modify entity or reject)
    code.push_str("        hooks::before_create(self, &mut record).await?;\n\n");

    // The junction and has_many lists are written from the record once it
    // has an id; the insert only borrows it.
    let junctions: Vec<_> = entity.junction_relations().collect();
    let listed = Listed::Create("record");
    linked_ids::emit_self_listing_check(code, entity, &listed);
    linked_ids::emit_listed_ids_check(code, entity, entities, &listed);
    emit_integer_range_checks(code, entity, IntegerSource::Record("record"), Skipped::Stored, db_error);
    emit_nan_checks(code, entity, FloatSource::Record("record"), Skipped::Stored, db_error);

    emit_begin(code);
    generate_insert_with_id(code, entity, id_strategy);

    // Sync junction tables
    for (field, info) in &junctions {
        let jt = junction_table_name(&snake, member_name(&field.name), info.junction.as_deref());
        let src_col = junction_source_col(&snake);
        let target_snake = to_snake_case(&info.target);
        let is_self_ref = entity.name == info.target;
        let tgt_col = junction_target_col(&snake, &target_snake, is_self_ref);

        code.push_str(&format!(
            "        self.sync_junction(&txn, \"{jt}\", \"{src_col}\", \"{tgt_col}\", &id, &record.{}).await?;\n",
            field.name
        ));
    }

    // Set parent_id on children (has_many reverse)
    for hm in has_many_writes(entity) {
        code.push_str(&format!("        for child_id in &record.{} {{\n", hm.field));
        code.push_str(&format!(
            "            self.set_{snake}_parent(&txn, child_id, {}).await?;\n",
            has_many::set_parent_arg(hm.fk_required, "&id")
        ));
        code.push_str("        }\n");
    }

    emit_commit(code);

    code.push_str(&format!("        let created = self.get_{snake}(&id).await?;\n"));
    code.push_str(&format!("        self.emit_change(ChangeOp::Created, EntityKind::{entity_kind}, id);\n\n"));

    // Hook: after_create
    code.push_str("        hooks::after_create(self, &created).await?;\n");
    code.push_str("        Ok(created)\n");
    code.push_str("    }\n\n");
}

/// Open the transaction every write of `create_*` and `update_*` goes
/// through, as `txn`. Dropped without a commit, it rolls back.
fn emit_begin(code: &mut String) {
    code.push_str("        let txn = self.db().begin().await.map_err(|e| AppError::DbError(e.to_string()))?;\n");
}

fn emit_commit(code: &mut String) {
    code.push_str("        txn.commit().await.map_err(|e| AppError::DbError(e.to_string()))?;\n\n");
}

/// The SeaORM store's catch-all for a value it refuses before writing.
fn db_error(message: &str) -> String {
    format!("AppError::DbError({message})")
}

/// Emit the insert of a new record, binding `id`. The same rules as the
/// markdown runtime's create (JSON:API wire contract §8.2): an id that is
/// empty or whitespace-only is absent, so the strategy derives one, probing
/// `-2`, `-3`, … past ids that are taken or reserved; any other id is
/// validated and inserted as given, and a duplicate is
/// `{Entity}AlreadyExists`. A derived id another writer took between the
/// probe and the insert moves on to the next suffix.
fn generate_insert_with_id(code: &mut String, entity: &EntityDef, id_strategy: &IdStrategy) {
    let name = &entity.name;
    let snake = to_snake_case(name);
    let module = rust_ident(&snake);

    let base = match id_strategy {
        IdStrategy::Provided => None,
        IdStrategy::SlugFromField(field) => Some(format!("ontogen_core::id::slugify(&record.{})", rust_ident(field))),
        IdStrategy::Uuid => Some("ontogen_core::id::new_uuid()".to_string()),
    };

    code.push_str("        let id = if record.id.trim().is_empty() {\n");
    match (&base, id_strategy) {
        (None, _) => {
            code.push_str(&format!(
                "            return Err(AppError::{name}IdRequired({:?}.to_string()));\n",
                "this store requires the caller to supply an id"
            ));
        }
        (Some(base), strategy) => {
            code.push_str(&format!("            let base = {base};\n"));
            if let IdStrategy::SlugFromField(field) = strategy {
                code.push_str("            if base.is_empty() {\n");
                code.push_str(&format!(
                    "                return Err(AppError::{name}IdRequired({:?}.to_string()));\n",
                    format!("field {field:?} produced an empty slug")
                ));
                code.push_str("            }\n");
            }
            code.push_str("            for candidate in ontogen_core::id::candidates(&base) {\n");
            code.push_str(&format!("                let taken = {module}::Entity::find_by_id(candidate.as_str())\n"));
            code.push_str("                    .one(&txn)\n");
            code.push_str("                    .await\n");
            code.push_str("                    .map_err(|e| AppError::DbError(e.to_string()))?\n");
            code.push_str("                    .is_some();\n");
            code.push_str("                if taken {\n");
            code.push_str("                    continue;\n");
            code.push_str("                }\n");
            code.push_str("                record.id = candidate;\n");
            code.push_str(&format!("                if self.try_insert_{snake}(&txn, &record).await? {{\n"));
            code.push_str("                    break;\n");
            code.push_str("                }\n");
            code.push_str("            }\n");
            code.push_str("            record.id.clone()\n");
        }
    }
    code.push_str("        } else {\n");
    code.push_str(
        "            ontogen_core::id::validate_id(&record.id).map_err(|e| AppError::DbError(e.to_string()))?;\n",
    );
    code.push_str(&format!("            if !self.try_insert_{snake}(&txn, &record).await? {{\n"));
    code.push_str(&format!("                return Err(AppError::{name}AlreadyExists(record.id));\n"));
    code.push_str("            }\n");
    code.push_str("            record.id.clone()\n");
    code.push_str("        };\n\n");
}

fn generate_update(code: &mut String, entity: &EntityDef, entities: &[EntityDef], has_relations: bool) {
    let name = &entity.name;
    let snake = to_snake_case(name);
    let module = rust_ident(&snake);
    let not_found = not_found_variant(name);
    let entity_kind = entity_kind_variant(name);

    code.push_str(&format!(
        "    pub async fn update_{snake}(&self, id: &str, updates: {name}Update) -> Result<{name}, AppError> {{\n"
    ));

    // Fetch existing
    code.push_str(&format!("        let existing_model = {module}::Entity::find_by_id(id)\n"));
    code.push_str("            .one(self.db())\n");
    code.push_str("            .await\n");
    code.push_str("            .map_err(|e| AppError::DbError(e.to_string()))?\n");
    code.push_str(&format!("            .ok_or_else(|| AppError::{not_found}(id.to_string()))?;\n\n"));

    code.push_str(&format!("        let mut current = {name}::from_model(&existing_model)?;\n"));

    if has_relations {
        code.push_str(&format!("        self.populate_{snake}_relations(&mut current).await?;\n\n"));
    }

    // Hook: before_update (can validate or reject)
    code.push_str("        hooks::before_update(self, &current, &updates).await?;\n\n");

    // Track which junction fields changed
    let junctions: Vec<_> = entity.junction_relations().collect();
    let has_manys: Vec<_> = entity.has_many_relations().collect();

    for (field, _info) in &junctions {
        code.push_str(&format!(
            "        let {} = updates.{}.is_some();\n",
            has_many::local(&field.name, "changed"),
            field.name
        ));
    }
    for (field, _info) in &has_manys {
        code.push_str(&format!(
            "        let {} = updates.{}.is_some();\n",
            has_many::local(&field.name, "changed"),
            field.name
        ));
    }
    if !junctions.is_empty() || !has_manys.is_empty() {
        code.push('\n');
    }
    let writes = has_many_writes(entity);
    linked_ids::emit_self_listing_check(code, entity, &Listed::Update);
    linked_ids::emit_listed_ids_check(code, entity, entities, &Listed::Update);
    has_many::emit_dropped_children(code, &writes);
    emit_integer_range_checks(code, entity, IntegerSource::Updates, Skipped::Stored, db_error);
    emit_nan_checks(code, entity, FloatSource::Updates, Skipped::Stored, db_error);

    // Apply updates
    code.push_str("        updates.apply(&mut current);\n");
    code.push_str("        let active = current.to_active_model()?;\n\n");

    // Re-persist
    emit_begin(code);
    code.push_str("        active\n");
    code.push_str("            .update(&txn)\n");
    code.push_str("            .await\n");
    code.push_str("            .map_err(|e| AppError::DbError(e.to_string()))?;\n");

    // Conditional junction sync
    for (field, info) in &junctions {
        let jt = junction_table_name(&snake, member_name(&field.name), info.junction.as_deref());
        let src_col = junction_source_col(&snake);
        let target_snake = to_snake_case(&info.target);
        let is_self_ref = entity.name == info.target;
        let tgt_col = junction_target_col(&snake, &target_snake, is_self_ref);

        code.push_str(&format!("        if {} {{\n", has_many::local(&field.name, "changed")));
        code.push_str(&format!(
            "            self.sync_junction(&txn, \"{jt}\", \"{src_col}\", \"{tgt_col}\", id, &current.{fname}).await?;\n",
            fname = field.name
        ));
        code.push_str("        }\n");
    }

    // Conditional has_many reverse sync
    has_many::emit_update_children(code, entity, &writes, |f| format!("&current.{f}"), Some("&txn"));
    emit_commit(code);

    code.push_str(&format!("        let result = self.get_{snake}(id).await?;\n"));
    code.push_str(&format!(
        "        self.emit_change(ChangeOp::Updated, EntityKind::{entity_kind}, id.to_string());\n\n"
    ));

    // Hook: after_update
    code.push_str("        hooks::after_update(self, &result).await?;\n");
    code.push_str("        Ok(result)\n");
    code.push_str("    }\n\n");
}

fn generate_delete(code: &mut String, entity: &EntityDef) {
    let name = &entity.name;
    let snake = to_snake_case(name);
    let module = rust_ident(&snake);
    let not_found = not_found_variant(name);
    let entity_kind = entity_kind_variant(name);

    code.push_str(&format!("    pub async fn delete_{snake}(&self, id: &str) -> Result<(), AppError> {{\n"));

    // Hook: before_delete
    code.push_str("        hooks::before_delete(self, id).await?;\n\n");

    code.push_str(&format!("        let existing = {module}::Entity::find_by_id(id)\n"));
    code.push_str("            .one(self.db())\n");
    code.push_str("            .await\n");
    code.push_str("            .map_err(|e| AppError::DbError(e.to_string()))?\n");
    code.push_str(&format!("            .ok_or_else(|| AppError::{not_found}(id.to_string()))?;\n\n"));
    code.push_str(&format!("        let active: {module}::ActiveModel = existing.into();\n"));
    code.push_str("        active\n");
    code.push_str("            .delete(self.db())\n");
    code.push_str("            .await\n");
    code.push_str("            .map_err(|e| AppError::DbError(e.to_string()))?;\n\n");
    code.push_str(&format!(
        "        self.emit_change(ChangeOp::Deleted, EntityKind::{entity_kind}, id.to_string());\n\n"
    ));

    // Hook: after_delete
    code.push_str("        hooks::after_delete(self, id).await?;\n");
    code.push_str("        Ok(())\n");
    code.push_str("    }\n\n");
}

// ─── populate_relations ──────────────────────────────────────────────────────

fn generate_populate_relations(code: &mut String, entity: &EntityDef) {
    let name = &entity.name;
    let snake = to_snake_case(name);

    code.push_str(&format!("    pub(crate) async fn populate_{snake}_relations(\n"));
    code.push_str(&format!("        &self,\n        record: &mut crate::schema::{name},\n"));
    code.push_str("    ) -> Result<(), crate::schema::AppError> {\n");

    // has_many fields: load child IDs via SeaORM query
    for (field, info) in entity.has_many_relations() {
        if let Some(ref fk) = info.foreign_key {
            let target_module = rust_ident(&to_snake_case(&info.target));
            let fk_col = to_pascal_case(member_name(fk));

            code.push_str(&format!("        record.{} = {{\n", field.name));
            code.push_str(&format!("            use crate::persistence::db::entities::{target_module};\n"));
            code.push_str(
                "            // sqlite-only: child ids sort in byte order under SQLite's default BINARY collation.\n",
            );
            code.push_str(&format!("            let children = {target_module}::Entity::find()\n"));
            code.push_str(&format!("                .filter({target_module}::Column::{fk_col}.eq(&record.id))\n"));
            // A record is never its own child: a root that is its own parent
            // (a required foreign key has to point somewhere) would otherwise
            // list itself, and an update that left it out would drop it. The
            // markdown backend skips it the same way.
            if info.target == entity.name {
                code.push_str(&format!("                .filter({target_module}::Column::Id.ne(&record.id))\n"));
            }
            // Same reason the list orders: this is a multi-row SELECT, so
            // without an `ORDER BY` the engine picks the order and a
            // `has_many` field comes back shuffled between calls. The markdown
            // backend returns these in id order too.
            code.push_str(&format!("                .order_by_asc({target_module}::Column::Id)\n"));
            code.push_str("                .all(self.db())\n");
            code.push_str("                .await\n");
            code.push_str("                .map_err(|e| crate::schema::AppError::DbError(e.to_string()))?;\n");
            code.push_str("            children.into_iter().map(|m| m.id).collect()\n");
            code.push_str("        };\n");
        }
    }

    // many_to_many fields: load junction IDs
    for (field, info) in entity.junction_relations() {
        let jt = junction_table_name(&snake, member_name(&field.name), info.junction.as_deref());
        let src_col = junction_source_col(&snake);
        let target_snake = to_snake_case(&info.target);
        let is_self_ref = entity.name == info.target;
        let tgt_col = junction_target_col(&snake, &target_snake, is_self_ref);

        code.push_str(&format!(
            "        record.{fname} = self\n            .load_junction_ids(\"{jt}\", \"{src_col}\", \"{tgt_col}\", &record.id)\n            .await?;\n",
            fname = field.name,
        ));
    }

    code.push_str("        Ok(())\n");
    code.push_str("    }\n\n");
}

// ─── has_many helper: set_parent ─────────────────────────────────────────────

/// `try_insert_{snake}`: insert a new row on `conn` (the create's
/// transaction), answering `false` when its id is already taken. Only a
/// unique violation with a row under that id means the id is taken; a
/// violation of some other unique constraint the consumer declared is a
/// `DbError` like any other.
fn generate_try_insert_helper(code: &mut String, entity: &EntityDef) {
    let name = &entity.name;
    let snake = to_snake_case(name);
    let module = rust_ident(&snake);

    code.push_str(&format!(
        "    async fn try_insert_{snake}<C: ::sea_orm::ConnectionTrait>(&self, conn: &C, record: &{name}) -> Result<bool, AppError> {{\n"
    ));
    code.push_str("        let active = record.to_active_model()?;\n");
    code.push_str("        match active.insert(conn).await {\n");
    code.push_str("            Ok(_) => Ok(true),\n");
    code.push_str(
        "            Err(e) if matches!(e.sql_err(), Some(::sea_orm::SqlErr::UniqueConstraintViolation(_))) => {\n",
    );
    code.push_str(
        "                // sqlite-only: a failed INSERT leaves an SQLite transaction usable; Postgres aborts it.\n",
    );
    code.push_str(&format!("                let taken = {module}::Entity::find_by_id(record.id.as_str())\n"));
    code.push_str("                    .one(conn)\n");
    code.push_str("                    .await\n");
    code.push_str("                    .map_err(|e| AppError::DbError(e.to_string()))?\n");
    code.push_str("                    .is_some();\n");
    code.push_str("                if taken { Ok(false) } else { Err(AppError::DbError(e.to_string())) }\n");
    code.push_str("            }\n");
    code.push_str("            Err(e) => Err(AppError::DbError(e.to_string())),\n");
    code.push_str("        }\n");
    code.push_str("    }\n\n");
}

/// `set_{snake}_parent`: point a child's FK at a parent with one `UPDATE`
/// on `conn` (the write's transaction). A required FK takes a parent, an
/// optional one `None` to clear it.
fn generate_set_parent_helper(code: &mut String, entity: &EntityDef, fk: &str, fk_required: bool) {
    let snake = to_snake_case(&entity.name);
    // Built by sea_query, which quotes every identifier for the connection's
    // backend: a column named after a keyword (`r#in` is the column `in`) is
    // not an SQL identifier bare.
    let id = entity.id_field().map_or("id", |f| member_name(&f.name));
    let alias = |name: &str| format!("::sea_orm::sea_query::Alias::new({name:?})");
    let parent = if fk_required { "parent_id.to_string()" } else { "parent_id.map(str::to_string)" };

    code.push_str(&format!("    async fn set_{snake}_parent<C: ::sea_orm::ConnectionTrait>(\n"));
    code.push_str("        &self,\n");
    code.push_str("        conn: &C,\n");
    code.push_str("        child_id: &str,\n");
    code.push_str(&format!("        parent_id: {},\n", has_many::parent_param_type(fk_required)));
    code.push_str("    ) -> Result<(), AppError> {\n");
    code.push_str("        let update = ::sea_orm::sea_query::Query::update()\n");
    code.push_str(&format!("            .table({})\n", alias(&entity.table)));
    code.push_str(&format!("            .value({}, {parent})\n", alias(member_name(fk))));
    code.push_str(&format!("            .and_where(::sea_orm::sea_query::Expr::col({}).eq(child_id))\n", alias(id)));
    code.push_str("            .to_owned();\n");
    code.push_str("        let stmt = conn.get_database_backend().build(&update);\n");
    code.push_str("        conn.execute(stmt)\n");
    code.push_str("            .await\n");
    code.push_str("            .map_err(|e| AppError::DbError(e.to_string()))?;\n");
    code.push_str("        Ok(())\n");
    code.push_str("    }\n\n");
}

/// `{snake}_exists`: whether a record with this id exists, for the checks
/// on the ids a write lists (`linked_ids`). It reads through `self.db()`:
/// those checks run before the write's transaction opens.
fn generate_exists_helper(code: &mut String, entity: &EntityDef) {
    let snake = to_snake_case(&entity.name);
    let module = rust_ident(&snake);

    code.push_str(&format!(
        "    pub(crate) async fn {}(&self, id: &str) -> Result<bool, AppError> {{\n",
        linked_ids::exists_helper(&entity.name)
    ));
    code.push_str(&format!("        Ok({module}::Entity::find_by_id(id)\n"));
    code.push_str("            .one(self.db())\n");
    code.push_str("            .await\n");
    code.push_str("            .map_err(|e| AppError::DbError(e.to_string()))?\n");
    code.push_str("            .is_some())\n");
    code.push_str("    }\n\n");
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// The SeaORM `Column` variant for an entity's primary key, e.g. `Id`.
/// Reading the field keeps a renamed id correct. The store stage refuses an
/// entity with no `#[ontology(id)]` field, so the `Id` fallback is never
/// emitted.
fn id_column(entity: &EntityDef) -> String {
    entity.id_field().map(|f| to_pascal_case(member_name(&f.name))).unwrap_or_else(|| "Id".to_string())
}

/// Map entity name to its `AppError::*NotFound` variant.
fn not_found_variant(entity_name: &str) -> String {
    format!("{entity_name}NotFound")
}

/// Map entity name to its `EntityKind::*` variant.
fn entity_kind_variant(entity_name: &str) -> String {
    entity_name.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::model::{EntityDef, FieldDef, FieldRole, FieldType, RelationInfo, RelationKind};

    fn make_role_entity() -> EntityDef {
        EntityDef {
            name: "Role".to_string(),
            directory: "role".to_string(),
            table: "roles".to_string(),
            type_name: "role".to_string(),
            prefix: "role".to_string(),
            id_strategy: None,
            fields: vec![
                FieldDef::new("id", FieldType::String, FieldRole::Id),
                FieldDef::new("body", FieldType::String, FieldRole::Body),
            ],
            doc: String::new(),
        }
    }

    fn make_node_entity() -> EntityDef {
        EntityDef {
            name: "Node".to_string(),
            directory: "node".to_string(),
            table: "nodes".to_string(),
            type_name: "node".to_string(),
            prefix: "node".to_string(),
            id_strategy: None,
            fields: vec![
                FieldDef::new("id", FieldType::String, FieldRole::Id),
                FieldDef::new("name", FieldType::String, FieldRole::Plain),
                FieldDef::new(
                    "parent_id",
                    FieldType::OptionString,
                    FieldRole::Relation(RelationInfo {
                        kind: RelationKind::BelongsTo,
                        target: "Node".to_string(),
                        junction: None,
                        foreign_key: None,
                    }),
                ),
                FieldDef::new(
                    "contains",
                    FieldType::VecString,
                    FieldRole::Relation(RelationInfo {
                        kind: RelationKind::HasMany,
                        target: "Node".to_string(),
                        junction: None,
                        foreign_key: Some("parent_id".to_string()),
                    }),
                ),
                FieldDef::new(
                    "fulfills",
                    FieldType::VecString,
                    FieldRole::Relation(RelationInfo {
                        kind: RelationKind::ManyToMany,
                        target: "Requirement".to_string(),
                        junction: Some("node_fulfills".to_string()),
                        foreign_key: None,
                    }),
                ),
                FieldDef::new("body", FieldType::String, FieldRole::Body),
            ],
            doc: String::new(),
        }
    }

    #[test]
    fn test_simple_entity_crud() {
        let entity = make_role_entity();
        let mut code = String::new();
        generate_crud_impl(&mut code, &entity, std::slice::from_ref(&entity), &[], &IdStrategy::Provided);

        assert!(code.contains("fn list_roles("));
        assert!(code.contains("fn get_role("));
        assert!(code.contains("fn create_role("));
        assert!(code.contains("fn update_role("));
        assert!(code.contains("fn delete_role("));
        // No populate_relations for simple entities
        assert!(!code.contains("populate_role_relations"));
    }

    #[test]
    fn test_complex_entity_has_populate_relations() {
        let entity = make_node_entity();
        let mut code = String::new();
        generate_crud_impl(&mut code, &entity, std::slice::from_ref(&entity), &[], &IdStrategy::Provided);

        assert!(code.contains("populate_node_relations"));
        assert!(code.contains("self.sync_junction(&txn, \"node_fulfills\""));
        assert!(code.contains("set_node_parent"));
    }

    #[test]
    fn test_junction_sync_in_create() {
        let entity = make_node_entity();
        let mut code = String::new();
        generate_crud_impl(&mut code, &entity, std::slice::from_ref(&entity), &[], &IdStrategy::Provided);

        // Create writes the junction and the children from the record
        let create = method(&code, "create_node");
        assert!(create.contains("&id, &record.fulfills).await?;"), "{create}");
        assert!(create.contains("for child_id in &record.contains {"), "{create}");
    }

    #[test]
    fn test_list_has_pagination_params() {
        let entity = make_role_entity();
        let mut code = String::new();
        generate_crud_impl(&mut code, &entity, std::slice::from_ref(&entity), &[], &IdStrategy::Provided);

        // Generated list_* must accept an order and optional limit / offset for SQL-level pagination
        assert!(
            code.contains(
                "fn list_roles(&self, order: &[OrderBy<RoleSortField>], limit: Option<u64>, offset: Option<u64>)"
            ),
            "list_roles should have order and pagination params, got:\n{code}"
        );
        // And wire them into the SeaORM query
        assert!(code.contains("query = query.limit(l);"), "missing .limit() call");
        assert!(code.contains("query = query.offset(o);"), "missing .offset() call");
        assert!(
            code.contains("if let Some(l) = limit.or(offset.map(|_| i64::MAX as u64)) {"),
            "an offset alone still emits a LIMIT, which SQLite requires: {code}"
        );
    }

    #[test]
    fn list_clamps_limit_and_offset_to_i64_before_binding() {
        let code = crud(&make_role_entity(), &IdStrategy::Provided);
        let list = method(&code, "list_roles");
        let clamp_limit = list.find("let limit = limit.map(|l| l.min(i64::MAX as u64));").expect("limit clamped");
        let clamp_offset = list.find("let offset = offset.map(|o| o.min(i64::MAX as u64));").expect("offset clamped");
        let bind = list.find("query = query.limit(l);").expect("limit bound");
        assert!(clamp_limit < bind && clamp_offset < bind, "clamped before binding: {list}");
    }

    #[test]
    fn the_list_orders_through_its_query_helper_before_the_page() {
        let code = crud(&make_role_entity(), &IdStrategy::Provided);
        let list = method(&code, "list_roles");
        let order = list.find("let mut query = order_roles_query(role::Entity::find(), order);").expect("ordered");
        assert!(order < list.find("query = query.limit(l);").unwrap(), "{list}");
    }

    #[test]
    fn the_order_helper_maps_every_sort_field_to_its_column_with_explicit_nulls() {
        let mut entity = make_node_entity();
        entity.fields.push(FieldDef::new("r#type", FieldType::OptionString, FieldRole::Plain));
        let code = crud(&entity, &IdStrategy::Provided);
        let helper = &code[code.find("pub fn order_nodes_query(").expect("a module-level helper")..];
        assert!(
            helper.starts_with(
                "pub fn order_nodes_query(mut query: ::sea_orm::Select<node::Entity>, order: &[OrderBy<NodeSortField>]) -> ::sea_orm::Select<node::Entity> {"
            ),
            "{helper}"
        );
        for expected in [
            "for key in ontogen_core::order::effective(order) {",
            "NodeSortField::Id => node::Column::Id,",
            "NodeSortField::Name => node::Column::Name,",
            "NodeSortField::Type => node::Column::Type,",
            "ontogen_core::order::Direction::Asc => (::sea_orm::sea_query::Order::Asc, ::sea_orm::sea_query::NullOrdering::First),",
            "ontogen_core::order::Direction::Desc => (::sea_orm::sea_query::Order::Desc, ::sea_orm::sea_query::NullOrdering::Last),",
            "string keys sort in byte order under SQLite's default BINARY collation.",
            "query = query.order_by_with_nulls(column, direction, nulls);",
        ] {
            assert!(helper.contains(expected), "missing `{expected}`:\n{helper}");
        }
        assert!(!helper.contains("ParentId") && !helper.contains("Body"), "relations and body do not sort: {helper}");
    }

    #[test]
    fn a_nan_is_refused_after_the_hook_and_before_anything_is_written() {
        let mut entity = make_node_entity();
        entity.fields.push(FieldDef::new("weight", FieldType::F32, FieldRole::Plain));
        entity.fields.push(FieldDef::new("high", FieldType::OptionF64, FieldRole::Plain));
        let code = crud(&entity, &IdStrategy::Provided);
        let refusal = r#"return Err(AppError::DbError("Node.weight: NaN cannot be stored".to_string()));"#;

        let create = method(&code, "create_node");
        let check = create.find("if record.weight.is_nan() {").unwrap_or_else(|| panic!("create checks: {create}"));
        assert!(create.contains("if record.high.is_some_and(f64::is_nan) {"), "{create}");
        assert!(create.contains(refusal), "{create}");
        assert!(check > create.find("hooks::before_create").unwrap(), "after the hook: {create}");
        assert!(check < create.find("let id = if record.id.trim().is_empty()").unwrap(), "before the insert: {create}");

        let update = method(&code, "update_node");
        let check = update.find("if updates.weight.is_some_and(f32::is_nan) {").unwrap_or_else(|| panic!("{update}"));
        assert!(update.contains("if updates.high.flatten().is_some_and(f64::is_nan) {"), "{update}");
        assert!(check > update.find("hooks::before_update").unwrap(), "after the hook: {update}");
        assert!(check < update.find(".update(&txn)").unwrap(), "before the record write: {update}");
        assert!(!crud(&make_role_entity(), &IdStrategy::Provided).contains("is_nan"), "no float, no check");
    }

    /// A skipped field has a column, so a create refuses a NaN in a skipped
    /// float as in any other; `NodeUpdate` has no skipped field, so an
    /// update has none to check.
    #[test]
    fn a_nan_in_a_skipped_float_is_refused_on_create() {
        let mut entity = make_node_entity();
        entity.fields.push(FieldDef::new("cached", FieldType::F64, FieldRole::Skip));
        entity.fields.push(FieldDef::new("maybe_cached", FieldType::OptionF32, FieldRole::Skip));
        let code = crud(&entity, &IdStrategy::Provided);
        let create = method(&code, "create_node");
        assert!(create.contains("if record.cached.is_nan() {"), "{create}");
        assert!(create.contains("if record.maybe_cached.is_some_and(f32::is_nan) {"), "{create}");
        assert!(
            create.contains(r#"return Err(AppError::DbError("Node.cached: NaN cannot be stored".to_string()));"#),
            "{create}"
        );
        assert!(!method(&code, "update_node").contains("cached"), "{code}");
    }

    #[test]
    fn a_record_is_not_its_own_has_many_child() {
        let code = crud(&make_node_entity(), &IdStrategy::Provided);
        let populate = method(&code, "populate_node_relations");
        assert!(populate.contains(".filter(node::Column::ParentId.eq(&record.id))"), "{populate}");
        assert!(populate.contains(".filter(node::Column::Id.ne(&record.id))"), "{populate}");
    }

    #[test]
    fn test_update_tracks_junction_changes() {
        let entity = make_node_entity();
        let mut code = String::new();
        generate_crud_impl(&mut code, &entity, std::slice::from_ref(&entity), &[], &IdStrategy::Provided);

        // Update should track which junction fields changed
        assert!(code.contains("let fulfills_changed = updates.fulfills.is_some()"));
        assert!(code.contains("let contains_changed = updates.contains.is_some()"));
        assert!(code.contains("if fulfills_changed {"));
        assert!(code.contains("if contains_changed {"));
    }

    fn strategies() -> [IdStrategy; 3] {
        [IdStrategy::Provided, IdStrategy::SlugFromField("name".into()), IdStrategy::Uuid]
    }

    /// The node fixture with its `parent_id` foreign key made required.
    fn make_node_with_required_parent() -> EntityDef {
        let mut entity = make_node_entity();
        entity.fields.iter_mut().find(|f| f.name == "parent_id").expect("parent_id").field_type = FieldType::String;
        entity
    }

    fn crud(entity: &EntityDef, strategy: &IdStrategy) -> String {
        let mut code = String::new();
        generate_crud_impl(&mut code, entity, std::slice::from_ref(entity), &[], strategy);
        code
    }

    fn method<'a>(code: &'a str, name: &str) -> &'a str {
        let start = code
            .find(&format!("fn {name}("))
            .or_else(|| code.find(&format!("fn {name}<")))
            .unwrap_or_else(|| panic!("no fn {name}"));
        let rest = &code[start..];
        &rest[..rest.find("\n    }\n").map(|e| e + 6).unwrap_or(rest.len())]
    }

    #[test]
    fn a_provided_create_requires_an_id_and_refuses_a_duplicate() {
        let code = crud(&make_role_entity(), &IdStrategy::Provided);
        let create = method(&code, "create_role");
        assert!(create.contains("let id = if record.id.trim().is_empty() {"), "{create}");
        assert!(
            create.contains(
                "return Err(AppError::RoleIdRequired(\"this store requires the caller to supply an id\".to_string()));"
            ),
            "{create}"
        );
        assert!(
            create
                .contains("ontogen_core::id::validate_id(&record.id).map_err(|e| AppError::DbError(e.to_string()))?;"),
            "{create}"
        );
        assert!(create.contains("return Err(AppError::RoleAlreadyExists(record.id));"), "{create}");
        assert!(!create.contains("candidates"), "nothing is derived under Provided: {create}");
    }

    #[test]
    fn a_slug_create_derives_and_probes() {
        let code = crud(&make_role_entity(), &IdStrategy::SlugFromField("name".into()));
        let create = method(&code, "create_role");
        assert!(create.contains("let base = ontogen_core::id::slugify(&record.name);"), "{create}");
        assert!(
            create.contains(
                "return Err(AppError::RoleIdRequired(\"field \\\"name\\\" produced an empty slug\".to_string()));"
            ),
            "the reason is the markdown runtime's wording: {create}"
        );
        assert!(create.contains("for candidate in ontogen_core::id::candidates(&base) {"), "{create}");
        assert!(create.contains("role::Entity::find_by_id(candidate.as_str())"), "probes before inserting: {create}");
        assert!(create.contains("if self.try_insert_role(&txn, &record).await? {"), "{create}");
    }

    #[test]
    fn a_uuid_create_derives_a_uuid() {
        let code = crud(&make_role_entity(), &IdStrategy::Uuid);
        let create = method(&code, "create_role");
        assert!(create.contains("let base = ontogen_core::id::new_uuid();"), "{create}");
        assert!(!create.contains("IdRequired"), "a uuid is never empty: {create}");
    }

    #[test]
    fn try_insert_tells_a_taken_id_from_other_unique_violations() {
        let code = crud(&make_role_entity(), &IdStrategy::Provided);
        let helper = method(&code, "try_insert_role");
        assert!(helper.contains("let active = record.to_active_model()?;"), "{helper}");
        assert!(helper.contains("Some(::sea_orm::SqlErr::UniqueConstraintViolation(_))"), "{helper}");
        assert!(helper.contains("role::Entity::find_by_id(record.id.as_str())"), "{helper}");
        assert!(helper.contains("Err(AppError::DbError(e.to_string()))"), "{helper}");
    }

    #[test]
    fn a_has_many_update_clears_dropped_children() {
        let code = crud(&make_node_entity(), &IdStrategy::Provided);
        let update = method(&code, "update_node");
        let dropped = update.find("let contains_dropped").expect("dropped computed");
        assert!(
            dropped < update.find("updates.apply(&mut current)").unwrap(),
            "computed from the stored list: {update}"
        );
        assert!(update.contains("self.set_node_parent(&txn, child_id, None).await?;"), "{update}");
        assert!(!update.contains("ParentRequired"), "an optional FK can be cleared: {update}");
        assert!(method(&code, "set_node_parent").contains("parent_id: Option<&str>,"));
    }

    #[test]
    fn a_required_foreign_key_refuses_the_drop_before_writing() {
        let code = crud(&make_node_with_required_parent(), &IdStrategy::Provided);
        let update = method(&code, "update_node");
        let refusal = update.find("return Err(AppError::NodeParentRequired(child_id.clone()));").expect("refusal");
        assert!(refusal < update.find(".update(&txn)").unwrap(), "refused before the record write: {update}");
        assert!(update.contains("self.set_node_parent(&txn, child_id, id).await?;"), "{update}");
        assert!(!update.contains("None).await"), "{update}");

        let helper = method(&code, "set_node_parent");
        assert!(helper.contains("parent_id: &str,"), "{helper}");
        assert!(
            helper.contains(r#".value(::sea_orm::sea_query::Alias::new("parent_id"), parent_id.to_string())"#),
            "{helper}"
        );
        assert!(method(&code, "create_node").contains("self.set_node_parent(&txn, child_id, &id).await?;"));
    }

    #[test]
    fn a_missing_child_is_refused_before_anything_is_written() {
        for entity in [make_node_entity(), make_node_with_required_parent()] {
            let code = crud(&entity, &IdStrategy::SlugFromField("name".into()));

            let create = method(&code, "create_node");
            let check = create.find("if !self.node_exists(child_id).await? {").expect("create checks");
            assert!(create.contains("for child_id in &record.contains {"), "{create}");
            assert!(check > create.find("hooks::before_create").unwrap(), "after the hook: {create}");
            assert!(
                check < create.find("let id = if record.id.trim().is_empty()").unwrap(),
                "before the insert: {create}"
            );

            let update = method(&code, "update_node");
            let check = update.find("if !self.node_exists(child_id).await? {").expect("update checks");
            assert!(update.contains("for child_id in updates.contains.iter().flatten() {"), "{update}");
            assert!(check > update.find("hooks::before_update").unwrap(), "after the hook: {update}");
            assert!(check < update.find("let contains_dropped").unwrap(), "before the drop check: {update}");
            assert!(check < update.find(".update(&txn)").unwrap(), "before the record write: {update}");
            assert!(update.contains("return Err(AppError::NodeNotFound(child_id.clone()));"), "{update}");

            let helper = method(&code, "node_exists");
            assert!(helper.contains("node::Entity::find_by_id(id)"), "{helper}");
        }
        assert!(!crud(&make_role_entity(), &IdStrategy::Provided).contains("_exists("), "no has_many, no check");
    }

    #[test]
    fn every_write_of_a_create_or_update_is_in_one_transaction() {
        let code = crud(&make_node_entity(), &IdStrategy::SlugFromField("name".into()));
        for (op, first_write) in
            [("create_node", "let id = if record.id.trim().is_empty()"), ("update_node", "active\n")]
        {
            let body = method(&code, op);
            let at = |needle: &str| body.find(needle).unwrap_or_else(|| panic!("{op}: missing `{needle}`:\n{body}"));
            let begin = at("let txn = self.db().begin().await.map_err(|e| AppError::DbError(e.to_string()))?;");
            let commit = at("txn.commit().await.map_err(|e| AppError::DbError(e.to_string()))?;");
            assert!(at("hooks::before_") < begin && begin < at(first_write), "{op}: {body}");
            assert!(at("self.sync_junction(&txn, ") < commit, "{op}: {body}");
            assert!(at("self.set_node_parent(&txn, child_id, ") < commit, "{op}: {body}");
            let after = at("hooks::after_");
            assert!(commit < at(".await?;\n        self.emit_change(") && commit < after, "{op}: {body}");
            let between = &body[begin + body[begin..].find('\n').unwrap()..commit];
            assert!(!between.contains("self.db()"), "{op}: every statement goes through the transaction: {between}");
        }
        let create = method(&code, "create_node");
        assert!(create.contains(".one(&txn)"), "the probe reads through the transaction: {create}");
        assert!(create.contains("if self.try_insert_node(&txn, &record).await? {"), "{create}");
        assert!(method(&code, "update_node").contains(".update(&txn)"));
        for helper in ["try_insert_node", "set_node_parent"] {
            let helper = method(&code, helper);
            assert!(helper.contains("<C: ::sea_orm::ConnectionTrait>(") && helper.contains("conn: &C,"), "{helper}");
            assert!(!helper.contains("self.db()"), "{helper}");
        }
        let simple = crud(&make_role_entity(), &IdStrategy::Provided);
        assert!(method(&simple, "create_role").contains("let txn = self.db().begin()"), "{simple}");
    }

    #[test]
    fn a_many_to_many_target_is_checked_before_anything_is_written() {
        let requirement = EntityDef {
            name: "Requirement".to_string(),
            directory: "requirement".to_string(),
            table: "requirements".to_string(),
            type_name: "requirement".to_string(),
            prefix: "req".to_string(),
            id_strategy: None,
            fields: vec![FieldDef::new("id", FieldType::String, FieldRole::Id)],
            doc: String::new(),
        };
        let entities = [make_node_entity(), requirement];
        for (entity, expected) in entities.iter().zip([true, true]) {
            let mut code = String::new();
            generate_crud_impl(&mut code, entity, &entities, &[], &IdStrategy::Provided);
            let exists = format!("pub(crate) async fn {}(&self, id: &str)", linked_ids::exists_helper(&entity.name));
            assert_eq!(code.contains(&exists), expected, "{code}");
        }
        let mut code = String::new();
        generate_crud_impl(&mut code, &entities[0], &entities, &[], &IdStrategy::Provided);
        for op in ["create_node", "update_node"] {
            let body = method(&code, op);
            let check =
                body.find("if !self.requirement_exists(target_id).await? {").unwrap_or_else(|| panic!("{body}"));
            assert!(body.contains("return Err(AppError::RequirementNotFound(target_id.clone()));"), "{body}");
            assert!(check < body.find("let txn = self.db().begin()").unwrap(), "{op}: before the transaction: {body}");
            assert!(body.find("ParentCycle").unwrap() < check, "{op}: after the self-listing check: {body}");
        }
    }

    #[test]
    fn a_value_outside_i64_is_refused_before_the_id_is_derived() {
        let mut entity = make_node_entity();
        entity.fields.push(FieldDef::new("seq", FieldType::Other("u64".into()), FieldRole::Plain));
        entity.fields.push(FieldDef::new("hidden", FieldType::Other("usize".into()), FieldRole::Skip));
        let code = crud(&entity, &IdStrategy::SlugFromField("name".into()));

        let create = method(&code, "create_node");
        let check = create.find("if let Some(v) = Some(record.seq).filter(|v| i64::try_from(*v).is_err()) {");
        let check = check.unwrap_or_else(|| panic!("{create}"));
        assert!(
            create
                .contains(r#"return Err(AppError::DbError(format!("Node.seq: value {v} is out of range for i64")));"#),
            "{create}"
        );
        assert!(create.contains("Some(record.hidden).filter("), "a skipped field has a column: {create}");
        assert!(check < create.find("if record.").unwrap_or(usize::MAX), "{create}");
        assert!(check < create.find("let id = if record.id.trim().is_empty()").unwrap(), "{create}");

        let update = method(&code, "update_node");
        let check = update.find("if let Some(v) = updates.seq.filter(|v| i64::try_from(*v).is_err()) {");
        let check = check.unwrap_or_else(|| panic!("{update}"));
        assert!(check < update.find("updates.apply(&mut current)").unwrap(), "{update}");
        assert!(!update.contains("hidden"), "{update}");
    }

    /// Syntax check: verify the generated `impl Store` block parses as valid
    /// Rust for simple and relation-heavy entities under every strategy.
    #[test]
    fn generated_code_is_valid_rust() {
        for entity in [make_role_entity(), make_node_entity(), make_node_with_required_parent()] {
            for strategy in strategies() {
                let code = crud(&entity, &strategy);
                syn::parse_file(&code).unwrap_or_else(|e| {
                    panic!(
                        "generate_crud_impl produced invalid Rust for '{}' under {strategy:?}: {e}\n--- code ---\n{code}",
                        entity.name
                    )
                });
            }
        }
    }
}
