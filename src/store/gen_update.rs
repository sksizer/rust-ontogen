//! Generate Update struct, apply() method, and From impls for store entities.
//!
//! For each entity generates:
//! - `{Entity}Update` struct — all non-id, non-skip fields wrapped in Option
//! - `{Entity}Update::apply()` — patches entity in-place
//! - `From<Update{Entity}Input> for {Entity}Update` — DTO → patch struct
//! - `From<Create{Entity}Input> for {Entity}` — DTO → domain entity

use super::helpers::to_snake_case;
use crate::schema::model::{EntityDef, FieldDef, FieldRole, FieldType, RelationKind};

// ─── Update struct ───────────────────────────────────────────────────────────

/// Generate the `{Entity}Update` struct definition.
pub fn generate_update_struct(code: &mut String, entity: &EntityDef) {
    let name = &entity.name;

    code.push_str(&format!("/// Partial update for a {name}. Only `Some` values are applied.\n"));
    code.push_str("#[derive(Debug, Clone, Default)]\n");
    code.push_str(&format!("pub struct {name}Update {{\n"));

    for field in updatable_fields(entity) {
        let update_type = field_to_update_type(field);
        code.push_str(&format!("    pub {}: {},\n", field.name, update_type));
    }

    code.push_str("}\n\n");
}

// ─── apply() method ──────────────────────────────────────────────────────────

/// Generate the `apply()` method on `{Entity}Update`.
pub fn generate_apply_method(code: &mut String, entity: &EntityDef) {
    let name = &entity.name;
    let snake = to_snake_case(name);

    code.push_str(&format!("impl {name}Update {{\n"));
    code.push_str(&format!("    fn apply(&self, {snake}: &mut {name}) {{\n"));

    for field in updatable_fields(entity) {
        let fname = &field.name;
        code.push_str(&format!("        if let Some({fname}) = &self.{fname} {{\n"));
        code.push_str(&format!("            {snake}.{fname}.clone_from({fname});\n"));
        code.push_str("        }\n");
    }

    code.push_str("    }\n");
    code.push_str("}\n\n");
}

// ─── From<UpdateInput> ──────────────────────────────────────────────────────

/// Generate `From<Update{Entity}Input> for {Entity}Update`.
pub fn generate_from_update_input(code: &mut String, entity: &EntityDef) {
    let name = &entity.name;
    let fields = updatable_fields(entity);
    let needs_strip = needs_wikilink_stripping(&fields);

    code.push_str(&format!("impl From<crate::schema::Update{name}Input> for {name}Update {{\n"));
    code.push_str(&format!("    fn from(input: crate::schema::Update{name}Input) -> Self {{\n"));

    // Import only the wikilink stripping functions actually needed
    if needs_strip {
        let mut imports = Vec::new();
        if fields.iter().any(|f| is_relation_string(f)) {
            imports.push("strip_wikilink");
        }
        if fields.iter().any(|f| is_relation_opt_string(f)) {
            imports.push("strip_wikilink_opt");
        }
        if fields.iter().any(|f| is_relation_vec(f)) {
            imports.push("strip_wikilinks_vec");
        }
        code.push_str(&format!(
            "        use crate::persistence::fs_markdown::parser::ontology::{{{}}};\n\n",
            imports.join(", ")
        ));
    }

    code.push_str("        Self {\n");

    for field in &fields {
        let fname = &field.name;
        if is_relation_vec(field) {
            code.push_str(&format!("            {fname}: input.{fname}.map(strip_wikilinks_vec),\n"));
        } else if is_relation_opt_string(field) {
            // Option<Option<String>> in Update: outer Option = "present?", inner = value
            code.push_str(&format!("            {fname}: input.{fname}.map(strip_wikilink_opt),\n"));
        } else if is_relation_string(field) {
            // Option<String> in Update: outer Option = "present?", inner = the string
            code.push_str(&format!("            {fname}: input.{fname}.map(|v| strip_wikilink(&v)),\n"));
        } else {
            code.push_str(&format!("            {fname}: input.{fname},\n"));
        }
    }

    code.push_str("        }\n");
    code.push_str("    }\n");
    code.push_str("}\n\n");
}

