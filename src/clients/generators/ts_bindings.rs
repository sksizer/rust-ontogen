//! Schema-known emitter: TS type aliases for entities and their generated
//! Create/Update DTOs, written straight from `EntityDef`. Bounded mapping
//! over `FieldType` — no AST walking, no external tooling.
//!
//! The long-tail (user-owned types referenced by custom API endpoints) is
//! handled separately by the `ontogen-ts` crate (built-in AST walker; see
//! `crates/ontogen-ts/`). The two emitters write to the same `bindings.ts`
//! file: this module first, then ontogen-ts appends.

use std::collections::HashSet;

use ontogen_core::model::{EntityDef, FieldRole, FieldType};

use crate::clients::config::Config;
use crate::clients::generators::command_name;
use crate::servers::parse::ApiModule;
use crate::servers::types::{collect_ts_import, extract_input_type, rust_type_to_ts};

/// Collect every TS type name referenced by the generated client surface
/// (return types + parameter types of every emitted command). Mirrors the
/// inline collection currently done in `ts_client::generate` and
/// `transport::generate`.
pub fn referenced_ts_types(modules: &[ApiModule], config: &Config) -> Vec<String> {
    let mut import_types: Vec<String> = Vec::new();
    for m in modules {
        import_types.extend(module_referenced_ts_types(m, config));
    }
    import_types.sort();
    import_types.dedup();
    import_types
}

/// The TS type names referenced by a single module's command signatures.
///
/// Split out from [`referenced_ts_types`] so the long-tail resolver can map
/// each referenced name back to the module that referenced it — needed to
/// resolve a bare name (`VaultConfig`) through *that* module's `use` imports
/// rather than guessing by terminal segment.
pub fn module_referenced_ts_types(m: &ApiModule, config: &Config) -> Vec<String> {
    let mut import_types: Vec<String> = Vec::new();
    if m.functions.is_empty() {
        return import_types;
    }
    for f in &m.functions {
        let cmd_name = command_name(&m.name, f, config);
        if cmd_name.is_empty() || config.ts_skip_commands.contains(&cmd_name) {
            continue;
        }
        let ts_ret = rust_type_to_ts(&f.return_type);
        collect_ts_import(&ts_ret, &mut import_types);
        for p in &f.params {
            let ty = extract_input_type(&p.ty);
            let ts_ty = rust_type_to_ts(&ty);
            collect_ts_import(&ts_ty, &mut import_types);
        }
    }
    import_types
}

/// Names that the schema-known emitter (this module's `emit`) writes to
/// bindings.ts: the entity itself plus its Create/Update DTOs.
pub fn schema_known_names(entities: &[EntityDef]) -> HashSet<String> {
    let mut set = HashSet::new();
    for e in entities {
        set.insert(e.name.clone());
        set.insert(format!("Create{}Input", e.name));
        set.insert(format!("Update{}Input", e.name));
    }
    set
}

/// Type idents referenced by `EntityDef.fields` that point at user-defined
/// types (i.e. NOT primitives, NOT containers, NOT qualified paths).
///
/// These idents are what the schema-known emitter renders into the entity
/// body as bare TS types — e.g. `interval_kind: IntervalKind` for an
/// `Option<IntervalKind>` field. The schema-known emitter does NOT emit
/// the body of `IntervalKind` itself; that's the long-tail emitter's job.
/// This function surfaces those idents so `long_tail` can union them into
/// its root set and `ontogen-ts::emit` will emit their bodies.
///
/// Caller is expected to filter the result against `schema_known_names`
/// (since an entity field can reference another entity by name — the
/// schema-known emitter already handles that case).
pub fn entity_field_type_names(entities: &[EntityDef]) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for entity in entities {
        for field in &entity.fields {
            if !include_in_entity(field) {
                continue;
            }
            if let Some(name) = field_type_user_ident(&field.field_type)
                && !names.contains(&name)
            {
                names.push(name);
            }
        }
    }
    names
}

