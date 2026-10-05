//! Markdown CRUD emission: the per-op bodies of the generated `impl Store`
//! block, wired to the `markdown-store` runtime crate via the per-entity
//! `{Entity}Frontmatter` boundary (ADR 0001).
//!
//! Lifecycle parity with the SeaORM emission is the load-bearing contract:
//! method signatures, hook call sites, `*_changed` tracking, `emit_change`
//! points, and the populate-before-hooks ordering are identical — only the
//! persistence primitives differ. The spec for the minimal entity is
//! `tests/golden/markdown-backend/store/note.rs.golden`, enforced by the
//! conformance test once the harness lands.

use crate::ir::IdStrategy;
use crate::schema::model::EntityDef;
use crate::store::gen_order::sort_field_type;
use crate::store::has_many::{self, has_many_writes};
use crate::store::helpers::{pluralize, to_snake_case};
use crate::store::int_range::{IntegerSource, emit_integer_range_checks};
use crate::store::linked_ids::{self, Listed};
use crate::store::nan::{FloatSource, Skipped, emit_nan_checks};

// ─── Public API ──────────────────────────────────────────────────────────────

/// Generate the complete `impl Store { ... }` block for the markdown backend.
pub fn generate_crud_impl(code: &mut String, entity: &EntityDef, entities: &[EntityDef], id_strategy: &IdStrategy) {
    let has_relations = entity.junction_relations().next().is_some() || entity.has_many_relations().next().is_some();

    code.push_str("impl Store {\n");

    generate_list(code, entity, has_relations);
    generate_count(code, entity);
    generate_get(code, entity, has_relations);
    generate_create(code, entity, entities, id_strategy);
    generate_update(code, entity, entities);
    generate_delete(code, entity);

    if has_relations {
        generate_populate_relations(code, entity);
    }

    // has_many reverse helpers (e.g., set_node_parent) — read-mutate-rewrite
    // replaces SeaORM's raw-SQL fast path.
    for hm in &has_many_writes(entity) {
        generate_set_parent_helper(code, entity, hm.fk, hm.fk_required);
    }
    if linked_ids::needs_exists_helper(entity, entities) {
        generate_exists_helper(code, entity);
    }

    code.push_str("}\n");
}

// ─── Shared snippets ─────────────────────────────────────────────────────────

fn fm_type(name: &str) -> String {
    format!("{name}Frontmatter")
}

fn fields_const(snake: &str) -> String {
    format!("{}_FM_FIELDS", snake.to_uppercase())
}

pub(super) fn dir_const(snake: &str) -> String {
    format!("{}_DIR", pluralize(snake).to_uppercase())
}

pub(super) fn type_const(snake: &str) -> String {
    format!("{}_TYPE", snake.to_uppercase())
}

/// The runtime's typed view of this entity's records, which stamps the OKF
/// `type` on write and filters a flat vault by it on read.
fn records(snake: &str) -> String {
    format!("entity({}, {})", dir_const(snake), type_const(snake))
}

/// The `into_{snake}` call: bodyful entities thread the document body
/// through; bodyless entities take only the id.
fn into_call(snake: &str, entity: &EntityDef, id_expr: &str, doc_var: &str) -> String {
    if entity.body_field().is_some() {
        format!("fm.into_{snake}({id_expr}, {doc_var}.body().to_string())")
    } else {
        format!("fm.into_{snake}({id_expr})")
    }
}

// ─── Individual CRUD methods ─────────────────────────────────────────────────