// ─── From<CreateInput> ──────────────────────────────────────────────────────

/// Generate `From<Create{Entity}Input> for {Entity}`.
pub fn generate_from_create_input(code: &mut String, entity: &EntityDef) {
    let name = &entity.name;
    let create_fields: Vec<_> = entity
        .fields
        .iter()
        .filter(|f| !matches!(f.role, FieldRole::Skip) && f.name != "wikilinks" && f.name != "source_file")
        .collect();
    let needs_strip =
        create_fields.iter().any(|f| is_relation_vec(f) || is_relation_opt_string(f) || is_relation_string(f));

    code.push_str(&format!("impl From<crate::schema::Create{name}Input> for {name} {{\n"));
    code.push_str(&format!("    fn from(input: crate::schema::Create{name}Input) -> Self {{\n"));

    if needs_strip {
        let mut imports = Vec::new();
        if create_fields.iter().any(|f| is_relation_string(f)) {
            imports.push("strip_wikilink");
        }
        if create_fields.iter().any(|f| is_relation_opt_string(f)) {
            imports.push("strip_wikilink_opt");
        }
        if create_fields.iter().any(|f| is_relation_vec(f)) {
            imports.push("strip_wikilinks_vec");
        }
        code.push_str(&format!(
            "        use crate::persistence::fs_markdown::parser::ontology::{{{}}};\n\n",
            imports.join(", ")
        ));
    }

    code.push_str("        Self {\n");

    for field in &create_fields {
        let fname = &field.name;
        if is_relation_vec(field) {
            code.push_str(&format!("            {fname}: strip_wikilinks_vec(input.{fname}),\n"));
        } else if is_relation_opt_string(field) {
            code.push_str(&format!("            {fname}: strip_wikilink_opt(input.{fname}),\n"));
        } else if is_relation_string(field) {
            code.push_str(&format!("            {fname}: strip_wikilink(&input.{fname}),\n"));
        } else {
            code.push_str(&format!("            {fname}: input.{fname},\n"));
        }
    }

    code.push_str("        }\n");
    code.push_str("    }\n");
    code.push_str("}\n\n");
}

// ─── Field helpers ───────────────────────────────────────────────────────────

/// Returns all fields that should appear in the Update struct.
/// Excludes: id, skip, persistence-only fields (wikilinks, source_file).
fn updatable_fields(entity: &EntityDef) -> Vec<&FieldDef> {
    entity
        .fields
        .iter()
        .filter(|f| {
            !matches!(f.role, FieldRole::Id | FieldRole::Skip) && f.name != "wikilinks" && f.name != "source_file"
        })
        .collect()
}

/// Convert a field's schema type into its Update struct type.
///
/// The pattern is:
/// - `String` → `Option<String>`
/// - `Option<T>` → `Option<Option<T>>` (double-wrapped for nullable clear)
/// - `Vec<T>` → `Option<Vec<T>>`
fn field_to_update_type(field: &FieldDef) -> String {
    match &field.field_type {
        FieldType::String => "Option<String>".to_string(),
        FieldType::OptionString => "Option<Option<String>>".to_string(),
        FieldType::OptionEnum(inner) => format!("Option<Option<{}>>", qualify_type(inner)),
        FieldType::VecString => "Option<Vec<String>>".to_string(),
        FieldType::VecStruct(inner) => format!("Option<Vec<crate::schema::{inner}>>"),
        FieldType::I32 => "Option<i32>".to_string(),
        FieldType::OptionI32 => "Option<Option<i32>>".to_string(),
        FieldType::I64 => "Option<i64>".to_string(),
        FieldType::OptionI64 => "Option<Option<i64>>".to_string(),
        FieldType::Bool => "Option<bool>".to_string(),
        FieldType::OptionBool => "Option<Option<bool>>".to_string(),
        FieldType::Other(ty) => format!("Option<{}>", qualify_type(ty)),
    }
}

/// Qualify a type with `crate::schema::` unless it's a primitive.
fn qualify_type(t: &str) -> String {
    match t {
        "u8" | "u16" | "u32" | "u64" | "u128" | "usize" | "i8" | "i16" | "i32" | "i64" | "i128" | "isize" | "f32"
        | "f64" | "bool" | "char" | "String" => t.to_string(),
        _ => format!("crate::schema::{t}"),
    }
}