/// Extract a single user-defined ident from a `FieldType`, or `None` if
/// the field type is a primitive/container/qualified path that the
/// long-tail emitter has no business walking.
///
/// - `OptionEnum(name)` / `VecStruct(name)`: the macro-side classifier
///   already pulled out the inner ident; pass it through directly.
/// - `Other(rendered)`: only accept if `rendered` is a plain
///   single-segment Rust ident (no `::`, no `<`, no whitespace, no `&`,
///   not a primitive scalar). Anything more complex is either a qualified
///   path (handled via the API surface's `collect_type_import` machinery
///   elsewhere) or a generic shape we deliberately don't try to walk.
/// - All other variants: primitives, return `None`.
fn field_type_user_ident(ft: &FieldType) -> Option<String> {
    match ft {
        FieldType::OptionEnum(name) | FieldType::VecStruct(name) => {
            if is_simple_user_ident(name) {
                Some(name.clone())
            } else {
                None
            }
        }
        FieldType::Other(rendered) => {
            let trimmed = rendered.trim();
            if is_simple_user_ident(trimmed) { Some(trimmed.to_string()) } else { None }
        }
        FieldType::String
        | FieldType::OptionString
        | FieldType::VecString
        | FieldType::I32
        | FieldType::OptionI32
        | FieldType::I64
        | FieldType::OptionI64
        | FieldType::F32
        | FieldType::OptionF32
        | FieldType::F64
        | FieldType::OptionF64
        | FieldType::Bool
        | FieldType::OptionBool => None,
    }
}

/// True when `s` looks like a single-segment Rust ident that names a
/// user-defined type (so suitable as a long-tail root candidate).
///
/// Rejects: empty strings, anything containing `::`, `<`, `>`, `&`,
/// whitespace, or starting with a non-alphabetic char; rejects names that
/// match the Rust primitive scalars (those should already have been
/// classified into typed `FieldType` variants — defense in depth).
fn is_simple_user_ident(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    if !s.chars().next().unwrap().is_ascii_alphabetic() {
        return false;
    }
    if !s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return false;
    }
    !matches!(
        s,
        "bool"
            | "char"
            | "i8"
            | "i16"
            | "i32"
            | "i64"
            | "i128"
            | "isize"
            | "u8"
            | "u16"
            | "u32"
            | "u64"
            | "u128"
            | "usize"
            | "f32"
            | "f64"
            | "String"
            | "str"
            | "PathBuf"
            | "Path"
    )
}

/// Referenced types that are NOT covered by schema-known emission. The
/// returned names are the long-tail root set fed to `ontogen-ts::emit`
/// in `generate_transport`.
///
/// Roots are sourced from two places:
/// 1. **API surface** — return types and parameter types of every emitted
///    command, via [`referenced_ts_types`]. This catches custom DTOs that
///    appear in endpoint signatures.
/// 2. **Schema entity fields** — type idents referenced by `EntityDef`
///    fields that point at user-defined types (e.g. `IntervalKind`,
///    `SessionStatus`), via [`entity_field_type_names`]. The schema-known
///    emitter renders these as bare TS idents in the entity body but
///    never emits the body of the referenced type itself — that's the
///    long-tail emitter's job, and these names need to be roots for the
///    long-tail walker to reach them.
///
/// In both cases, names that the schema-known emitter already writes
/// (the entity types + generated DTOs) are filtered out so the long-tail
/// emitter doesn't double-emit them.
pub fn long_tail(modules: &[ApiModule], config: &Config, entities: &[EntityDef]) -> Vec<String> {
    let known = schema_known_names(entities);
    let mut merged: Vec<String> = Vec::new();
    for name in referenced_ts_types(modules, config) {
        if !known.contains(&name) && !merged.contains(&name) {
            merged.push(name);
        }
    }
    for name in entity_field_type_names(entities) {
        if !known.contains(&name) && !merged.contains(&name) {
            merged.push(name);
        }
    }
    merged
}

pub fn emit(entities: &[EntityDef]) -> String {
    let mut out = String::new();
    out.push_str("// Auto-generated by ontogen. DO NOT EDIT.\n");
    out.push_str("// Schema-known surface: entities + generated Create/Update DTOs.\n\n");
    for entity in entities {
        out.push_str(&emit_entity(entity));
        out.push('\n');
        out.push_str(&emit_create_dto(entity));
        out.push('\n');
        out.push_str(&emit_update_dto(entity));
        out.push('\n');
    }
    out
}

fn emit_entity(entity: &EntityDef) -> String {
    let mut out = format!("export type {} = {{\n", entity.name);
    for f in &entity.fields {
        if !include_in_entity(f) {
            continue;
        }
        out.push_str(&format!("  {}: {};\n", f.name, field_to_ts(&f.field_type)));
    }
    out.push_str("};\n");
    out
}

fn emit_create_dto(entity: &EntityDef) -> String {
    let mut out = format!("export type Create{}Input = {{\n", entity.name);
    for f in &entity.fields {
        if !include_in_dto(f) {
            continue;
        }
        out.push_str(&format!("  {}: {};\n", f.name, field_to_ts(&f.field_type)));
    }
    out.push_str("};\n");
    out
}