fn generate_list(code: &mut String, entity: &EntityDef, has_relations: bool) {
    let name = &entity.name;
    let snake = to_snake_case(name);
    let plural = pluralize(&snake);
    let fm = fm_type(name);
    let records = records(&snake);
    let sort_field = sort_field_type(entity);

    code.push_str(&format!(
        "    pub async fn list_{plural}(&self, order: &[OrderBy<{sort_field}>], limit: Option<u64>, offset: Option<u64>) -> Result<Vec<{name}>, AppError> {{\n"
    ));
    code.push_str(&format!("        let mut {plural} = Vec::new();\n"));
    code.push_str(&format!("        for (id, doc) in self.vault().{records}.read_all().map_err(AppError::from)? {{\n"));
    code.push_str(&format!("            let fm: {fm} = doc.deserialize().map_err(AppError::from)?;\n"));
    code.push_str(&format!("            {plural}.push({});\n", into_call(&snake, entity, "id", "doc")));
    code.push_str("        }\n");
    // The walk's path order is not the list order: a nested layout's paths
    // do not sort as its ids do (ADR 0006 §5). The sort precedes the page.
    code.push_str(&format!("        sort_{plural}(&mut {plural}, order);\n"));
    code.push_str("        let offset = offset.unwrap_or(0) as usize;\n");
    code.push_str("        let limit = limit.map(|l| l as usize).unwrap_or(usize::MAX);\n");

    if has_relations {
        code.push_str(&format!(
            "        let mut {plural}: Vec<{name}> = {plural}.into_iter().skip(offset).take(limit).collect();\n"
        ));
        code.push_str(&format!("        for entity in &mut {plural} {{\n"));
        code.push_str(&format!("            self.populate_{snake}_relations(entity).await?;\n"));
        code.push_str("        }\n");
        code.push_str(&format!("        Ok({plural})\n"));
    } else {
        code.push_str(&format!("        Ok({plural}.into_iter().skip(offset).take(limit).collect())\n"));
    }
    code.push_str("    }\n\n");
}

fn generate_count(code: &mut String, entity: &EntityDef) {
    let snake = to_snake_case(&entity.name);
    let plural = pluralize(&snake);
    let records = records(&snake);

    code.push_str(&format!("    pub async fn count_{plural}(&self) -> Result<u64, AppError> {{\n"));
    // A count needs the number of records, not their contents: the runtime's
    // `count` walks a per-entity directory without reading a file, where
    // `read_all` would also parse every one. Both go through `list_paths`,
    // so the list cap still applies and an oversized directory fails the
    // same way.
    code.push_str(&format!("        Ok(self.vault().{records}.count().map_err(AppError::from)? as u64)\n"));
    code.push_str("    }\n\n");
}

fn generate_get(code: &mut String, entity: &EntityDef, has_relations: bool) {
    let name = &entity.name;
    let snake = to_snake_case(name);
    let fm = fm_type(name);
    let records = records(&snake);
    let not_found = not_found_variant(name);

    code.push_str(&format!("    pub async fn get_{snake}(&self, id: &str) -> Result<{name}, AppError> {{\n"));
    // An id the vault cannot hold (`Error::InvalidId`) names no record: a
    // lookup of one is NotFound, like any other id with no file behind it.
    code.push_str(&format!("        let doc = match self.vault().{records}.read_opt(id) {{\n"));
    code.push_str("            Ok(Some(doc)) => doc,\n");
    code.push_str("            Ok(None) | Err(markdown_store::Error::InvalidId { .. }) => {\n");
    code.push_str(&format!("                return Err(AppError::{not_found}(id.to_string()));\n"));
    code.push_str("            }\n");
    code.push_str("            Err(e) => return Err(AppError::from(e)),\n");
    code.push_str("        };\n");
    code.push_str(&format!("        let fm: {fm} = doc.deserialize().map_err(AppError::from)?;\n"));

    if has_relations {
        code.push_str(&format!("        let mut {snake} = {};\n", into_call(&snake, entity, "id.to_string()", "doc")));
        code.push_str(&format!("        self.populate_{snake}_relations(&mut {snake}).await?;\n"));
        code.push_str(&format!("        Ok({snake})\n"));
    } else {
        code.push_str(&format!("        Ok({})\n", into_call(&snake, entity, "id.to_string()", "doc")));
    }
    code.push_str("    }\n\n");
}

