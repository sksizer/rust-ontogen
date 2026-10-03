//! OKF 0.2 build-time checks for the markdown generator.
//!
//! OKF fixes the meaning of some frontmatter keys (§4.1, §5). `type` is the
//! generator's own, so a field stored there is an error, and so is one
//! stored under `generated` when `okf.generated_by` makes the vault stamp
//! it. The others are warnings when the field's shape cannot carry what OKF
//! means by the key: a vault with its own vocabulary under `status` is
//! legitimate, just not OKF-conformant for that key. Checked here rather
//! than in the schema parser because only the markdown backend writes
//! frontmatter; a SeaORM-only schema may name a field `status` freely.
//!
//! The `okf.generated_by` actor is checked here too, so a vault never
//! stamps a value OKF consumers would misread.

use std::collections::HashMap;

use crate::ir::OkfOptions;
use crate::persistence::markdown::gen_frontmatter::frontmatter_fields;
use crate::schema::model::{EntityDef, EnumDef, FieldDef, FieldRole, FieldType, RelationKind};

/// The fix every diagnostic points at.
const RENAME_HINT: &str = "#[ontology(frontmatter_name = \"...\")]";

/// Named in full because a consumer meeting a warning may never have heard
/// of the format.
const OKF: &str = "OKF (Open Knowledge Format) 0.2";
const OKF_SPEC: &str = "https://github.com/GoogleCloudPlatform/knowledge-catalog/blob/main/okf/SPEC.md";

/// What [`check_frontmatter_keys`] found: errors fail generation, warnings
/// are printed as `cargo:warning=` lines.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct KeyDiagnostics {
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

