//! Snapshot tests for representative generator outputs.
//!
//! These tests freeze the exact string output of a handful of generators so
//! future changes surface as reviewable diffs in the `.snap` files under
//! `src/snapshots/`. This is intentionally narrow coverage - simple + complex
//! cases per generator - not a comprehensive sweep.
//!
//! ## Updating snapshots
//!
//! When a generator change is intentional, accept the new snapshot with either:
//!
//! ```sh
//! INSTA_UPDATE=always cargo test --lib snapshots
//! # or, for interactive review:
//! cargo insta review
//! ```
//!
//! ## How the tests are structured
//!
//! - `gen_entity` (SeaORM) is snapshotted by calling `generate_entity_code`
//!   directly - it's pub and returns a `String`.
//! - `gen_dtos` and `gen_store` are snapshotted by calling their public
//!   `generate()` entrypoints against a tempdir and reading the resulting
//!   file back. This is deliberate: the public API is what consumers see, so
//!   the snapshot includes rustfmt output exactly as it lands on disk.

use std::collections::HashMap;
use std::path::Path;

use crate::ir::SchemaOutput;
use crate::persistence::seaorm::gen_entity::{generate_entity_code, generate_junction_code, to_snake_case};
use crate::schema::model::{
    EntityDef, EnumDef, EnumVariant, FieldDef, FieldRole, FieldType, RelationInfo, RelationKind,
};
use crate::{DocsConfig, DtoConfig, StoreConfig};

// ─── Fixture builders ────────────────────────────────────────────────────────

/// Simple entity: `Role { id, name, body }` - no relations.
fn simple_role_entity() -> EntityDef {
    EntityDef {
        name: "Role".to_string(),
        directory: "roles".to_string(),
        table: "roles".to_string(),
        type_name: "role".to_string(),
        prefix: "role".to_string(),
        fields: vec![
            FieldDef::new("id", FieldType::String, FieldRole::Id),
            FieldDef::new("name", FieldType::String, FieldRole::Plain),
            FieldDef::new("body", FieldType::String, FieldRole::Body),
        ],
        doc: String::new(),
    }
}

/// Entity with a single `belongs_to` relation:
/// `Comment { id, post_id -> Post, body }`.
fn comment_belongs_to_post_entity() -> EntityDef {
    EntityDef {
        name: "Comment".to_string(),
        directory: "comments".to_string(),
        table: "comments".to_string(),
        type_name: "comment".to_string(),
        prefix: "comment".to_string(),
        fields: vec![
            FieldDef::new("id", FieldType::String, FieldRole::Id),
            FieldDef::new(
                "post_id",
                FieldType::OptionString,
                FieldRole::Relation(RelationInfo {
                    kind: RelationKind::BelongsTo,
                    target: "Post".to_string(),
                    junction: None,
                    foreign_key: None,
                }),
            ),
            FieldDef::new("body", FieldType::String, FieldRole::Body),
        ],
        doc: String::new(),
    }
}

/// Entity with a `many_to_many` relation:
/// `Article { id, title, tags -> Tag (via article_tags), body }`.
fn article_mtm_tags_entity() -> EntityDef {
    EntityDef {
        name: "Article".to_string(),
        directory: "articles".to_string(),
        table: "articles".to_string(),
        type_name: "article".to_string(),
        prefix: "article".to_string(),
        fields: vec![
            FieldDef::new("id", FieldType::String, FieldRole::Id),
            FieldDef::new("title", FieldType::String, FieldRole::Plain),
            FieldDef::new(
                "tags",
                FieldType::VecString,
                FieldRole::Relation(RelationInfo {
                    kind: RelationKind::ManyToMany,
                    target: "Tag".to_string(),
                    junction: Some("article_tags".to_string()),
                    foreign_key: None,
                }),
            ),
            FieldDef::new("body", FieldType::String, FieldRole::Body),
        ],
        doc: String::new(),
    }
}

/// Build a `{name -> snake_case}` module map containing every entity name
/// referenced by a fixture - used by `generate_entity_code`.
fn modules_map(names: &[&str]) -> HashMap<String, String> {
    names.iter().map(|n| (n.to_string(), to_snake_case(n))).collect()
}

// ─── Helpers for file-producing generators ───────────────────────────────────