/// Check if a field is a relation-typed Vec (has_many or many_to_many).
/// These fields need wikilink stripping on input via `strip_wikilinks_vec`.
fn is_relation_vec(field: &FieldDef) -> bool {
    match &field.role {
        FieldRole::Relation(info) => {
            matches!(info.kind, RelationKind::HasMany | RelationKind::ManyToMany)
        }
        _ => false,
    }
}

/// Check if a field is a BelongsTo relation with Option<String> type.
/// These need wikilink stripping via `strip_wikilink_opt`.
fn is_relation_opt_string(field: &FieldDef) -> bool {
    matches!(
        (&field.role, &field.field_type),
        (FieldRole::Relation(info), FieldType::OptionString)
            if matches!(info.kind, RelationKind::BelongsTo)
    )
}

/// Check if a field is a BelongsTo relation with plain String type.
/// These need wikilink stripping via `strip_wikilink`.
fn is_relation_string(field: &FieldDef) -> bool {
    matches!(
        (&field.role, &field.field_type),
        (FieldRole::Relation(info), FieldType::String)
            if matches!(info.kind, RelationKind::BelongsTo)
    )
}

/// Check if any field needs wikilink stripping.
fn needs_wikilink_stripping(fields: &[&FieldDef]) -> bool {
    fields.iter().any(|f| is_relation_vec(f) || is_relation_opt_string(f) || is_relation_string(f))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::model::{EntityDef, FieldDef, FieldRole, FieldType};

    fn make_role_entity() -> EntityDef {
        EntityDef {
            name: "Role".to_string(),
            directory: "role".to_string(),
            table: "roles".to_string(),
            type_name: "role".to_string(),
            prefix: "role".to_string(),
            fields: vec![
                FieldDef::new("id", FieldType::String, FieldRole::Id),
                FieldDef::new("body", FieldType::String, FieldRole::Body),
            ],
        }
    }

    #[test]
    fn test_update_struct_has_correct_fields() {
        let entity = make_role_entity();
        let mut code = String::new();
        generate_update_struct(&mut code, &entity);

        assert!(code.contains("pub struct RoleUpdate {"));
        assert!(code.contains("pub body: Option<String>"));
        // id should NOT be in the update struct
        assert!(!code.contains("pub id:"));
    }

    #[test]
    fn test_apply_method_generated() {
        let entity = make_role_entity();
        let mut code = String::new();
        generate_apply_method(&mut code, &entity);

        assert!(code.contains("fn apply(&self, role: &mut Role)"));
        assert!(code.contains("role.body.clone_from(body)"));
    }

    #[test]
    fn test_from_create_input() {
        let entity = make_role_entity();
        let mut code = String::new();
        generate_from_create_input(&mut code, &entity);

        assert!(code.contains("impl From<crate::schema::CreateRoleInput> for Role"));
        assert!(code.contains("id: input.id"));
        assert!(code.contains("body: input.body"));
    }

    #[test]
    fn test_from_update_input() {
        let entity = make_role_entity();
        let mut code = String::new();
        generate_from_update_input(&mut code, &entity);

        assert!(code.contains("impl From<crate::schema::UpdateRoleInput> for RoleUpdate"));
        assert!(code.contains("body: input.body"));
    }

    /// Ensures the combined Update-related generators produce syntactically valid Rust.
    /// Each emits top-level items (struct, impl blocks); together they form a parseable file.
    /// `syn::parse_file` does no name resolution, so `crate::schema::...` paths are fine.
    #[test]
    fn generated_code_is_valid_rust() {
        let entity = make_role_entity();
        let mut code = String::new();
        generate_update_struct(&mut code, &entity);
        generate_apply_method(&mut code, &entity);
        generate_from_update_input(&mut code, &entity);
        generate_from_create_input(&mut code, &entity);

        syn::parse_file(&code)
            .unwrap_or_else(|e| panic!("store/gen_update emitted invalid Rust: {e}\n--- code ---\n{code}"));
    }
}