fn emit_update_dto(entity: &EntityDef) -> String {
    let mut out = format!("export type Update{}Input = {{\n", entity.name);
    for f in &entity.fields {
        if !include_in_dto(f) || matches!(f.role, FieldRole::Id) {
            continue;
        }
        out.push_str(&format!("  {}?: {} | null;\n", f.name, field_to_ts(&f.field_type)));
    }
    out.push_str("};\n");
    out
}

fn include_in_entity(f: &ontogen_core::model::FieldDef) -> bool {
    !matches!(f.role, FieldRole::Skip) && f.name != "wikilinks" && f.name != "source_file"
}

fn include_in_dto(f: &ontogen_core::model::FieldDef) -> bool {
    f.name != "wikilinks" && f.name != "source_file"
}

fn field_to_ts(ft: &FieldType) -> String {
    match ft {
        FieldType::String => "string".into(),
        FieldType::OptionString => "string | null".into(),
        FieldType::I32 | FieldType::I64 | FieldType::F32 | FieldType::F64 => "number".into(),
        FieldType::OptionI32 | FieldType::OptionI64 | FieldType::OptionF32 | FieldType::OptionF64 => {
            "number | null".into()
        }
        FieldType::Bool => "boolean".into(),
        FieldType::OptionBool => "boolean | null".into(),
        FieldType::VecString => "string[]".into(),
        FieldType::VecStruct(name) => format!("{name}[]"),
        FieldType::OptionEnum(name) => format!("{name} | null"),
        FieldType::Other(name) => name.clone(),
    }
}

#[cfg(test)]
mod tests {
    use ontogen_core::model::{EntityDef, FieldDef, FieldRole, FieldType};

    use super::{entity_field_type_names, field_type_user_ident, is_simple_user_ident, long_tail};
    use crate::clients::config::Config;

    fn empty_config() -> Config {
        Config {
            api_dir: std::path::PathBuf::from("/tmp/does-not-matter"),
            state_type: String::new(),
            service_import_path: String::new(),
            types_import_path: String::new(),
            state_import: String::new(),
            naming: Default::default(),
            generators: Vec::new(),
            sse_route_overrides: Default::default(),
            ts_skip_commands: Default::default(),
            route_prefix: None,
            store_type: None,
            store_import: None,
            schema_entities: Vec::new(),
            pagination: None,
            pool_extra_roots: Vec::new(),
            pool_exclude_paths: Vec::new(),
        }
    }

    fn entity_with_fields(name: &str, fields: Vec<FieldDef>) -> EntityDef {
        EntityDef {
            name: name.to_string(),
            directory: name.to_ascii_lowercase(),
            table: name.to_ascii_lowercase(),
            type_name: name.to_ascii_lowercase(),
            prefix: name.to_ascii_lowercase(),
            fields,
        }
    }

    #[test]
    fn is_simple_user_ident_accepts_pascal_case_ident() {
        assert!(is_simple_user_ident("IntervalKind"));
        assert!(is_simple_user_ident("SessionStatus"));
        assert!(is_simple_user_ident("MyType2"));
    }

    #[test]
    fn is_simple_user_ident_rejects_primitives() {
        assert!(!is_simple_user_ident("String"));
        assert!(!is_simple_user_ident("i64"));
        assert!(!is_simple_user_ident("bool"));
    }

    #[test]
    fn is_simple_user_ident_rejects_qualified_and_generic_shapes() {
        assert!(!is_simple_user_ident("chrono::DateTime"));
        assert!(!is_simple_user_ident("HashMap<String, i32>"));
        assert!(!is_simple_user_ident("Vec<Foo>"));
        assert!(!is_simple_user_ident("&str"));
        assert!(!is_simple_user_ident(""));
        assert!(!is_simple_user_ident("2Hot"));
    }

    #[test]
    fn field_type_user_ident_extracts_enum_and_vec_struct_names() {
        assert_eq!(field_type_user_ident(&FieldType::OptionEnum("IntervalKind".into())), Some("IntervalKind".into()));
        assert_eq!(
            field_type_user_ident(&FieldType::VecStruct("AcceptanceCriterion".into())),
            Some("AcceptanceCriterion".into())
        );
    }