fn generate_create(code: &mut String, entity: &EntityDef, entities: &[EntityDef], id_strategy: &IdStrategy) {
    let name = &entity.name;
    let snake = to_snake_case(name);
    let fm = fm_type(name);
    let fields = fields_const(&snake);
    let records = records(&snake);
    let entity_kind = entity_kind_variant(name);

    code.push_str(&format!(
        "    pub async fn create_{snake}(&self, mut {snake}: {name}) -> Result<{name}, AppError> {{\n"
    ));
    code.push_str(&format!("        hooks::before_create(self, &mut {snake}).await?;\n\n"));

    // has_many children are derived views: capture before persisting so the
    // reverse FKs can be set after the record exists (same sequencing as the
    // SeaORM emission; m2m needs no step — the wikilink list IS the storage).
    let writes = has_many_writes(entity);
    for hm in &writes {
        code.push_str(&format!("        let {fname} = {snake}.{fname}.clone();\n", fname = hm.field));
    }
    if !writes.is_empty() {
        code.push('\n');
    }
    let listed = Listed::Create(&snake);
    linked_ids::emit_self_listing_check(code, entity, &listed);
    linked_ids::emit_listed_ids_check(code, entity, entities, &listed);
    emit_integer_range_checks(code, entity, IntegerSource::Record(&snake), Skipped::NotStored, serialize_error);
    emit_nan_checks(code, entity, FloatSource::Record(&snake), Skipped::NotStored, serialize_error);

    code.push_str("        let mut doc = markdown_store::Document::new();\n");
    code.push_str(&format!(
        "        doc.merge_serialize(&{fm}::from_{snake}(&{snake}), {fields}).map_err(AppError::from)?;\n"
    ));
    if let Some(body) = entity.body_field() {
        code.push_str(&format!("        doc.set_body({snake}.{}.clone());\n", body.name));
    }

    // The runtime derives a missing id by the strategy, probing `-2`, `-3`
    // under the vault's write lock, so only a provided id can already exist.
    code.push_str(&format!("        let id = match self.vault().{records}.create(\n"));
    code.push_str(&format!("            &{},\n", runtime_strategy(id_strategy)));
    code.push_str(&format!("            Some({snake}.id.as_str()).filter(|s| !s.trim().is_empty()),\n"));
    code.push_str(&format!("            {},\n", slug_source_expr(&snake, id_strategy)));
    code.push_str("            doc,\n");
    code.push_str("        ) {\n");
    code.push_str("            Ok(id) => id,\n");
    code.push_str(&format!(
        "            Err(markdown_store::Error::IdRequired {{ reason }}) => return Err(AppError::{name}IdRequired(reason)),\n"
    ));
    code.push_str(&format!(
        "            Err(markdown_store::Error::AlreadyExists {{ .. }}) => return Err(AppError::{name}AlreadyExists({snake}.id)),\n"
    ));
    code.push_str("            Err(e) => return Err(AppError::from(e)),\n");
    code.push_str("        };\n\n");

    for hm in &writes {
        code.push_str(&format!("        for child_id in &{fname} {{\n", fname = hm.field));
        code.push_str(&format!(
            "            self.set_{snake}_parent(child_id, {}).await?;\n",
            has_many::set_parent_arg(hm.fk_required, "&id")
        ));
        code.push_str("        }\n\n");
    }

    code.push_str(&format!("        let created = self.get_{snake}(&id).await?;\n"));
    code.push_str(&format!("        self.emit_change(ChangeOp::Created, EntityKind::{entity_kind}, id);\n"));
    code.push_str("        hooks::after_create(self, &created).await?;\n");
    code.push_str("        Ok(created)\n");
    code.push_str("    }\n\n");
}

