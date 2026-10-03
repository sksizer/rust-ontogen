//! OKF 0.2 frontmatter-key checks for the markdown generator.
//!
//! OKF fixes the meaning of some frontmatter keys (§4.1, §5). `type` is the
//! generator's own, so a field stored there is an error. The others are
//! warnings when the field's shape cannot carry what OKF means by the key:
//! a vault with its own vocabulary under `status` is legitimate, just not
//! OKF-conformant for that key. Checked here rather than in the schema
//! parser because only the markdown backend writes frontmatter; a
//! SeaORM-only schema may name a field `status` freely.

use std::collections::HashMap;

use crate::persistence::markdown::gen_frontmatter::frontmatter_fields;
use crate::schema::model::{EntityDef, EnumDef, FieldDef, FieldRole, FieldType, RelationKind};

/// The fix every diagnostic points at.
const RENAME_HINT: &str = "#[ontology(frontmatter_name = \"...\")]";

/// What [`check_frontmatter_keys`] found: errors fail generation, warnings
/// are printed as `cargo:warning=` lines.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct KeyDiagnostics {
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

/// Check every frontmatter field's effective key
/// ([`FieldDef::frontmatter_key`]) against the keys OKF reserves, and the
/// keys of one entity against each other. `enums` resolves enum-typed
/// fields for the `status` check.
pub fn check_frontmatter_keys(entities: &[EntityDef], enums: &[EnumDef]) -> KeyDiagnostics {
    let mut out = KeyDiagnostics::default();
    for entity in entities {
        for field in &entity.fields {
            if field.frontmatter_name.is_some()
                && let Some(why) = not_in_frontmatter(field)
            {
                out.errors.push(format!(
                    "entity `{}`, field `{}`: frontmatter_name has no effect, because {why}",
                    entity.name, field.name
                ));
            }
        }

        let mut seen: HashMap<&str, &str> = HashMap::new();
        for field in frontmatter_fields(entity) {
            let key = field.frontmatter_key();
            if let Some(first) = seen.insert(key, &field.name) {
                out.errors.push(format!(
                    "entity `{}`: fields `{first}` and `{}` are both stored under frontmatter key `{key}`; \
                     give one of them another key with {RENAME_HINT}",
                    entity.name, field.name
                ));
            }
            match check_key(field, key, enums) {
                Some(Finding::Error(msg)) => {
                    out.errors.push(format!("entity `{}`, field `{}`: {msg}", entity.name, field.name))
                }
                Some(Finding::Warning(msg)) => {
                    out.warnings.push(format!("ontogen: entity `{}`, field `{}`: {msg}", entity.name, field.name))
                }
                None => {}
            }
        }
    }
    out
}

enum Finding {
    Error(String),
    Warning(String),
}

/// Why a field never reaches frontmatter, so renaming its key means nothing.
fn not_in_frontmatter(field: &FieldDef) -> Option<&'static str> {
    match &field.role {
        FieldRole::Id => Some("the id is the record's filename, not a frontmatter key"),
        FieldRole::Body => Some("the body is the markdown after the frontmatter"),
        FieldRole::Skip => Some("the field is skipped"),
        FieldRole::Relation(info) if info.kind == RelationKind::HasMany => {
            Some("a has_many view is derived from the children and never stored")
        }
        _ => None,
    }
}