    #[test]
    fn field_type_user_ident_ignores_primitives_and_strings() {
        assert_eq!(field_type_user_ident(&FieldType::String), None);
        assert_eq!(field_type_user_ident(&FieldType::OptionString), None);
        assert_eq!(field_type_user_ident(&FieldType::VecString), None);
        assert_eq!(field_type_user_ident(&FieldType::I64), None);
        assert_eq!(field_type_user_ident(&FieldType::Bool), None);
    }

    #[test]
    fn field_type_user_ident_handles_other_simple_ident_and_skips_complex_paths() {
        // Other(<simple ident>) → accepted as a user-defined ident.
        assert_eq!(field_type_user_ident(&FieldType::Other("MyOpaque".into())), Some("MyOpaque".into()));
        // Other(<qualified>) and Other(<generic>) → skipped; the API surface
        // import path handles qualified types via collect_type_import.
        assert_eq!(field_type_user_ident(&FieldType::Other("chrono :: DateTime < Utc >".into())), None);
        assert_eq!(field_type_user_ident(&FieldType::Other("HashMap < String , i32 >".into())), None);
    }

    #[test]
    fn entity_field_type_names_collects_user_idents_across_fields() {
        // Mirrors Pumice's TimerSession shape: an Option<IntervalKind>
        // field and an Option<SessionStatus> field, plus a plain String.
        let entity = entity_with_fields(
            "TimerSession",
            vec![
                FieldDef::new("id", FieldType::String, FieldRole::Id),
                FieldDef::new("interval_kind", FieldType::OptionEnum("IntervalKind".into()), FieldRole::EnumField),
                FieldDef::new("status", FieldType::OptionEnum("SessionStatus".into()), FieldRole::EnumField),
                FieldDef::new("note", FieldType::OptionString, FieldRole::Plain),
            ],
        );
        let names = entity_field_type_names(&[entity]);
        assert!(names.contains(&"IntervalKind".to_string()), "missing IntervalKind in {names:?}");
        assert!(names.contains(&"SessionStatus".to_string()), "missing SessionStatus in {names:?}");
        assert_eq!(names.len(), 2, "unexpected extras in {names:?}");
    }

    #[test]
    fn entity_field_type_names_skips_skip_role_fields() {
        // FieldRole::Skip fields are not part of the schema-known body, so
        // they shouldn't contribute roots either — the long-tail walker
        // would have nothing to emit them into.
        let entity = entity_with_fields(
            "Thing",
            vec![
                FieldDef::new("id", FieldType::String, FieldRole::Id),
                FieldDef::new("ignored", FieldType::OptionEnum("HiddenEnum".into()), FieldRole::Skip),
            ],
        );
        let names = entity_field_type_names(&[entity]);
        assert!(names.is_empty(), "expected no roots from Skip field, got {names:?}");
    }

    #[test]
    fn long_tail_includes_entity_field_type_idents_not_in_schema_known_surface() {
        // No API modules → API-derived names are empty. Entity has a field
        // pointing at IntervalKind, which is not schema-known. long_tail
        // must return it so the long-tail emitter promotes it to a root.
        let entity = entity_with_fields(
            "TimerSession",
            vec![
                FieldDef::new("id", FieldType::String, FieldRole::Id),
                FieldDef::new("interval_kind", FieldType::OptionEnum("IntervalKind".into()), FieldRole::EnumField),
            ],
        );
        let entities = vec![entity];
        let names = long_tail(&[], &empty_config(), &entities);
        assert!(names.contains(&"IntervalKind".to_string()), "missing IntervalKind in {names:?}");
    }

    #[test]
    fn long_tail_excludes_entity_names_already_emitted_by_schema_known() {
        // An entity field referencing ANOTHER entity (e.g. has-many target)
        // resolves to a name the schema-known emitter already writes — it
        // must not appear in long_tail or ontogen-ts would double-emit.
        let other = entity_with_fields("Other", vec![FieldDef::new("id", FieldType::String, FieldRole::Id)]);
        let parent = entity_with_fields(
            "Parent",
            vec![
                FieldDef::new("id", FieldType::String, FieldRole::Id),
                // Imagine an unusual schema field that just renders as the
                // bare entity name — `Other` is in schema_known_names so it
                // must be filtered out.
                FieldDef::new("rel", FieldType::Other("Other".into()), FieldRole::Plain),
            ],
        );
        let entities = vec![other, parent];
        let names = long_tail(&[], &empty_config(), &entities);
        assert!(!names.contains(&"Other".to_string()), "schema-known name leaked into long_tail: {names:?}");
    }
}
