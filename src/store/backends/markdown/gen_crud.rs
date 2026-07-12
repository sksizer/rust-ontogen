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

use crate::schema::model::EntityDef;
use crate::store::helpers::{pluralize, to_snake_case};

// ─── Public API ──────────────────────────────────────────────────────────────

/// Generate the complete `impl Store { ... }` block for the markdown backend.
pub fn generate_crud_impl(code: &mut String, entity: &EntityDef, slug_source: Option<&str>) {
    let has_relations = entity.junction_relations().next().is_some() || entity.has_many_relations().next().is_some();

    code.push_str("impl Store {\n");

    generate_list(code, entity, has_relations);
    generate_get(code, entity, has_relations);
    generate_create(code, entity, slug_source);
    generate_update(code, entity);
    generate_delete(code, entity);

    if has_relations {
        generate_populate_relations(code, entity);
    }

    // has_many reverse helpers (e.g., set_node_parent) — read-mutate-rewrite
    // replaces SeaORM's raw-SQL fast path.
    for (_field, info) in entity.has_many_relations() {
        if let Some(ref fk) = info.foreign_key {
            generate_set_parent_helper(code, entity, fk);
        }
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
    let dir = dir_const(&snake);

    code.push_str(&format!(
        "    pub async fn list_{plural}(&self, limit: Option<u64>, offset: Option<u64>) -> Result<Vec<{name}>, AppError> {{\n"
    ));
    code.push_str(&format!("        let mut {plural} = Vec::new();\n"));
    code.push_str(&format!("        for (id, doc) in self.vault().read_all({dir}).map_err(AppError::from)? {{\n"));
    code.push_str(&format!("            let fm: {fm} = doc.deserialize().map_err(AppError::from)?;\n"));
    code.push_str(&format!("            {plural}.push({});\n", into_call(&snake, entity, "id", "doc")));
    code.push_str("        }\n");
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

fn generate_get(code: &mut String, entity: &EntityDef, has_relations: bool) {
    let name = &entity.name;
    let snake = to_snake_case(name);
    let fm = fm_type(name);
    let dir = dir_const(&snake);
    let not_found = not_found_variant(name);

    code.push_str(&format!("    pub async fn get_{snake}(&self, id: &str) -> Result<{name}, AppError> {{\n"));
    code.push_str("        let doc = self\n");
    code.push_str("            .vault()\n");
    code.push_str(&format!("            .read_record_opt({dir}, id)\n"));
    code.push_str("            .map_err(AppError::from)?\n");
    code.push_str(&format!("            .ok_or_else(|| AppError::{not_found}(id.to_string()))?;\n"));
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

fn generate_create(code: &mut String, entity: &EntityDef, slug_source: Option<&str>) {
    let name = &entity.name;
    let snake = to_snake_case(name);
    let fm = fm_type(name);
    let fields = fields_const(&snake);
    let dir = dir_const(&snake);
    let entity_kind = entity_kind_variant(name);

    code.push_str(&format!(
        "    pub async fn create_{snake}(&self, mut {snake}: {name}) -> Result<{name}, AppError> {{\n"
    ));
    code.push_str(&format!("        hooks::before_create(self, &mut {snake}).await?;\n\n"));

    // has_many children are derived views: capture before persisting so the
    // reverse FKs can be set after the record exists (same sequencing as the
    // SeaORM emission; m2m needs no step — the wikilink list IS the storage).
    let has_manys: Vec<_> = entity.has_many_relations().collect();
    for (field, _info) in &has_manys {
        code.push_str(&format!("        let {fname} = {snake}.{fname}.clone();\n", fname = field.name));
    }
    if !has_manys.is_empty() {
        code.push('\n');
    }

    code.push_str("        let mut doc = markdown_store::Document::new();\n");
    code.push_str(&format!(
        "        doc.merge_serialize(&{fm}::from_{snake}(&{snake}), {fields}).map_err(AppError::from)?;\n"
    ));
    if let Some(body) = entity.body_field() {
        code.push_str(&format!("        doc.set_body({snake}.{}.clone());\n", body.name));
    }

    code.push_str("        let id = self\n");
    code.push_str("            .vault()\n");
    code.push_str("            .create_record_derived(\n");
    code.push_str(&format!("                {dir},\n"));
    code.push_str(&format!("                Some({snake}.id.as_str()).filter(|s| !s.is_empty()),\n"));
    code.push_str(&format!("                {},\n", slug_source_expr(&snake, slug_source)));
    code.push_str("                &doc,\n");
    code.push_str("            )\n");
    code.push_str("            .map_err(AppError::from)?;\n\n");

    for (field, info) in &has_manys {
        if info.foreign_key.is_some() {
            code.push_str(&format!("        for child_id in &{fname} {{\n", fname = field.name));
            code.push_str(&format!("            self.set_{snake}_parent(child_id, Some(&id)).await?;\n"));
            code.push_str("        }\n\n");
        }
    }

    code.push_str(&format!("        let created = self.get_{snake}(&id).await?;\n"));
    code.push_str(&format!("        self.emit_change(ChangeOp::Created, EntityKind::{entity_kind}, id);\n"));
    code.push_str("        hooks::after_create(self, &created).await?;\n");
    code.push_str("        Ok(created)\n");
    code.push_str("    }\n\n");
}

fn generate_update(code: &mut String, entity: &EntityDef) {
    let name = &entity.name;
    let snake = to_snake_case(name);
    let fm = fm_type(name);
    let fields = fields_const(&snake);
    let dir = dir_const(&snake);
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

    code.push_str("        self.vault()\n");
    code.push_str(&format!("            .modify_record({dir}, id, |doc| {{\n"));
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
    for (field, info) in &has_manys {
        if info.foreign_key.is_some() {
            code.push_str(&format!("        if {fname}_changed {{\n", fname = field.name));
            code.push_str(&format!("            if let Some({fname}) = &updates.{fname} {{\n", fname = field.name));
            code.push_str(&format!("                for child_id in {fname} {{\n", fname = field.name));
            code.push_str(&format!("                    self.set_{snake}_parent(child_id, Some(id)).await?;\n"));
            code.push_str("                }\n");
            code.push_str("            }\n");
            code.push_str("        }\n");
        }
    }
    if has_manys.iter().any(|(_, info)| info.foreign_key.is_some()) {
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
    let dir = dir_const(&snake);
    let not_found = not_found_variant(name);
    let entity_kind = entity_kind_variant(name);

    code.push_str(&format!("    pub async fn delete_{snake}(&self, id: &str) -> Result<(), AppError> {{\n"));
    code.push_str("        hooks::before_delete(self, id).await?;\n\n");

    code.push_str(&format!("        match self.vault().remove_record({dir}, id) {{\n"));
    code.push_str("            Ok(()) => {}\n");
    code.push_str("            Err(markdown_store::Error::NotFound { .. }) => {\n");
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
    let dir = dir_const(&snake);

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
    for (field, info) in &has_manys {
        let Some(ref fk) = info.foreign_key else { continue };
        code.push_str(&format!("        let mut {fname} = Vec::new();\n", fname = field.name));
        code.push_str(&format!(
            "        for (child_id, doc) in self.vault().read_all({dir}).map_err(AppError::from)? {{\n"
        ));
        code.push_str(&format!("            if child_id == {snake}.id {{\n"));
        code.push_str("                continue;\n");
        code.push_str("            }\n");
        code.push_str(&format!("            let child: {fm} = doc.deserialize().map_err(AppError::from)?;\n"));
        code.push_str(&format!(
            "            if markdown_store::wikilink::strip_opt(child.{fk}).as_deref() == Some({snake}.id.as_str()) {{\n"
        ));
        code.push_str(&format!("                {fname}.push(child_id);\n", fname = field.name));
        code.push_str("            }\n");
        code.push_str("        }\n");
        code.push_str(&format!("        {snake}.{fname} = {fname};\n", fname = field.name));
    }

    code.push_str("        Ok(())\n");
    code.push_str("    }\n\n");
}

// ─── set_parent helper ───────────────────────────────────────────────────────

/// `set_{snake}_parent`: read-mutate-rewrite the child's FK field — the
/// markdown replacement for SeaORM's raw-SQL fast path. (Like the SeaORM
/// emission, this assumes the self-referential has_many shape: children are
/// records of the same entity.)
fn generate_set_parent_helper(code: &mut String, entity: &EntityDef, fk: &str) {
    let name = &entity.name;
    let snake = to_snake_case(name);
    let fm = fm_type(name);
    let fields = fields_const(&snake);
    let dir = dir_const(&snake);

    code.push_str(&format!("    async fn set_{snake}_parent(\n"));
    code.push_str("        &self,\n");
    code.push_str("        child_id: &str,\n");
    code.push_str("        parent_id: Option<&str>,\n");
    code.push_str("    ) -> Result<(), AppError> {\n");
    code.push_str("        self.vault()\n");
    code.push_str(&format!("            .modify_record({dir}, child_id, |doc| {{\n"));
    code.push_str(&format!("                let mut fm: {fm} = doc.deserialize()?;\n"));
    code.push_str(&format!("                fm.{fk} = parent_id.map(markdown_store::wikilink::encode);\n"));
    code.push_str(&format!("                doc.merge_serialize(&fm, {fields})\n"));
    code.push_str("            })\n");
    code.push_str("            .map_err(AppError::from)\n");
    code.push_str("    }\n\n");
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn not_found_variant(entity_name: &str) -> String {
    format!("{entity_name}NotFound")
}

fn entity_kind_variant(entity_name: &str) -> String {
    entity_name.to_string()
}

/// The slug-source argument for `create_record_derived`, derived from the
/// configured id strategy. `None` when the strategy doesn't slug.
fn slug_source_expr(snake: &str, slug_source: Option<&str>) -> String {
    match slug_source {
        Some(field) => format!("Some({snake}.{field}.as_str())"),
        None => "None".to_string(),
    }
}