fn check_key(field: &FieldDef, key: &str, enums: &[EnumDef]) -> Option<Finding> {
    let ty = &field.field_type;
    let found = format!("`{}`", rust_type(ty));
    match key {
        "type" => Some(Finding::Error(format!(
            "frontmatter key `type` is reserved: the generator writes each record's OKF type there. \
             Store the field under another key with {RENAME_HINT}"
        ))),
        "status" => {
            let Some(def) = field.enum_def(enums) else {
                return warn(key, "a lifecycle state (draft, stable or deprecated)", "an enum of those values", &found);
            };
            let values: Vec<&str> = def.variants.iter().map(|v| v.value.as_str()).collect();
            if values.iter().all(|v| matches!(*v, "draft" | "stable" | "deprecated")) {
                return None;
            }
            warn(
                key,
                "a lifecycle state (draft, stable or deprecated)",
                "an enum of those values",
                &format!("{found} with values {}", values.join(", ")),
            )
        }
        "resource" | "stale_after" if !matches!(ty, FieldType::String | FieldType::OptionString) => {
            let purpose = if key == "resource" {
                "the URI of the asset a concept describes"
            } else {
                "the instant a concept goes stale (an ISO 8601 datetime)"
            };
            warn(key, purpose, "`String` or `Option<String>`", &found)
        }
        "generated" if !is_struct(ty, enums) => {
            warn(key, "how the content was produced (a mapping of `by` and `at`)", "a struct", &found)
        }
        "usage_window" if !is_struct(ty, enums) => {
            warn(key, "the window usage counts cover (a mapping of `from` and `to`)", "a struct", &found)
        }
        // OKF allows one verification as a bare mapping or several as a list.
        "verified" if !is_struct(ty, enums) && !matches!(ty, FieldType::VecStruct(_)) => warn(
            key,
            "who confirmed the content (one or a list of mappings of `by` and `at`)",
            "a struct or a `Vec` of structs",
            &found,
        ),
        "sources" if !matches!(ty, FieldType::VecStruct(_)) => {
            warn(key, "the materials a concept derives from (a list of mappings)", "a `Vec` of structs", &found)
        }
        _ => None,
    }
}

fn warn(key: &str, purpose: &str, expected: &str, found: &str) -> Option<Finding> {
    Some(Finding::Warning(format!(
        "frontmatter key `{key}` is reserved by OKF for {purpose}; expected {expected}, found {found}. \
         The vault is not OKF-conformant for `{key}` until the field is stored under another key with {RENAME_HINT}"
    )))
}

/// A named type that is not a schema enum: what `classify_type` leaves for
/// structs (and `Option<Struct>`). Lowercase names are primitives such as
/// `u32` or `char`, and generic or path types are not plain structs.
fn is_struct(ty: &FieldType, enums: &[EnumDef]) -> bool {
    match ty {
        FieldType::Other(name) | FieldType::OptionEnum(name) => {
            let mut chars = name.chars();
            chars.next().is_some_and(|c| c.is_ascii_uppercase())
                && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
                && !enums.iter().any(|e| &e.name == name)
        }
        _ => false,
    }
}