/// Check every frontmatter field's effective key
/// ([`FieldDef::frontmatter_key`]) against the keys OKF reserves, and the
/// keys of one entity against each other. `enums` resolves enum-typed
/// fields for the `status` check; `okf` says which keys the vault itself
/// writes.
pub(crate) fn check_frontmatter_keys(entities: &[EntityDef], enums: &[EnumDef], okf: &OkfOptions) -> KeyDiagnostics {
    let stamps_generated = okf.generated_by.is_some();
    let mut out = KeyDiagnostics::default();
    for entity in entities {
        let stored = frontmatter_fields(entity);
        for field in &entity.fields {
            if field.frontmatter_name.is_some() && !stored.iter().any(|f| std::ptr::eq(*f, field)) {
                out.errors.push(format!(
                    "entity `{}`, field `{}`: frontmatter_name has no effect, because {}",
                    entity.name,
                    field.name,
                    why_not_stored(field)
                ));
            }
        }

        let mut seen: HashMap<&str, &str> = HashMap::new();
        for field in stored {
            let key = field.frontmatter_key();
            if let Some(first) = seen.insert(key, &field.name) {
                out.errors.push(format!(
                    "entity `{}`: fields `{first}` and `{}` are both stored under frontmatter key `{key}`; \
                     give one of them another key with {RENAME_HINT}",
                    entity.name, field.name
                ));
            }
            match check_key(field, key, enums, stamps_generated) {
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

/// Why a field that [`frontmatter_fields`] leaves out never reaches
/// frontmatter, so renaming its key means nothing.
fn why_not_stored(field: &FieldDef) -> &'static str {
    match &field.role {
        FieldRole::Id => "the id is the record's filename, not a frontmatter key",
        FieldRole::Body => "the body is the markdown after the frontmatter",
        FieldRole::Skip => "the field is skipped",
        FieldRole::Relation(info) if info.kind == RelationKind::HasMany => {
            "a has_many view is derived from the children and never stored"
        }
        _ => "the markdown backend never stores this field in frontmatter",
    }
}

fn check_key(field: &FieldDef, key: &str, enums: &[EnumDef], stamps_generated: bool) -> Option<Finding> {
    let ty = &field.field_type;
    let found = format!("`{}`", rust_type(ty));
    match key {
        "type" => Some(Finding::Error(format!(
            "frontmatter key `type` is reserved: the generator writes each record's {OKF} type there. \
             Store the field under another key with {RENAME_HINT} (spec: {OKF_SPEC})"
        ))),
        "generated" if stamps_generated => Some(Finding::Error(format!(
            "frontmatter key `generated` is reserved while `okf.generated_by` is set: the vault stamps each \
             record's {OKF} provenance there on every write. Store the field under another key with \
             {RENAME_HINT}, or unset `okf.generated_by` (spec: {OKF_SPEC})"
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
        "verified" if !is_struct(ty, enums) && !is_struct_list(ty, enums) => warn(
            key,
            "who confirmed the content (one or a list of mappings of `by` and `at`)",
            "a struct or a `Vec` of structs",
            &found,
        ),
        "sources" if !is_struct_list(ty, enums) => {
            warn(key, "the materials a concept derives from (a list of mappings)", "a `Vec` of structs", &found)
        }
        _ => None,
    }
}

/// Check an `okf.generated_by` value against the OKF §7 actor convention.
///
/// The vault's writer is a program, so the value must name one:
/// `<producer>/<version>` (non-empty on both sides of the first `/`) or
/// `process:<id>` (non-empty id). `human:<id>` is refused because OKF trust
/// tiers (§5.3) read it as a person's sign-off. No actor may contain
/// whitespace or control characters. Returns the build error on failure.
pub(crate) fn check_generated_by(actor: &str) -> Result<(), String> {
    let problem = if actor.is_empty() {
        Some("it is empty")
    } else if actor.chars().any(|c| c.is_whitespace() || c.is_control()) {
        Some("it contains whitespace or a control character")
    } else if actor.get(..6).is_some_and(|prefix| prefix.eq_ignore_ascii_case("human:")) {
        Some(
            "`human:` actors mark content a person wrote or confirmed, and OKF trust tiers read them that way; \
             the vault's writer is a program",
        )
    } else if let Some(id) = actor.strip_prefix("process:") {
        id.is_empty().then_some("`process:` needs an id after the colon")
    } else if let Some((producer, version)) = actor.split_once('/') {
        (producer.is_empty() || version.is_empty())
            .then_some("`<producer>/<version>` needs both a producer and a version")
    } else {
        Some("it is neither `<producer>/<version>` nor `process:<id>`")
    };
    match problem {
        None => Ok(()),
        Some(problem) => Err(format!(
            "okf.generated_by = {actor:?} is not an {OKF} actor for a program: {problem}. \
             Use `<producer>/<version>` (e.g. `my-app/1.2.0`) or `process:<id>` (spec §7: {OKF_SPEC})"
        )),
    }
}

fn warn(key: &str, purpose: &str, expected: &str, found: &str) -> Option<Finding> {
    Some(Finding::Warning(format!(
        "frontmatter key `{key}` is reserved by {OKF} for {purpose}; expected {expected}, found {found}. \
         The vault is not OKF-conformant for `{key}` until the field is stored under another key with \
         {RENAME_HINT} (spec: {OKF_SPEC})"
    )))
}

/// Bare type names that can name a struct field's type but never a struct.
/// `classify_type` keeps only the last path segment of a generic argument,
/// so `Option<Vec<T>>` arrives as `OptionEnum("Vec")` and
/// `Vec<chrono::DateTime<Utc>>` as `VecStruct("DateTime")`. Smart pointers
/// are absent on purpose: `Option<Box<Stamp>>` still holds an object.
const NOT_STRUCTS: &[&str] = &[
    "Vec",
    "VecDeque",
    "HashMap",
    "BTreeMap",
    "IndexMap",
    "HashSet",
    "BTreeSet",
    "Option",
    "Cow",
    "DateTime",
    "NaiveDate",
    "NaiveDateTime",
    "NaiveTime",
    "Duration",
    "Uuid",
    "Value",
    "Url",
    "PathBuf",
];

/// A struct or `Option<Struct>`: what `classify_type` reports as a bare
/// named type.
fn is_struct(ty: &FieldType, enums: &[EnumDef]) -> bool {
    matches!(ty, FieldType::Other(name) | FieldType::OptionEnum(name) if is_struct_name(name, enums))
}

/// A `Vec` of structs. `classify_type` reports every non-`String` element
/// type as `VecStruct`, primitives and enums included.
fn is_struct_list(ty: &FieldType, enums: &[EnumDef]) -> bool {
    matches!(ty, FieldType::VecStruct(name) if is_struct_name(name, enums))
}

/// Lowercase names are primitives such as `u32` or `char`; a name with
/// generics or a path (`Other` keeps the whole tokenized type) is not a
/// plain struct.
fn is_struct_name(name: &str, enums: &[EnumDef]) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_uppercase())
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !NOT_STRUCTS.contains(&name)
        && !enums.iter().any(|e| e.name == name)
}

/// The field's Rust type as far as `FieldType` records it, for messages.
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
            id_strategy: None,
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
        check_frontmatter_keys(&[entity(fields)], enums, &OkfOptions::default())
    }

    fn stamping() -> OkfOptions {
        OkfOptions { generated_by: Some("app/1.0".into()), ..OkfOptions::default() }
    }

    #[test]
    fn a_string_status_warns_exactly_once_and_a_rename_clears_it() {
        let found = check(vec![plain("title", FieldType::String), plain("status", FieldType::String)], &[]);
        assert!(found.errors.is_empty(), "{found:?}");
        assert_eq!(found.warnings.len(), 1, "{found:?}");
        let warning = &found.warnings[0];
        for part in [
            "ontogen: entity `Task`, field `status`",
            "frontmatter key `status` is reserved by OKF (Open Knowledge Format) 0.2 for a lifecycle state \
             (draft, stable or deprecated)",
            "found `String`",
            "#[ontology(frontmatter_name = \"...\")]",
            "(spec: https://github.com/GoogleCloudPlatform/knowledge-catalog/blob/main/okf/SPEC.md)",
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
            assert!(found.errors[0].contains("OKF (Open Knowledge Format) 0.2 type"), "{found:?}");
            assert!(found.errors[0].contains("frontmatter_name"), "{found:?}");
            assert!(found.errors[0].contains(OKF_SPEC), "{found:?}");
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
            // `OptionEnum("Box")` is how `Option<Box<Stamp>>` arrives.
            for ok in [
                FieldType::Other("Stamp".into()),
                FieldType::OptionEnum("Stamp".into()),
                FieldType::OptionEnum("Box".into()),
            ] {
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

    /// `classify_type` reduces a generic argument to its last path segment,
    /// so containers and well-known value types arrive looking like structs.
    #[test]
    fn mapping_keys_reject_containers_and_value_types_that_look_like_structs() {
        // `Option<Vec<Stamp>>` and `Option<chrono::DateTime<Utc>>`.
        for (key, bad) in [("generated", "Vec"), ("usage_window", "DateTime"), ("verified", "HashMap")] {
            let found = check(vec![plain(key, FieldType::OptionEnum(bad.into()))], &[]);
            assert_eq!(found.warnings.len(), 1, "{key} Option<{bad}>: {found:?}");
            assert!(found.warnings[0].contains(&format!("found `Option<{bad}>`")), "{found:?}");
        }
        for bare in ["Uuid", "NaiveDate", "Value"] {
            assert_eq!(check(vec![plain("generated", FieldType::Other(bare.into()))], &[]).warnings.len(), 1, "{bare}");
        }
        // A struct whose name merely starts like a container still passes.
        assert_eq!(
            check(vec![plain("usage_window", FieldType::OptionEnum("DateTimeWindow".into()))], &[]),
            KeyDiagnostics::default()
        );
    }

    #[test]
    fn list_keys_test_the_element_type() {
        let enums = [string_enum("Kind", &["paper", "dataset"])];
        // `Vec<i64>`, `Vec<Kind>`, `Vec<Vec<Source>>`, `Vec<DateTime<Utc>>`.
        for key in ["sources", "verified"] {
            for bad in ["i64", "Kind", "Vec", "DateTime"] {
                let found = check(vec![plain(key, FieldType::VecStruct(bad.into()))], &enums);
                assert_eq!(found.warnings.len(), 1, "{key} Vec<{bad}>: {found:?}");
                assert!(found.warnings[0].contains(&format!("found `Vec<{bad}>`")), "{found:?}");
            }
            assert_eq!(
                check(vec![plain(key, FieldType::VecStruct("Source".into()))], &enums),
                KeyDiagnostics::default(),
                "{key}"
            );
        }
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

        for (name, role, why) in [
            ("x", FieldRole::Body, "the body is the markdown after the frontmatter"),
            ("x", has_many, "a has_many view is derived"),
            ("x", FieldRole::Skip, "skipped"),
            ("wikilinks", FieldRole::Plain, "never stores this field in frontmatter"),
            ("source_file", FieldRole::Plain, "never stores this field in frontmatter"),
        ] {
            let field =
                FieldDef { frontmatter_name: Some("other".into()), ..FieldDef::new(name, FieldType::VecString, role) };
            let found = check(vec![field], &[]);
            assert_eq!(found.errors.len(), 1, "{name}: {found:?}");
            assert!(
                found.errors[0].contains(&format!("field `{name}`: frontmatter_name has no effect"))
                    && found.errors[0].contains(why),
                "{found:?}"
            );
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
            list_cap: 10_000,
            okf: OkfOptions::default(),
        };
        let err = crate::gen_markdown_io(&schema, &config).unwrap_err();
        assert!(matches!(&err, crate::CodegenError::Persistence(msg) if msg.contains("`type` is reserved")), "{err}");
        assert!(!out.exists(), "nothing is generated for a schema the vault could not hold");
    }

    #[test]
    fn generation_fails_on_a_bad_actor_or_a_stamped_generated_key_before_writing_anything() {
        let tmp = tempfile::tempdir().unwrap();
        let out = tmp.path().join("generated");
        let generate = |fields: Vec<FieldDef>, generated_by: &str| {
            let schema = crate::ir::SchemaOutput { entities: vec![entity(fields)], enums: vec![] };
            let config = crate::MarkdownIoConfig {
                output_dir: out.clone(),
                vault_root: "data/vault".into(),
                layout: crate::ir::MarkdownLayout::PerEntityDir,
                list_cap: 10_000,
                okf: OkfOptions { index: true, generated_by: Some(generated_by.into()) },
            };
            match crate::gen_markdown_io(&schema, &config) {
                Err(crate::CodegenError::Persistence(msg)) => msg,
                other => panic!("expected a persistence error, got {other:?}"),
            }
        };

        let msg = generate(vec![plain("title", FieldType::String)], "human:me");
        assert!(msg.contains("okf.generated_by = \"human:me\" is not an OKF"), "{msg}");
        let msg = generate(vec![plain("generated", FieldType::Other("Stamp".into()))], "app/1.0");
        assert!(msg.contains("`generated` is reserved while `okf.generated_by` is set"), "{msg}");
        assert!(!out.exists(), "nothing is generated");
    }

    #[test]
    fn an_entity_named_vault_generates_beside_open_vault() {
        let mut vault = entity(vec![]);
        vault.name = "Vault".into();
        let schema = crate::ir::SchemaOutput { entities: vec![vault], enums: vec![] };
        let tmp = tempfile::tempdir().unwrap();
        let out = tmp.path().join("generated");
        let config = crate::MarkdownIoConfig {
            output_dir: out.clone(),
            vault_root: "data/vault".into(),
            layout: crate::ir::MarkdownLayout::PerEntityDir,
            list_cap: 10_000,
            okf: OkfOptions::default(),
        };
        crate::gen_markdown_io(&schema, &config).expect("`Vault` is an ordinary entity name");
        let module = std::fs::read_to_string(out.join("vault.rs")).unwrap();
        assert!(module.contains("pub struct VaultFrontmatter"), "{module}");
        let mod_rs = std::fs::read_to_string(out.join("mod.rs")).unwrap();
        assert!(mod_rs.contains("pub mod vault;") && mod_rs.contains("pub fn open_vault("), "{mod_rs}");
    }

    #[test]
    fn a_generated_key_is_an_error_only_while_the_vault_stamps_it() {
        for field in [
            plain("generated", FieldType::Other("Stamp".into())),
            renamed("provenance", FieldType::String, "generated"),
        ] {
            let name = field.name.clone();
            assert!(check(vec![field.clone()], &[]).errors.is_empty(), "{name}: knob off, the phase-1 rules apply");

            let found = check_frontmatter_keys(&[entity(vec![field])], &[], &stamping());
            assert_eq!(found.errors.len(), 1, "{name}: {found:?}");
            assert!(found.warnings.is_empty(), "{name}: the error replaces the shape warning: {found:?}");
            for part in [
                &format!("entity `Task`, field `{name}`"),
                "frontmatter key `generated` is reserved while `okf.generated_by` is set",
                "#[ontology(frontmatter_name = \"...\")]",
                "or unset `okf.generated_by`",
                OKF_SPEC,
            ] {
                assert!(found.errors[0].contains(part), "missing {part:?} in {}", found.errors[0]);
            }
        }
        let other = check_frontmatter_keys(&[entity(vec![plain("made_by", FieldType::String)])], &[], &stamping());
        assert_eq!(other, KeyDiagnostics::default());
    }

    #[test]
    fn program_actors_pass() {
        for actor in [
            "notes-kb/0.1.0",
            "reference_agent/gemini-2.5-pro",
            "a/b",
            "tool/1.0/beta",
            "process:finance-nightly",
            "process:x",
        ] {
            assert_eq!(check_generated_by(actor), Ok(()), "{actor}");
        }
    }

    #[test]
    fn anything_but_a_program_actor_fails_and_says_why() {
        for (actor, why) in [
            ("", "it is empty"),
            ("human:ahormati", "`human:` actors mark content a person wrote or confirmed"),
            ("Human:someone/1.0", "`human:` actors"),
            ("process:", "`process:` needs an id"),
            ("/1.0", "needs both a producer and a version"),
            ("app/", "needs both a producer and a version"),
            ("my app/1.0", "whitespace or a control character"),
            ("app/1.0\n", "whitespace or a control character"),
            ("app", "neither `<producer>/<version>` nor `process:<id>`"),
            ("agent:x", "neither `<producer>/<version>` nor `process:<id>`"),
        ] {
            let err = check_generated_by(actor).unwrap_err();
            assert!(err.contains(why), "{actor:?}: {err}");
            assert!(
                err.starts_with(&format!(
                    "okf.generated_by = {actor:?} is not an OKF (Open Knowledge Format) 0.2 actor"
                )),
                "{err}"
            );
            assert!(
                err.contains(
                    "(spec §7: https://github.com/GoogleCloudPlatform/knowledge-catalog/blob/main/okf/SPEC.md)"
                ),
                "{err}"
            );
        }
    }
}