fn generate_update(code: &mut String, entity: &EntityDef, entities: &[EntityDef]) {
    let name = &entity.name;
    let snake = to_snake_case(name);
    let fm = fm_type(name);
    let fields = fields_const(&snake);
    let records = records(&snake);
    let entity_kind = entity_kind_variant(name);

    code.push_str(&format!(
        "    pub async fn update_{snake}(&self, id: &str, updates: {name}Update) -> Result<{name}, AppError> {{\n"
    ));

    // Fetch existing (relation-populated, so before_update sees the same
    // value shape SeaORM hooks see — hook VALUE parity, not just signatures).
    code.push_str(&format!("        let current = self.get_{snake}(id).await?;\n"));
    code.push_str("        hooks::before_update(self, &current, &updates).await?;\n\n");

    // Track which derived has_many fields changed (m2m lives in frontmatter
    // and persists with the record itself — no tracking needed).
    let has_manys: Vec<_> = entity.has_many_relations().collect();
    for (field, _info) in &has_manys {
        code.push_str(&format!("        let {fname}_changed = updates.{fname}.is_some();\n", fname = field.name));
    }
    if !has_manys.is_empty() {
        code.push('\n');
    }
    let writes = has_many_writes(entity);
    linked_ids::emit_self_listing_check(code, entity, &Listed::Update);
    linked_ids::emit_listed_ids_check(code, entity, entities, &Listed::Update);
    has_many::emit_dropped_children(code, &writes);
    emit_integer_range_checks(code, entity, IntegerSource::Updates, Skipped::NotStored, serialize_error);
    emit_nan_checks(code, entity, FloatSource::Updates, Skipped::NotStored, serialize_error);

    code.push_str("        self.vault()\n");
    code.push_str(&format!("            .{records}\n"));
    code.push_str("            .modify(id, |doc| {\n");
    code.push_str(&format!("                let fm: {fm} = doc.deserialize()?;\n"));
    code.push_str(&format!(
        "                let mut {snake} = {};\n",
        into_call(&snake, entity, "id.to_string()", "doc")
    ));
    code.push_str(&format!("                updates.apply(&mut {snake});\n"));
    code.push_str(&format!("                doc.merge_serialize(&{fm}::from_{snake}(&{snake}), {fields})?;\n"));
    if let Some(body) = entity.body_field() {
        code.push_str(&format!("                doc.set_body({snake}.{});\n", body.name));
    }
    code.push_str("                Ok(())\n");
    code.push_str("            })\n");
    code.push_str("            .map_err(AppError::from)?;\n\n");

    // Conditional has_many reverse sync (read-mutate-rewrite per child).
    has_many::emit_update_children(code, entity, &writes, |f| format!("updates.{f}.iter().flatten()"), None);
    if !writes.is_empty() {
        code.push('\n');
    }

    code.push_str(&format!("        let result = self.get_{snake}(id).await?;\n"));
    code.push_str(&format!(
        "        self.emit_change(ChangeOp::Updated, EntityKind::{entity_kind}, id.to_string());\n"
    ));
    code.push_str("        hooks::after_update(self, &result).await?;\n");
    code.push_str("        Ok(result)\n");
    code.push_str("    }\n\n");
}

fn generate_delete(code: &mut String, entity: &EntityDef) {
    let name = &entity.name;
    let snake = to_snake_case(name);
    let records = records(&snake);
    let not_found = not_found_variant(name);
    let entity_kind = entity_kind_variant(name);

    code.push_str(&format!("    pub async fn delete_{snake}(&self, id: &str) -> Result<(), AppError> {{\n"));
    code.push_str("        hooks::before_delete(self, id).await?;\n\n");

    code.push_str(&format!("        match self.vault().{records}.remove(id) {{\n"));
    code.push_str("            Ok(()) => {}\n");
    code.push_str(
        "            Err(markdown_store::Error::NotFound { .. } | markdown_store::Error::InvalidId { .. }) => {\n",
    );
    code.push_str(&format!("                return Err(AppError::{not_found}(id.to_string()));\n"));
    code.push_str("            }\n");
    code.push_str("            Err(e) => return Err(AppError::from(e)),\n");
    code.push_str("        }\n\n");

    code.push_str(&format!(
        "        self.emit_change(ChangeOp::Deleted, EntityKind::{entity_kind}, id.to_string());\n"
    ));
    code.push_str("        hooks::after_delete(self, id).await?;\n");
    code.push_str("        Ok(())\n");
    code.push_str("    }\n\n");
}

// ─── populate_relations ──────────────────────────────────────────────────────