/// The field's Rust type as the schema author wrote it, for messages.
fn rust_type(ty: &FieldType) -> String {
    match ty {
        FieldType::String => "String".into(),
        FieldType::OptionString => "Option<String>".into(),
        FieldType::OptionEnum(name) => format!("Option<{name}>"),
        FieldType::VecString => "Vec<String>".into(),
        FieldType::VecStruct(name) => format!("Vec<{name}>"),
        FieldType::I32 => "i32".into(),
        FieldType::OptionI32 => "Option<i32>".into(),
        FieldType::I64 => "i64".into(),
        FieldType::OptionI64 => "Option<i64>".into(),
        FieldType::F32 => "f32".into(),
        FieldType::OptionF32 => "Option<f32>".into(),
        FieldType::F64 => "f64".into(),
        FieldType::OptionF64 => "Option<f64>".into(),
        FieldType::Bool => "bool".into(),
        FieldType::OptionBool => "Option<bool>".into(),
        FieldType::Other(name) => name.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::model::{EnumVariant, RelationInfo};

    fn entity(fields: Vec<FieldDef>) -> EntityDef {
        let mut all = vec![FieldDef::new("id", FieldType::String, FieldRole::Id)];
        all.extend(fields);
        EntityDef {
            name: "Task".into(),
            doc: String::new(),
            directory: "tasks".into(),
            table: "tasks".into(),
            type_name: "Task".into(),
            prefix: "task".into(),
            fields: all,
        }
    }

    fn plain(name: &str, ty: FieldType) -> FieldDef {
        FieldDef::new(name, ty, FieldRole::Plain)
    }

    fn renamed(name: &str, ty: FieldType, key: &str) -> FieldDef {
        FieldDef { frontmatter_name: Some(key.into()), ..plain(name, ty) }
    }

    fn string_enum(name: &str, values: &[&str]) -> EnumDef {
        EnumDef {
            name: name.into(),
            variants: values.iter().map(|v| EnumVariant { name: v.to_uppercase(), value: (*v).into() }).collect(),
        }
    }

    fn check(fields: Vec<FieldDef>, enums: &[EnumDef]) -> KeyDiagnostics {
        check_frontmatter_keys(&[entity(fields)], enums)
    }

    #[test]
    fn a_string_status_warns_exactly_once_and_a_rename_clears_it() {
        let found = check(vec![plain("title", FieldType::String), plain("status", FieldType::String)], &[]);
        assert!(found.errors.is_empty(), "{found:?}");
        assert_eq!(found.warnings.len(), 1, "{found:?}");
        let warning = &found.warnings[0];
        for part in [
            "ontogen: entity `Task`, field `status`",
            "frontmatter key `status` is reserved by OKF for a lifecycle state (draft, stable or deprecated)",
            "found `String`",
            "#[ontology(frontmatter_name = \"...\")]",
        ] {
            assert!(warning.contains(part), "missing {part:?} in {warning}");
        }

        let clean = check(vec![renamed("status", FieldType::String, "task_status")], &[]);
        assert_eq!(clean, KeyDiagnostics::default());
    }

    #[test]
    fn an_okf_vocabulary_status_enum_is_conformant() {
        let lifecycle = string_enum("Lifecycle", &["draft", "stable", "deprecated"]);
        let subset = string_enum("Maturity", &["draft", "stable"]);
        let enums = [lifecycle, subset];
        assert_eq!(
            check(vec![plain("status", FieldType::Other("Lifecycle".into()))], &enums),
            KeyDiagnostics::default()
        );
        assert_eq!(
            check(vec![plain("status", FieldType::OptionEnum("Maturity".into()))], &enums),
            KeyDiagnostics::default()
        );
    }

    #[test]
    fn a_status_enum_with_other_values_warns_and_names_them() {
        let enums = [string_enum("TaskStatus", &["open", "draft", "closed"])];
        let found = check(vec![plain("status", FieldType::Other("TaskStatus".into()))], &enums);
        assert_eq!(found.warnings.len(), 1, "{found:?}");
        assert!(found.warnings[0].contains("found `TaskStatus` with values open, draft, closed"), "{found:?}");
    }

    #[test]
    fn a_type_key_is_an_error_however_it_arises() {
        for field in [
            plain("type", FieldType::String),
            plain("r#type", FieldType::OptionString),
            renamed("kind", FieldType::String, "type"),
        ] {
            let name = field.name.clone();
            let found = check(vec![field], &[]);
            assert_eq!(found.errors.len(), 1, "{name}: {found:?}");
            assert!(found.errors[0].contains(&format!("entity `Task`, field `{name}`")), "{found:?}");
            assert!(found.errors[0].contains("frontmatter key `type` is reserved"), "{found:?}");
            assert!(found.errors[0].contains("frontmatter_name"), "{found:?}");
        }
    }

    #[test]
    fn colliding_effective_keys_are_an_error() {
        let found = check(vec![plain("state", FieldType::String), renamed("status", FieldType::String, "state")], &[]);
        assert_eq!(found.errors.len(), 1, "{found:?}");
        assert!(found.errors[0].contains("fields `state` and `status` are both stored under frontmatter key `state`"));

        let found = check(vec![renamed("a", FieldType::String, "same"), renamed("b", FieldType::String, "same")], &[]);
        assert_eq!(found.errors.len(), 1, "{found:?}");
    }

    #[test]
    fn string_keys_accept_only_strings() {
        for key in ["resource", "stale_after"] {
            for ok in [FieldType::String, FieldType::OptionString] {
                assert_eq!(check(vec![plain(key, ok)], &[]), KeyDiagnostics::default());
            }
            for bad in [FieldType::I64, FieldType::Other("DateTime < Utc >".into()), FieldType::VecString] {
                let found = check(vec![plain(key, bad)], &[]);
                assert_eq!(found.warnings.len(), 1, "{key}: {found:?}");
                assert!(found.warnings[0].contains("expected `String` or `Option<String>`"), "{found:?}");
            }
        }
    }

    #[test]
    fn mapping_keys_accept_only_structs() {
        let enums = [string_enum("Origin", &["human", "machine"])];
        for key in ["generated", "usage_window", "verified"] {
            for ok in [FieldType::Other("Stamp".into()), FieldType::OptionEnum("Stamp".into())] {
                assert_eq!(check(vec![plain(key, ok)], &enums), KeyDiagnostics::default(), "{key}");
            }
            for bad in [
                FieldType::String,
                FieldType::Other("Origin".into()),
                FieldType::OptionEnum("Origin".into()),
                FieldType::Other("u32".into()),
                FieldType::VecString,
            ] {
                let found = check(vec![plain(key, bad.clone())], &enums);
                assert_eq!(found.warnings.len(), 1, "{key} {bad:?}: {found:?}");
            }
        }
        // OKF writes several verifications as a list.
        assert_eq!(
            check(vec![plain("verified", FieldType::VecStruct("Stamp".into()))], &enums),
            KeyDiagnostics::default()
        );
        assert_eq!(check(vec![plain("generated", FieldType::VecStruct("Stamp".into()))], &enums).warnings.len(), 1);
    }

    #[test]
    fn sources_accept_only_a_list_of_structs() {
        assert_eq!(
            check(vec![plain("sources", FieldType::VecStruct("Source".into()))], &[]),
            KeyDiagnostics::default()
        );
        for bad in [FieldType::VecString, FieldType::Other("Source".into()), FieldType::String] {
            assert_eq!(check(vec![plain("sources", bad)], &[]).warnings.len(), 1);
        }
    }

    #[test]
    fn title_description_and_tags_are_never_flagged() {
        let found = check(
            vec![
                plain("title", FieldType::I64),
                plain("description", FieldType::VecString),
                plain("tags", FieldType::String),
            ],
            &[],
        );
        assert_eq!(found, KeyDiagnostics::default());
    }

    #[test]
    fn fields_outside_frontmatter_are_not_checked_but_cannot_be_renamed() {
        let has_many = FieldRole::Relation(RelationInfo {
            kind: RelationKind::HasMany,
            target: "Task".into(),
            junction: None,
            foreign_key: Some("parent_id".into()),
        });
        let found = check(
            vec![
                FieldDef::new("status", FieldType::VecString, has_many.clone()),
                FieldDef::new("sources", FieldType::String, FieldRole::Skip),
            ],
            &[],
        );
        assert_eq!(found, KeyDiagnostics::default(), "a derived view or skipped field has no frontmatter key");

        for (role, why) in [
            (FieldRole::Body, "the body is the markdown after the frontmatter"),
            (has_many, "a has_many view is derived"),
            (FieldRole::Skip, "skipped"),
        ] {
            let field =
                FieldDef { frontmatter_name: Some("other".into()), ..FieldDef::new("x", FieldType::String, role) };
            let found = check(vec![field], &[]);
            assert_eq!(found.errors.len(), 1, "{found:?}");
            assert!(found.errors[0].contains("frontmatter_name has no effect") && found.errors[0].contains(why));
        }
    }

    #[test]
    fn a_renamed_relation_is_checked_by_its_effective_key() {
        let belongs_to = FieldRole::Relation(RelationInfo {
            kind: RelationKind::BelongsTo,
            target: "Epic".into(),
            junction: None,
            foreign_key: None,
        });
        let field = FieldDef {
            frontmatter_name: Some("type".into()),
            ..FieldDef::new("epic_id", FieldType::OptionString, belongs_to)
        };
        assert_eq!(check(vec![field], &[]).errors.len(), 1);
    }

    #[test]
    fn generation_fails_on_an_error_before_writing_anything() {
        let schema =
            crate::ir::SchemaOutput { entities: vec![entity(vec![plain("type", FieldType::String)])], enums: vec![] };
        let tmp = tempfile::tempdir().unwrap();
        let out = tmp.path().join("generated");
        let config = crate::MarkdownIoConfig {
            output_dir: out.clone(),
            vault_root: "data/vault".into(),
            layout: crate::ir::MarkdownLayout::PerEntityDir,
            id_strategy: crate::ir::IdStrategy::Provided,
            list_cap: 10_000,
        };
        let err = crate::gen_markdown_io(&schema, &config).unwrap_err();
        assert!(matches!(&err, crate::CodegenError::Persistence(msg) if msg.contains("`type` is reserved")), "{err}");
        assert!(!out.exists(), "nothing is generated for a schema the vault could not hold");
    }
}