/// Call `persistence::dto::generate` into a tempdir and return the generated
/// file for `entity`.
fn generate_dto_file(entity: &EntityDef) -> String {
    let tmp = tempfile::tempdir().expect("tempdir");
    let config = DtoConfig { output_dir: tmp.path().to_path_buf() };
    crate::gen_dtos(std::slice::from_ref(entity), &config).expect("gen_dtos failed");

    let snake = to_snake_case(&entity.name);
    read_file(&tmp.path().join(format!("{snake}.rs")))
}

/// Call `store::generate` into a tempdir and return the generated file for `entity`.
fn generate_store_file(entity: &EntityDef) -> String {
    let tmp = tempfile::tempdir().expect("tempdir");
    let config = StoreConfig {
        output_dir: tmp.path().to_path_buf(),
        hooks_dir: None,
        schema_module_path: "crate::schema".to_string(),
        backend: crate::ir::Backend::Seaorm(None),
        wikilink_policy: None,
    };
    crate::gen_store(std::slice::from_ref(entity), &config).expect("gen_store failed");

    let snake = to_snake_case(&entity.name);
    read_file(&tmp.path().join(format!("{snake}.rs")))
}

fn read_file(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

// ─── Snapshots ───────────────────────────────────────────────────────────────

#[test]
fn seaorm_entity_simple() {
    let entity = simple_role_entity();
    let mods = modules_map(&["Role"]);
    let code = generate_entity_code(&entity, &mods);
    insta::assert_snapshot!(code);
}

#[test]
fn seaorm_entity_with_belongs_to() {
    let entity = comment_belongs_to_post_entity();
    let mods = modules_map(&["Comment", "Post"]);
    let code = generate_entity_code(&entity, &mods);
    insta::assert_snapshot!(code);
}

#[test]
fn seaorm_entity_with_many_to_many_and_junction() {
    let entity = article_mtm_tags_entity();
    let mods = modules_map(&["Article", "Tag"]);
    let entity_code = generate_entity_code(&entity, &mods);

    // Also snapshot the junction table code for the same relation.
    let tags_field = entity.fields.iter().find(|f| f.name == "tags").expect("tags field");
    let info = match &tags_field.role {
        FieldRole::Relation(info) => info,
        _ => panic!("tags should be a relation"),
    };
    let (junction_name, junction_code) = generate_junction_code(&entity, tags_field, info, &mods);

    let combined =
        format!("// === Article entity ===\n{entity_code}\n// === Junction: {junction_name} ===\n{junction_code}");
    insta::assert_snapshot!(combined);
}

#[test]
fn dto_simple_entity() {
    let code = generate_dto_file(&simple_role_entity());
    insta::assert_snapshot!(code);
}

#[test]
fn store_crud_complex_entity() {
    // Article has a many_to_many relation → exercises the relation-heavy
    // branches in gen_crud (populate_relations, sync_junction, etc.).
    let code = generate_store_file(&article_mtm_tags_entity());
    insta::assert_snapshot!(code);
}

/// Self-referential `has_many` entity (the in-tree set_parent shape):
/// `Node { id, label, parent_id -> Node, contains: has_many(parent_id), body }`.
fn node_has_many_entity() -> EntityDef {
    EntityDef {
        name: "Node".to_string(),
        directory: "nodes".to_string(),
        table: "nodes".to_string(),
        type_name: "node".to_string(),
        prefix: "node".to_string(),
        fields: vec![
            FieldDef::new("id", FieldType::String, FieldRole::Id),
            FieldDef::new("label", FieldType::String, FieldRole::Plain),
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
            FieldDef::new("body", FieldType::String, FieldRole::Body),
        ],
        doc: String::new(),
    }
}

/// Generate a MARKDOWN-backed store file for `entity` and read it back.
fn generate_markdown_store_file(entity: &EntityDef) -> String {
    use crate::ir::{Backend, IdStrategy, MarkdownEntityMeta, MarkdownIoOutput, MarkdownLayout};

    let tmp = tempfile::tempdir().expect("tempdir");
    let config = StoreConfig {
        output_dir: tmp.path().to_path_buf(),
        hooks_dir: None,
        schema_module_path: "crate::schema".to_string(),
        backend: Backend::Markdown(MarkdownIoOutput {
            vault_root: "data/vault".into(),
            layout: MarkdownLayout::PerEntityDir,
            id_strategy: IdStrategy::Provided,
            list_cap: 10_000,
            module_path: "crate::persistence::markdown::generated".into(),
            entities: vec![MarkdownEntityMeta {
                entity_name: entity.name.clone(),
                type_name: entity.type_name.clone(),
                dir_segment: entity.directory.clone(),
                body_field: entity.body_field().map(|f| f.name.clone()),
                authoritative_m2m: entity.junction_relations().map(|(f, _)| f.name.clone()).collect(),
            }],
        }),
        wikilink_policy: None,
    };
    crate::gen_store(std::slice::from_ref(entity), &config).expect("gen_store(markdown) failed");

    let snake = to_snake_case(&entity.name);
    read_file(&tmp.path().join(format!("{snake}.rs")))
}

#[test]
fn markdown_store_complex_entity() {
    // The m2m twin of store_crud_complex_entity: the authoritative wikilink
    // list persists with the record itself — no junction sync step exists.
    let code = generate_markdown_store_file(&article_mtm_tags_entity());
    insta::assert_snapshot!(code);
}

#[test]
fn markdown_store_has_many_entity() {
    // Exercises the derived-view paths: reverse-walk populate and the
    // read-mutate-rewrite set_parent helper (SeaORM's raw-SQL fast path
    // replacement).
    let code = generate_markdown_store_file(&node_has_many_entity());
    insta::assert_snapshot!(code);
}

#[test]
fn markdown_frontmatter_complex_entity() {
    // Article's m2m relation exercises the wikilink-encode/strip boundary
    // and the owned-keys constant; body field exercises the conversion
    // signature.
    let code = crate::persistence::markdown::gen_frontmatter::generate_frontmatter_module(&article_mtm_tags_entity());
    insta::assert_snapshot!(code);
}

// ─── Servers: two API surfaces ───────────────────────────────────────────────

/// Run `generate_transport` over the two-surface fixture with `generator`
/// pointed at a tempdir file, and read the generated file back.
fn generate_two_surface_file(
    generator: impl FnOnce(std::path::PathBuf) -> crate::servers::ServerGenerator,
    pagination: Option<crate::servers::PaginationConfig>,
) -> String {
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut surfaces = crate::servers::tests::two_surface_fixture(tmp.path());
    surfaces[1].pagination = pagination;
    surfaces[1].paginated_modules = vec!["exercise".to_string()];
    let mut config = crate::servers::tests::two_surface_config(surfaces);
    let output = tmp.path().join("generated.rs");
    config.generators = vec![generator(output.clone())];
    crate::servers::generate_transport(&config).expect("generate_transport failed");
    read_file(&output)
}

#[test]
fn servers_two_surfaces_http() {
    // `workout` merges custom fns from the primary surface with the CRUD five
    // from the fitness surface; the fitness handlers open `fitness_store()`,
    // call through the `workout_1` alias, and qualify the shared `Workout`.
    let code = generate_two_surface_file(
        |output| crate::servers::ServerGenerator::HttpAxum { output },
        Some(crate::servers::PaginationConfig { default_limit: 20, max_limit: 100 }),
    );
    insta::assert_snapshot!(code);
}

#[test]
fn servers_two_surfaces_ipc() {
    let code = generate_two_surface_file(|output| crate::servers::ServerGenerator::TauriIpc { output }, None);
    insta::assert_snapshot!(code);
}

// ─── Docs: the data-model reference and JSON Schema ──────────────────────────

/// Every file the docs stage wrote, read back from the tempdir.
struct DocsFiles {
    markdown: String,
    schemas: HashMap<String, String>,
    export: String,
}

/// Run the docs stage over `schema` into a tempdir and read its output back.
fn generate_docs(schema: &SchemaOutput) -> DocsFiles {
    let tmp = tempfile::tempdir().expect("tempdir");
    let schema_dir = tmp.path().join("schema");
    let config = DocsConfig {
        markdown_output: tmp.path().join("data-model.md"),
        json_schema_dir: schema_dir.clone(),
        title: "Blog data model".to_string(),
        export_format: "blog-log".to_string(),
    };
    crate::gen_docs(schema, &config).expect("gen_docs failed");

    let schemas = schema
        .entities
        .iter()
        .map(|entity| {
            let snake = to_snake_case(&entity.name);
            let content = read_file(&schema_dir.join(format!("{snake}.schema.json")));
            (snake, content)
        })
        .collect();

    DocsFiles {
        markdown: read_file(&config.markdown_output),
        schemas,
        export: read_file(&schema_dir.join("export.schema.json")),
    }
}

fn schema_output(entities: Vec<EntityDef>, enums: Vec<EnumDef>) -> SchemaOutput {
    SchemaOutput { entities, enums }
}

/// A documented entity that exercises the whole type mapping: an id, an
/// optional integer, an enum, a list, and a float.
fn documented_set_entity() -> EntityDef {
    let documented = |doc: &str, field: FieldDef| FieldDef { doc: doc.to_string(), ..field };
    EntityDef {
        name: "Set".to_string(),
        doc: "One working set of an exercise.".to_string(),
        directory: "sets".to_string(),
        table: "sets".to_string(),
        type_name: "set".to_string(),
        prefix: "set".to_string(),
        fields: vec![
            documented("Stable id.", FieldDef::new("id", FieldType::String, FieldRole::Id)),
            documented("Integer metres.", FieldDef::new("distance_m", FieldType::OptionI32, FieldRole::Plain)),
            documented(
                "How the set was\nrecorded.",
                FieldDef::new("kind", FieldType::OptionEnum("SetKind".to_string()), FieldRole::EnumField),
            ),
            FieldDef::new("tags", FieldType::VecString, FieldRole::Plain),
            FieldDef::new("weight_kg", FieldType::F64, FieldRole::Plain),
        ],
    }
}

fn set_kind_enum() -> EnumDef {
    EnumDef {
        name: "SetKind".to_string(),
        variants: vec![
            EnumVariant { name: "Working".to_string(), value: "working".to_string() },
            EnumVariant { name: "WarmUp".to_string(), value: "warm-up".to_string() },
        ],
    }
}

#[test]
fn docs_data_model_reference() {
    // The belongs_to fixture: one entity section, a relations block, and the
    // erDiagram edge to the target it points at.
    let files = generate_docs(&schema_output(vec![comment_belongs_to_post_entity()], vec![]));
    insta::assert_snapshot!(files.markdown);
}

#[test]
fn docs_entity_json_schema() {
    let files = generate_docs(&schema_output(vec![comment_belongs_to_post_entity()], vec![]));
    insta::assert_snapshot!(files.schemas["comment"]);
}

#[test]
fn docs_json_schema_required_matches_non_optional_fields() {
    let files = generate_docs(&schema_output(vec![documented_set_entity()], vec![set_kind_enum()]));

    let schema: serde_json::Value = serde_json::from_str(&files.schemas["set"]).expect("entity schema parses");
    assert_eq!(schema["required"], serde_json::json!(["id", "tags", "weight_kg"]));
    assert_eq!(schema["description"], "One working set of an exercise.");
    assert_eq!(schema["properties"]["distance_m"]["type"], "integer");
    assert_eq!(schema["properties"]["distance_m"]["description"], "Integer metres.");
    assert_eq!(schema["properties"]["kind"]["enum"], serde_json::json!(["working", "warm-up"]));
    assert_eq!(schema["properties"]["tags"]["items"]["type"], "string");

    let export: serde_json::Value = serde_json::from_str(&files.export).expect("export schema parses");
    assert_eq!(export["properties"]["tables"]["properties"]["sets"]["items"]["$ref"], "set.schema.json");
    assert_eq!(export["properties"]["manifest"]["properties"]["format"]["const"], "blog-log");
    assert_eq!(export["x-schema-revision"], schema["x-schema-revision"]);
}

#[test]
fn docs_markdown_prints_enum_values_and_flattens_docs() {
    let files = generate_docs(&schema_output(vec![documented_set_entity()], vec![set_kind_enum()]));

    assert!(files.markdown.contains("| distance_m | integer | no | Integer metres. |"), "{}", files.markdown);
    assert!(files.markdown.contains("enum(working, warm-up)"), "{}", files.markdown);
    // A multi-line field doc collapses onto the table row.
    assert!(files.markdown.contains("How the set was recorded."), "{}", files.markdown);
}