fn generate_populate_relations(code: &mut String, entity: &EntityDef) {
    let name = &entity.name;
    let snake = to_snake_case(name);
    let fm = fm_type(name);
    let records = records(&snake);

    code.push_str(&format!("    pub(crate) async fn populate_{snake}_relations(\n"));
    code.push_str(&format!("        &self,\n        {snake}: &mut crate::schema::{name},\n"));
    code.push_str("    ) -> Result<(), crate::schema::AppError> {\n");

    // m2m: authoritative wikilink list in this record's own frontmatter —
    // already populated by the parse; nothing to load.
    let has_manys: Vec<_> = entity.has_many_relations().collect();
    if has_manys.iter().all(|(_, info)| info.foreign_key.is_none()) {
        code.push_str("        // many_to_many lists are authoritative in this record's own\n");
        code.push_str("        // frontmatter and were populated at parse time.\n");
        code.push_str("        let _ = &*self;\n");
        code.push_str(&format!("        let _ = &*{snake};\n"));
    }

    // has_many: derived view — walk the entity directory and collect the ids
    // of records whose FK points back here. O(N) over the folder, by design.
    // The walk is in id order, so the list is id-ascending (ADR 0006 §3),
    // as SeaORM's `ORDER BY id` makes it there.
    for hm in has_many_writes(entity) {
        let fk = hm.fk;
        code.push_str(&format!("        let mut {fname} = Vec::new();\n", fname = hm.field));
        code.push_str(&format!(
            "        for (child_id, doc) in self.vault().{records}.read_all().map_err(AppError::from)? {{\n"
        ));
        code.push_str(&format!("            if child_id == {snake}.id {{\n"));
        code.push_str("                continue;\n");
        code.push_str("            }\n");
        code.push_str(&format!("            let child: {fm} = doc.deserialize().map_err(AppError::from)?;\n"));
        if hm.fk_required {
            code.push_str(&format!("            if markdown_store::wikilink::strip(&child.{fk}) == {snake}.id {{\n"));
        } else {
            code.push_str(&format!(
                "            if markdown_store::wikilink::strip_opt(child.{fk}).as_deref() == Some({snake}.id.as_str()) {{\n"
            ));
        }
        code.push_str(&format!("                {fname}.push(child_id);\n", fname = hm.field));
        code.push_str("            }\n");
        code.push_str("        }\n");
        code.push_str(&format!("        {snake}.{fname} = {fname};\n", fname = hm.field));
    }

    code.push_str("        Ok(())\n");
    code.push_str("    }\n\n");
}

// ─── Refused values ──────────────────────────────────────────────────────────

/// The catch-all the markdown store refuses a value with before writing:
/// `markdown_store::Error::Serialize` into the consumer's `AppError`, as a
/// value the vault could hold but SeaORM could not. `message` is an
/// expression of type `String`.
fn serialize_error(message: &str) -> String {
    format!("AppError::from(markdown_store::Error::Serialize {{ message: {message} }})")
}

// ─── set_parent helper ───────────────────────────────────────────────────────

/// `set_{snake}_parent`: read-mutate-rewrite the child's FK field — the
/// markdown replacement for SeaORM's raw-SQL fast path. The children are
/// records of the same entity (`has_many::validate_targets` refuses any
/// other shape). A required FK takes a parent, an optional one `None` to
/// clear it.
fn generate_set_parent_helper(code: &mut String, entity: &EntityDef, fk: &str, fk_required: bool) {
    let name = &entity.name;
    let snake = to_snake_case(name);
    let fm = fm_type(name);
    let fields = fields_const(&snake);
    let records = records(&snake);

    code.push_str(&format!("    async fn set_{snake}_parent(\n"));
    code.push_str("        &self,\n");
    code.push_str("        child_id: &str,\n");
    code.push_str(&format!("        parent_id: {},\n", has_many::parent_param_type(fk_required)));
    code.push_str("    ) -> Result<(), AppError> {\n");
    code.push_str("        self.vault()\n");
    code.push_str(&format!("            .{records}\n"));
    code.push_str("            .modify(child_id, |doc| {\n");
    code.push_str(&format!("                let mut fm: {fm} = doc.deserialize()?;\n"));
    if fk_required {
        code.push_str(&format!("                fm.{fk} = markdown_store::wikilink::encode(parent_id);\n"));
    } else {
        code.push_str(&format!("                fm.{fk} = parent_id.map(markdown_store::wikilink::encode);\n"));
    }
    code.push_str(&format!("                doc.merge_serialize(&fm, {fields})\n"));
    code.push_str("            })\n");
    code.push_str("            .map_err(AppError::from)\n");
    code.push_str("    }\n\n");
}

