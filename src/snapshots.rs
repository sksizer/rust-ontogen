//! Snapshot tests for representative generator outputs.
//!
//! These tests freeze the exact string output of a handful of generators so
//! future changes surface as reviewable diffs in the `.snap` files under
//! `src/snapshots/`. This is intentionally narrow coverage — simple + complex
//! cases per generator — not a comprehensive sweep.
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
//!   directly — it's pub and returns a `String`.
//! - `gen_dtos` and `gen_store` are snapshotted by calling their public
//!   `generate()` entrypoints against a tempdir and reading the resulting
//!   file back. This is deliberate: the public API is what consumers see, so
//!   the snapshot includes rustfmt output exactly as it lands on disk.

use std::collections::HashMap;
use std::path::Path;

use crate::persistence::seaorm::gen_entity::{generate_entity_code, generate_junction_code, to_snake_case};
use crate::schema::model::{EntityDef, FieldDef, FieldRole, FieldType, RelationInfo, RelationKind};
use crate::{DtoConfig, StoreConfig};

// ─── Fixture builders ────────────────────────────────────────────────────────

/// Simple entity: `Role { id, name, body }` — no relations.
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
    }
}

/// Build a `{name -> snake_case}` module map containing every entity name
/// referenced by a fixture — used by `generate_entity_code`.
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
    let config = StoreConfig { output_dir: tmp.path().to_path_buf(), hooks_dir: None };
    crate::gen_store(std::slice::from_ref(entity), None, &config).expect("gen_store failed");

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