/// `{snake}_exists`: whether a record with this id exists, for the checks
/// on the ids a write lists (`linked_ids`). An id the vault cannot hold
/// names no record, as in `get_*`.
fn generate_exists_helper(code: &mut String, entity: &EntityDef) {
    let records = records(&to_snake_case(&entity.name));

    code.push_str(&format!(
        "    pub(crate) async fn {}(&self, id: &str) -> Result<bool, AppError> {{\n",
        linked_ids::exists_helper(&entity.name)
    ));
    code.push_str(&format!("        match self.vault().{records}.read_opt(id) {{\n"));
    code.push_str("            Ok(doc) => Ok(doc.is_some()),\n");
    code.push_str("            Err(markdown_store::Error::InvalidId { .. }) => Ok(false),\n");
    code.push_str("            Err(e) => Err(AppError::from(e)),\n");
    code.push_str("        }\n");
    code.push_str("    }\n\n");
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn not_found_variant(entity_name: &str) -> String {
    format!("{entity_name}NotFound")
}

fn entity_kind_variant(entity_name: &str) -> String {
    entity_name.to_string()
}

/// The runtime `IdStrategy` value the generated create passes, spelled out
/// from the build-time one.
fn runtime_strategy(id_strategy: &IdStrategy) -> String {
    match id_strategy {
        IdStrategy::Provided => "markdown_store::IdStrategy::Provided".to_string(),
        IdStrategy::SlugFromField(field) => format!("markdown_store::IdStrategy::SlugFromField({field:?}.into())"),
        IdStrategy::Uuid => "markdown_store::IdStrategy::Uuid".to_string(),
    }
}

/// The slug-source argument of the runtime create: the slug field's value,
/// or `None` when the strategy doesn't slug.
fn slug_source_expr(snake: &str, id_strategy: &IdStrategy) -> String {
    match id_strategy {
        IdStrategy::SlugFromField(field) => format!("Some({snake}.{field}.as_str())"),
        IdStrategy::Provided | IdStrategy::Uuid => "None".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::model::{FieldDef, FieldRole, FieldType, RelationInfo, RelationKind};

    fn node(fk_type: FieldType) -> EntityDef {
        EntityDef {
            name: "Node".to_string(),
            directory: "nodes".to_string(),
            table: "nodes".to_string(),
            type_name: "node".to_string(),
            prefix: "node".to_string(),
            id_strategy: None,
            fields: vec![
                FieldDef::new("id", FieldType::String, FieldRole::Id),
                FieldDef::new("name", FieldType::String, FieldRole::Plain),
                FieldDef::new(
                    "parent_id",
                    fk_type,
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
                FieldDef::new("body", FieldType::String, FieldRole::Body),
            ],
            doc: String::new(),
        }
    }

    fn crud(entity: &EntityDef) -> String {
        let mut code = String::new();
        generate_crud_impl(&mut code, entity, std::slice::from_ref(entity), &IdStrategy::SlugFromField("name".into()));
        code
    }

    fn method<'a>(code: &'a str, name: &str) -> &'a str {
        let start = code.find(&format!("fn {name}(")).unwrap_or_else(|| panic!("no fn {name}"));
        let rest = &code[start..];
        &rest[..rest.find("\n    }\n").map(|e| e + 6).unwrap_or(rest.len())]
    }

    #[test]
    fn a_missing_child_is_refused_before_anything_is_written() {
        for fk_type in [FieldType::OptionString, FieldType::String] {
            let code = crud(&node(fk_type));

            let create = method(&code, "create_node");
            let check = create.find("if !self.node_exists(child_id).await? {").expect("create checks");
            assert!(create.contains("for child_id in &node.contains {"), "{create}");
            assert!(check > create.find("hooks::before_create").unwrap(), "after the hook: {create}");
            assert!(check < create.find(".create(\n").unwrap(), "before the record write: {create}");

            let update = method(&code, "update_node");
            let check = update.find("if !self.node_exists(child_id).await? {").expect("update checks");
            assert!(update.contains("for child_id in updates.contains.iter().flatten() {"), "{update}");
            assert!(check > update.find("hooks::before_update").unwrap(), "after the hook: {update}");
            assert!(check < update.find("let contains_dropped").unwrap(), "before the drop check: {update}");
            assert!(check < update.find(".modify(id,").unwrap(), "before the record write: {update}");
            assert!(update.contains("return Err(AppError::NodeNotFound(child_id.clone()));"), "{update}");

            let helper = method(&code, "node_exists");
            assert!(helper.contains(".entity(NODES_DIR, NODE_TYPE).read_opt(id)"), "{helper}");
            assert!(helper.contains("Err(markdown_store::Error::InvalidId { .. }) => Ok(false),"), "{helper}");
        }
    }

    #[test]
    fn the_listed_ids_are_checked_before_anything_is_written() {
        let mut node = node(FieldType::OptionString);
        node.fields.push(FieldDef::new(
            "tags",
            FieldType::VecString,
            FieldRole::Relation(RelationInfo {
                kind: RelationKind::ManyToMany,
                target: "Tag".to_string(),
                junction: None,
                foreign_key: None,
            }),
        ));
        let tag = EntityDef {
            name: "Tag".to_string(),
            directory: "tags".to_string(),
            table: "tags".to_string(),
            type_name: "tag".to_string(),
            prefix: "tag".to_string(),
            id_strategy: None,
            fields: vec![FieldDef::new("id", FieldType::String, FieldRole::Id)],
            doc: String::new(),
        };
        let entities = [node, tag];
        let strategy = IdStrategy::SlugFromField("name".into());
        let mut code = String::new();
        generate_crud_impl(&mut code, &entities[0], &entities, &strategy);

        for (op, write) in [("create_node", ".create(\n"), ("update_node", ".modify(id,")] {
            let body = method(&code, op);
            let at = |needle: &str| body.find(needle).unwrap_or_else(|| panic!("{op}: missing `{needle}`:\n{body}"));
            let cycle = at("return Err(AppError::NodeParentCycle(");
            let child = at("if !self.node_exists(child_id).await? {");
            let target = at("if !self.tag_exists(target_id).await? {");
            assert!(cycle < child && child < target && target < at(write), "{op}: {body}");
            assert!(body.contains("return Err(AppError::TagNotFound(target_id.clone()));"), "{body}");
        }

        let mut tag_code = String::new();
        generate_crud_impl(&mut tag_code, &entities[1], &entities, &strategy);
        let helper = method(&tag_code, "tag_exists");
        assert!(helper.starts_with("fn tag_exists(&self, id: &str)"), "{helper}");
        assert!(tag_code.contains("pub(crate) async fn tag_exists("), "callable from Node's module: {tag_code}");
        syn::parse_file(&code).unwrap_or_else(|e| panic!("invalid Rust: {e}\n{code}"));
    }

    #[test]
    fn a_value_outside_i64_is_refused_before_anything_is_written() {
        let mut entity = node(FieldType::OptionString);
        entity.fields.push(FieldDef::new("seq", FieldType::Other("u64".into()), FieldRole::Plain));
        entity.fields.push(FieldDef::new("cap", FieldType::OptionEnum("u128".into()), FieldRole::Plain));
        entity.fields.push(FieldDef::new("small", FieldType::Other("u32".into()), FieldRole::Plain));
        entity.fields.push(FieldDef::new("hidden", FieldType::Other("u64".into()), FieldRole::Skip));
        let code = crud(&entity);

        let create = method(&code, "create_node");
        let check = create.find("if let Some(v) = Some(node.seq).filter(|v| i64::try_from(*v).is_err()) {");
        let check = check.unwrap_or_else(|| panic!("create checks a bare u64: {create}"));
        assert!(create.contains("if let Some(v) = node.cap.filter(|v| i64::try_from(*v).is_err()) {"), "{create}");
        assert!(
            create.contains(
                r#"return Err(AppError::from(markdown_store::Error::Serialize { message: format!("Node.seq: value {v} is out of range for i64") }));"#
            ),
            "{create}"
        );
        assert!(check > create.find("if !self.node_exists(child_id)").unwrap(), "after the child check: {create}");
        assert!(check < create.find(".create(\n").unwrap(), "before the record write: {create}");

        let update = method(&code, "update_node");
        let check = update.find("if let Some(v) = updates.seq.filter(|v| i64::try_from(*v).is_err()) {");
        let check = check.unwrap_or_else(|| panic!("update checks a set u64: {update}"));
        assert!(update.contains("if let Some(v) = updates.cap.flatten().filter(|v| i64::try_from(*v).is_err()) {"));
        assert!(check > update.find("let contains_dropped").unwrap(), "after the drop check: {update}");
        assert!(check < update.find(".modify(id,").unwrap(), "before the record write: {update}");

        assert!(!code.contains("small"), "a u32 always fits: {code}");
        assert!(!code.contains("hidden"), "a skipped field is not stored: {code}");
        syn::parse_file(&code).unwrap_or_else(|e| panic!("invalid Rust: {e}\n{code}"));
    }

    #[test]
    fn a_list_sorts_every_record_before_it_cuts_the_page() {
        let code = crud(&node(FieldType::OptionString));
        let list = method(&code, "list_nodes");
        assert!(
            list.contains(
                "fn list_nodes(&self, order: &[OrderBy<NodeSortField>], limit: Option<u64>, offset: Option<u64>)"
            ),
            "{list}"
        );
        let sort = list.find("sort_nodes(&mut nodes, order);").expect("the list sorts");
        assert!(sort > list.find("read_all()").unwrap(), "after every record is read: {list}");
        assert!(sort < list.find(".skip(offset)").unwrap(), "before the page is cut: {list}");
    }

    #[test]
    fn a_nan_is_refused_after_the_hook_and_before_anything_is_written() {
        let mut entity = node(FieldType::OptionString);
        entity.fields.push(FieldDef::new("weight", FieldType::F64, FieldRole::Plain));
        entity.fields.push(FieldDef::new("low", FieldType::OptionF32, FieldRole::Plain));
        // Never written to the file, so never checked.
        entity.fields.push(FieldDef::new("cached", FieldType::F64, FieldRole::Skip));
        let code = crud(&entity);
        assert!(!code.contains("cached.is_nan"), "{code}");
        let refusal = r#"return Err(AppError::from(markdown_store::Error::Serialize { message: "Node.weight: NaN cannot be stored".to_string() }));"#;

        let create = method(&code, "create_node");
        let check = create.find("if node.weight.is_nan() {").unwrap_or_else(|| panic!("create checks: {create}"));
        assert!(create.contains("if node.low.is_some_and(f32::is_nan) {"), "{create}");
        assert!(create.contains(refusal), "{create}");
        assert!(check > create.find("hooks::before_create").unwrap(), "after the hook: {create}");
        assert!(check < create.find(".create(\n").unwrap(), "before the record write: {create}");

        let update = method(&code, "update_node");
        let check = update.find("if updates.weight.is_some_and(f64::is_nan) {").unwrap_or_else(|| panic!("{update}"));
        assert!(update.contains("if updates.low.flatten().is_some_and(f32::is_nan) {"), "{update}");
        assert!(check > update.find("hooks::before_update").unwrap(), "after the hook: {update}");
        assert!(check < update.find(".modify(id,").unwrap(), "before the record write: {update}");
        assert!(!update.contains("current.weight"), "only what the update sets is checked: {update}");
        syn::parse_file(&code).unwrap_or_else(|e| panic!("invalid Rust: {e}\n{code}"));
    }

    #[test]
    fn generated_code_is_valid_rust() {
        for fk_type in [FieldType::OptionString, FieldType::String] {
            let code = crud(&node(fk_type));
            syn::parse_file(&code).unwrap_or_else(|e| panic!("invalid Rust: {e}\n--- code ---\n{code}"));
        }
    }
}
