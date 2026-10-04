//! Integration tests for store generation against real schema files.

// The file is already the `tests` module (declared `#[cfg(test)] mod tests;`
// in mod.rs); the inner wrapper is long-standing layout, kept to avoid a
// whole-file reindent. Surfaced when clippy went workspace/all-targets-wide.
#[cfg(test)]
#[allow(clippy::module_inception)]
mod tests {
    use std::path::PathBuf;

    use crate::{StoreConfig, schema::parse::parse_schema_dir, store};

    /// Test that gen_store produces valid code for all real entities.
    /// This reads the embedded fixture schema files and generates store code to a temp dir.
    #[test]
    fn generate_all_real_schemas() {
        let schema_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/schema");

        let entities = parse_schema_dir(&schema_dir).expect("parse_schema_dir failed");
        assert!(!entities.is_empty(), "Expected at least one entity from fixture schemas");

        let tmp = tempfile::tempdir().expect("tempdir");
        let config = StoreConfig {
            output_dir: tmp.path().to_path_buf(),
            hooks_dir: None,
            schema_module_path: "crate::schema".to_string(),
            backend: crate::ir::Backend::Seaorm(None),
            wikilink_policy: None,
            id_strategy: crate::ir::IdStrategy::Provided,
        };

        let result = store::generate(&crate::schema::schema_of(&entities), &config);
        assert!(result.is_ok(), "gen_store failed: {:?}", result.err());

        let output = result.unwrap();

        // Five CRUD methods and count per entity
        assert_eq!(output.methods.len(), entities.len() * 6, "Expected 6 methods per entity");

        // Check that files were written
        let mod_rs = tmp.path().join("mod.rs");
        assert!(mod_rs.exists(), "mod.rs should be generated");

        // Check that each entity has a file
        for entity in &entities {
            let snake = crate::store::helpers::to_snake_case(&entity.name);
            let path = tmp.path().join(format!("{snake}.rs"));
            assert!(path.exists(), "Expected file for entity {}", entity.name);

            let content = std::fs::read_to_string(&path).unwrap();

            // Every generated file should have the CRUD methods
            assert!(content.contains(&"fn list_".to_string()), "Missing list for {}", entity.name);
            assert!(content.contains(&"fn get_".to_string()), "Missing get for {}", entity.name);
            assert!(content.contains(&"fn create_".to_string()), "Missing create for {}", entity.name);
            assert!(content.contains(&"fn update_".to_string()), "Missing update for {}", entity.name);
            assert!(content.contains(&"fn delete_".to_string()), "Missing delete for {}", entity.name);

            // Every entity should have Update struct + From impls
            assert!(
                content.contains(&format!("pub struct {}Update", entity.name)),
                "Missing Update struct for {}",
                entity.name
            );
            assert!(
                content.contains(&format!("From<crate::schema::Create{}Input>", entity.name)),
                "Missing From<CreateInput> for {}",
                entity.name
            );
        }
    }

    /// Test that Tag (simplest entity, no relations) generates code matching the hand-written pattern.
    #[test]
    fn tag_matches_hand_written_pattern() {
        let schema_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/schema");

        let entities = parse_schema_dir(&schema_dir).expect("parse failed");
        let tag = entities.iter().find(|e| e.name == "Tag").expect("Tag entity not found");

        let tmp = tempfile::tempdir().expect("tempdir");
        let config = StoreConfig {
            output_dir: tmp.path().to_path_buf(),
            hooks_dir: None,
            schema_module_path: "crate::schema".to_string(),
            backend: crate::ir::Backend::Seaorm(None),
            wikilink_policy: None,
            id_strategy: crate::ir::IdStrategy::Provided,
        };

        store::generate(&crate::schema::schema_of(std::slice::from_ref(tag)), &config).expect("gen_store failed");

        let content = std::fs::read_to_string(tmp.path().join("tag.rs")).unwrap();

        // Key CRUD method names
        assert!(content.contains("list_tags"), "Missing list_tags");
        assert!(content.contains("get_tag"), "Missing get_tag");
        assert!(content.contains("create_tag"), "Missing create_tag");
        assert!(content.contains("update_tag"), "Missing update_tag");
        assert!(content.contains("delete_tag"), "Missing delete_tag");
        assert!(content.contains("TagUpdate"), "Missing TagUpdate struct");
        assert!(content.contains("emit_change(ChangeOp::Created"), "Missing Created event");
        assert!(content.contains("emit_change(ChangeOp::Updated"), "Missing Updated event");
        assert!(content.contains("emit_change(ChangeOp::Deleted"), "Missing Deleted event");
        assert!(content.contains("TagNotFound"), "Missing error variant");
        // Should NOT have populate_relations (simple entity, no relations)
        assert!(!content.contains("populate_tag_relations"), "Tag should not have populate_relations");

        // Hook calls should be present in generated CRUD
        assert!(content.contains("hooks::before_create("), "Missing before_create hook call");
        assert!(content.contains("hooks::after_create("), "Missing after_create hook call");
        assert!(content.contains("hooks::before_update("), "Missing before_update hook call");
        assert!(content.contains("hooks::after_update("), "Missing after_update hook call");
        assert!(content.contains("hooks::before_delete("), "Missing before_delete hook call");
        assert!(content.contains("hooks::after_delete("), "Missing after_delete hook call");

        // Hook module import
        assert!(content.contains("use crate::store::hooks::tag as hooks;"), "Missing hooks import");
    }

    /// Test that a non-default `schema_module_path` flows into generated store code.
    #[test]
    fn custom_schema_module_path_is_respected() {
        let schema_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/schema");

        let entities = parse_schema_dir(&schema_dir).expect("parse failed");
        let tag = entities.iter().find(|e| e.name == "Tag").expect("Tag entity not found");

        let tmp = tempfile::tempdir().expect("tempdir");
        let hooks = tmp.path().join("hooks");
        let config = StoreConfig {
            output_dir: tmp.path().to_path_buf(),
            hooks_dir: Some(hooks.clone()),
            schema_module_path: "my_crate::domain".to_string(),
            backend: crate::ir::Backend::Seaorm(None),
            wikilink_policy: None,
            id_strategy: crate::ir::IdStrategy::Provided,
        };

        store::generate(&crate::schema::schema_of(std::slice::from_ref(tag)), &config).expect("gen_store failed");

        let content = std::fs::read_to_string(tmp.path().join("tag.rs")).unwrap();
        assert!(content.contains("use my_crate::domain::Tag;"), "Expected custom schema path import, got:\n{content}");
        assert!(
            content.contains("use my_crate::domain::{AppError, ChangeOp, EntityKind};"),
            "Expected custom schema path for AppError/ChangeOp/EntityKind, got:\n{content}"
        );
        assert!(!content.contains("use crate::schema::Tag;"), "Default schema path should not appear");

        // Hook file should also use the custom path
        let hook_content = std::fs::read_to_string(hooks.join("tag.rs")).unwrap();
        assert!(
            hook_content.contains("use my_crate::domain::{AppError, Tag};"),
            "Expected custom schema path in hook file, got:\n{hook_content}"
        );
    }

    /// Test that Workout (entity with junction many_to_many + self belongs_to) generates junction sync code.
    #[test]
    fn workout_has_junction_sync() {
        let schema_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/schema");

        let entities = parse_schema_dir(&schema_dir).expect("parse failed");
        let workout = entities.iter().find(|e| e.name == "Workout").expect("Workout entity not found");

        let tmp = tempfile::tempdir().expect("tempdir");
        let config = StoreConfig {
            output_dir: tmp.path().to_path_buf(),
            hooks_dir: None,
            schema_module_path: "crate::schema".to_string(),
            backend: crate::ir::Backend::Seaorm(None),
            wikilink_policy: None,
            id_strategy: crate::ir::IdStrategy::Provided,
        };

        store::generate(&crate::schema::schema_of(std::slice::from_ref(workout)), &config).expect("gen_store failed");

        let content = std::fs::read_to_string(tmp.path().join("workout.rs")).unwrap();

        assert!(content.contains("populate_workout_relations"), "Missing populate_workout_relations");
        assert!(content.contains("sync_junction"), "Missing sync_junction call");
        assert!(content.contains("workout_tags"), "Missing workout_tags junction table");
        assert!(content.contains("tags_changed"), "Missing conditional junction sync tracking");
    }

    /// `StoreMethodMeta.params` should match the actual generated method signatures.
    #[test]
    fn method_meta_params_match_signatures() {
        let schema_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../src-tauri/src/schema");
        if !schema_dir.exists() {
            return;
        }

        let entities = parse_schema_dir(&schema_dir).expect("parse failed");
        let role = entities.iter().find(|e| e.name == "Role").expect("Role entity not found");

        let tmp = tempfile::tempdir().expect("tempdir");
        let config = StoreConfig {
            output_dir: tmp.path().to_path_buf(),
            hooks_dir: None,
            schema_module_path: "crate::schema".to_string(),
            backend: crate::ir::Backend::Seaorm(None),
            wikilink_policy: None,
            id_strategy: crate::ir::IdStrategy::Provided,
        };

        let output =
            store::generate(&crate::schema::schema_of(std::slice::from_ref(role)), &config).expect("gen_store failed");

        let by_name =
            |n: &str| output.methods.iter().find(|m| m.name == n).unwrap_or_else(|| panic!("missing method {n}"));

        let list = by_name("list_roles");
        assert_eq!(list.params.len(), 3);
        assert_eq!(list.params[0].name, "order");
        assert_eq!(list.params[0].param_type, "&[OrderBy<RoleSortField>]");
        assert_eq!(list.params[1].name, "limit");
        assert_eq!(list.params[1].param_type, "Option<u64>");
        assert_eq!(list.params[2].name, "offset");

        let count = by_name("count_roles");
        assert_eq!(count.kind, crate::ir::StoreMethodKind::Crud(crate::ir::CrudOp::Count));
        assert!(count.params.is_empty());
        assert_eq!(count.return_type, "u64");

        let get = by_name("get_role");
        assert_eq!(get.params.len(), 1);
        assert_eq!(get.params[0].name, "id");
        assert_eq!(get.params[0].param_type, "&str");

        let create = by_name("create_role");
        assert_eq!(create.params.len(), 1);
        assert_eq!(create.params[0].name, "role");
        assert_eq!(create.params[0].param_type, "Role");

        let update = by_name("update_role");
        assert_eq!(update.params.len(), 2);
        assert_eq!(update.params[0].name, "id");
        assert_eq!(update.params[1].name, "updates");
        assert_eq!(update.params[1].param_type, "RoleUpdate");

        let delete = by_name("delete_role");
        assert_eq!(delete.params.len(), 1);
        assert_eq!(delete.params[0].name, "id");
    }

    fn markdown_backend() -> crate::ir::Backend {
        use crate::ir::{Backend, MarkdownIoOutput};
        Backend::Markdown(MarkdownIoOutput {
            module_path: "crate::persistence::markdown::generated".into(),
            entities: Vec::new(),
        })
    }

    /// The markdown backend generates a store module shaped like the SeaORM
    /// one: same method names, hook call sites, and emit_change points —
    /// only the persistence primitives differ.
    #[test]
    fn markdown_backend_generates_crud_with_lifecycle_parity() {
        let schema_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/schema");
        let entities = parse_schema_dir(&schema_dir).expect("parse failed");
        let tag = entities.iter().find(|e| e.name == "Tag").expect("Tag entity not found");

        let tmp = tempfile::tempdir().expect("tempdir");
        let config = StoreConfig {
            output_dir: tmp.path().to_path_buf(),
            hooks_dir: None,
            schema_module_path: "crate::schema".to_string(),
            backend: markdown_backend(),
            wikilink_policy: None,
            id_strategy: crate::ir::IdStrategy::SlugFromField("name".into()),
        };

        store::generate(&crate::schema::schema_of(std::slice::from_ref(tag)), &config)
            .expect("gen_store(markdown) failed");
        let content = std::fs::read_to_string(tmp.path().join("tag.rs")).unwrap();

        // Lifecycle parity with the SeaORM emission.
        for needle in [
            "list_tags",
            "get_tag",
            "create_tag",
            "update_tag",
            "delete_tag",
            "hooks::before_create(",
            "hooks::after_create(",
            "hooks::before_update(",
            "hooks::after_update(",
            "hooks::before_delete(",
            "hooks::after_delete(",
            "emit_change(ChangeOp::Created",
            "emit_change(ChangeOp::Updated",
            "emit_change(ChangeOp::Deleted",
            "TagNotFound",
        ] {
            assert!(content.contains(needle), "missing {needle}:\n{content}");
        }
        // Markdown primitives, not SeaORM ones.
        assert!(content.contains("self.vault()"), "markdown store talks to the vault: {content}");
        assert!(
            content.contains(".entity(TAGS_DIR, TAG_TYPE).create(\n            &markdown_store::IdStrategy::SlugFromField(\"name\".into()),"),
            "create derives ids through the typed entity view, by the configured strategy: {content}"
        );
        for needle in [
            "Err(markdown_store::Error::IdRequired { reason }) => return Err(AppError::TagIdRequired(reason)),",
            "Err(markdown_store::Error::AlreadyExists { .. }) => return Err(AppError::TagAlreadyExists(tag.id)),",
            "Ok(None) | Err(markdown_store::Error::InvalidId { .. }) => {",
            "Err(markdown_store::Error::NotFound { .. } | markdown_store::Error::InvalidId { .. }) => {",
        ] {
            assert!(content.contains(needle), "missing {needle}:\n{content}");
        }
        assert!(content.contains("const TAG_TYPE: &str = \"Tag\";"), "the store declares the OKF type: {content}");
        assert!(!content.contains("sea_orm"), "no SeaORM in a markdown module: {content}");
        assert!(!content.contains("self.db()"), "no db() in a markdown module: {content}");
    }

    /// The OKF type comes from the markdown IR, the same place as the
    /// directory segment, so the two can't disagree about an entity.
    #[test]
    fn markdown_store_takes_the_okf_type_from_the_ir() {
        let schema_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/schema");
        let entities = parse_schema_dir(&schema_dir).expect("parse failed");
        let tag = entities.iter().find(|e| e.name == "Tag").expect("Tag entity not found");

        let mut backend = markdown_backend();
        if let crate::ir::Backend::Markdown(md) = &mut backend {
            md.entities.push(crate::ir::MarkdownEntityMeta {
                entity_name: "Tag".into(),
                type_name: "Label".into(),
                dir_segment: "labels".into(),
                body_field: None,
                authoritative_m2m: Vec::new(),
            });
        }
        let tmp = tempfile::tempdir().expect("tempdir");
        let config = StoreConfig {
            output_dir: tmp.path().to_path_buf(),
            hooks_dir: None,
            schema_module_path: "crate::schema".to_string(),
            backend,
            wikilink_policy: None,
            id_strategy: crate::ir::IdStrategy::Provided,
        };
        store::generate(&crate::schema::schema_of(std::slice::from_ref(tag)), &config)
            .expect("gen_store(markdown) failed");
        let content = std::fs::read_to_string(tmp.path().join("tag.rs")).unwrap();

        assert!(content.contains("const TAG_TYPE: &str = \"Label\";"), "{content}");
        assert!(content.contains("const TAGS_DIR: &str = \"labels\";"), "{content}");
        assert!(!content.contains("TAG_TYPE, TagFrontmatter"), "the frontmatter module has no type const: {content}");
    }

    /// Every store method reads the record's id, and the id ends every
    /// order, so an entity without one fails before anything is written.
    #[test]
    fn an_entity_without_an_id_field_fails_on_both_backends() {
        let schema_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/schema");
        let entities = parse_schema_dir(&schema_dir).expect("parse failed");
        let mut tag = entities.iter().find(|e| e.name == "Tag").expect("Tag entity not found").clone();
        tag.fields.retain(|f| f.role != crate::schema::model::FieldRole::Id);

        for backend in [crate::ir::Backend::Seaorm(None), markdown_backend()] {
            let tmp = tempfile::tempdir().expect("tempdir");
            let out_dir = tmp.path().join("generated");
            let config = StoreConfig {
                output_dir: out_dir.clone(),
                hooks_dir: None,
                schema_module_path: "crate::schema".to_string(),
                backend: backend.clone(),
                wikilink_policy: None,
                id_strategy: crate::ir::IdStrategy::Provided,
            };
            let err = store::generate(&crate::schema::schema_of(std::slice::from_ref(&tag)), &config)
                .expect_err("an entity needs an id");
            assert!(format!("{err}").contains("entity `Tag` has no `#[ontology(id)]` field"), "{backend:?}: {err}");
            assert!(!out_dir.exists(), "validation failures must not write files");
        }
    }

    /// SlugFromField must name a String field on every entity — validated at
    /// generation time, before any file is written, on both backends.
    #[test]
    fn slug_strategy_validates_source_field_on_both_backends() {
        let schema_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/schema");
        let entities = parse_schema_dir(&schema_dir).expect("parse failed");
        let entity = |name: &str| entities.iter().find(|e| e.name == name).expect("fixture entity").clone();
        let cases = [
            (entity("Tag"), "no_such_field", "has no field `no_such_field`"),
            (entity("Workout"), "name", "must be a plain String"),
        ];

        for backend in [crate::ir::Backend::Seaorm(None), markdown_backend()] {
            for (target, field, needle) in &cases {
                let tmp = tempfile::tempdir().expect("tempdir");
                let out_dir = tmp.path().join("generated");
                let config = StoreConfig {
                    output_dir: out_dir.clone(),
                    hooks_dir: None,
                    schema_module_path: "crate::schema".to_string(),
                    backend: backend.clone(),
                    wikilink_policy: None,
                    id_strategy: crate::ir::IdStrategy::SlugFromField((*field).into()),
                };

                let err = store::generate(&crate::schema::schema_of(std::slice::from_ref(target)), &config)
                    .expect_err("a bad slug field must fail");
                let msg = format!("{err}");
                assert!(msg.contains(*needle), "{backend:?}: the error says what is wrong: {msg}");
                assert!(
                    msg.contains(&format!("the store default IdStrategy::SlugFromField({field:?})"))
                        && msg.contains(&format!("give `{}` its own `#[ontology(entity, id = ", target.name))
                        && msg.contains("or choose a store default that fits every entity without one"),
                    "{backend:?}: the error blames the default and says how to fix it: {msg}"
                );
                assert!(!out_dir.exists(), "validation failures must not write files");
            }
        }
    }

    /// An entity's own `id = "..."` beats the store default on both
    /// backends; entities without one keep the default. Workout has no plain
    /// `String` `name`, so the default alone would be refused for it.
    #[test]
    fn entity_id_override_beats_the_store_default_on_both_backends() {
        use crate::ir::IdStrategy;
        let schema_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/schema");
        let entities = parse_schema_dir(&schema_dir).expect("parse failed");
        let entity = |name: &str| entities.iter().find(|e| e.name == name).expect("fixture entity").clone();
        let tag = entity("Tag");
        let mut workout = entity("Workout");
        workout.id_strategy = Some(IdStrategy::Uuid);
        let mut exercise = entity("Exercise");
        exercise.id_strategy = Some(IdStrategy::Provided);

        for backend in [crate::ir::Backend::Seaorm(None), markdown_backend()] {
            let tmp = tempfile::tempdir().expect("tempdir");
            let config = StoreConfig {
                output_dir: tmp.path().to_path_buf(),
                hooks_dir: None,
                schema_module_path: "crate::schema".to_string(),
                backend: backend.clone(),
                wikilink_policy: None,
                id_strategy: IdStrategy::SlugFromField("name".into()),
            };
            store::generate(&crate::schema::schema_of(&[tag.clone(), workout.clone(), exercise.clone()]), &config)
                .unwrap_or_else(|e| panic!("{backend:?}: {e}"));
            let read = |file: &str| std::fs::read_to_string(tmp.path().join(file)).unwrap();
            let (tag_code, workout_code, exercise_code) = (read("tag.rs"), read("workout.rs"), read("exercise.rs"));

            let (default, uuid, provided) = match backend {
                crate::ir::Backend::Seaorm(_) => (
                    "ontogen_core::id::slugify(&tag.name)",
                    "ontogen_core::id::new_uuid()",
                    "AppError::ExerciseIdRequired(\"this store requires the caller to supply an id\"",
                ),
                crate::ir::Backend::Markdown(_) => (
                    "&markdown_store::IdStrategy::SlugFromField(\"name\".into()),",
                    "&markdown_store::IdStrategy::Uuid,",
                    "&markdown_store::IdStrategy::Provided,",
                ),
            };
            assert!(tag_code.contains(default), "{backend:?}: Tag keeps the default:\n{tag_code}");
            assert!(workout_code.contains(uuid), "{backend:?}: Workout's override wins:\n{workout_code}");
            assert!(exercise_code.contains(provided), "{backend:?}: Exercise's override wins:\n{exercise_code}");
            for code in [&workout_code, &exercise_code] {
                assert!(!code.contains("slugify") && !code.contains("SlugFromField"), "{backend:?}:\n{code}");
            }
        }
    }

    /// An override is validated for its own entity, with an error that names
    /// the attribute; the default is not checked against that entity.
    #[test]
    fn entity_slug_override_is_validated_per_entity() {
        use crate::ir::IdStrategy;
        let schema_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/schema");
        let entities = parse_schema_dir(&schema_dir).expect("parse failed");
        let mut workout = entities.iter().find(|e| e.name == "Workout").expect("Workout").clone();
        workout.id_strategy = Some(IdStrategy::SlugFromField("name".into()));

        for backend in [crate::ir::Backend::Seaorm(None), markdown_backend()] {
            let tmp = tempfile::tempdir().expect("tempdir");
            let out_dir = tmp.path().join("generated");
            let config = StoreConfig {
                output_dir: out_dir.clone(),
                hooks_dir: None,
                schema_module_path: "crate::schema".to_string(),
                backend: backend.clone(),
                wikilink_policy: None,
                id_strategy: IdStrategy::Provided,
            };
            let err = store::generate(&crate::schema::schema_of(std::slice::from_ref(&workout)), &config)
                .expect_err("Workout.name is optional");
            let msg = format!("{err}");
            assert!(
                msg.contains("`#[ontology(entity, id = \"slug(name)\")]` on entity `Workout`")
                    && msg.contains("must be a plain String")
                    && msg.contains(
                        "name a plain String field of `Workout` in `slug(...)`, or use `id = \"provided\"` or `id = \"uuid\"`"
                    ),
                "{backend:?}: the error blames the override and says how to fix it: {msg}"
            );
            assert!(!msg.contains("store default"), "{backend:?}: the default is not involved: {msg}");
            assert!(!out_dir.exists(), "validation failures must not write files");
        }
    }

    /// `wikilink_policy: Some(Strip)` overrides the SQL backend's default
    /// Passthrough: the DTO `From` impls strip `[[id]]` on relation fields
    /// even though the CRUD bodies stay SeaORM. This is the hybrid contract
    /// of a SQL-backed store whose wire inputs come from markdown-authoring
    /// agents.
    #[test]
    fn seaorm_backend_honors_wikilink_strip_override() {
        let schema_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/schema");
        let entities = parse_schema_dir(&schema_dir).expect("parse failed");
        // Workout carries belongs_to + many_to_many relation fields.
        let workout = entities.iter().find(|e| e.name == "Workout").expect("Workout entity not found");

        let tmp = tempfile::tempdir().expect("tempdir");
        let config = StoreConfig {
            output_dir: tmp.path().to_path_buf(),
            hooks_dir: None,
            schema_module_path: "crate::schema".to_string(),
            backend: crate::ir::Backend::Seaorm(None),
            wikilink_policy: Some(crate::ir::WikilinkPolicy::Strip),
            id_strategy: crate::ir::IdStrategy::Provided,
        };

        store::generate(&crate::schema::schema_of(std::slice::from_ref(workout)), &config).expect("gen_store failed");
        let content = std::fs::read_to_string(tmp.path().join("workout.rs")).unwrap();

        assert!(
            content.contains("markdown_store::wikilink::strip"),
            "Strip override must emit wikilink stripping in From impls: {content}"
        );
        assert!(content.contains("sea_orm"), "CRUD bodies stay SeaORM under the override: {content}");
    }

    /// A `has_many` whose target is another entity fails generation on both
    /// backends, before any file is written.
    #[test]
    fn cross_entity_has_many_fails_on_both_backends() {
        use crate::schema::model::{FieldDef, FieldRole, FieldType, RelationInfo, RelationKind};

        let schema_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/schema");
        let entities = parse_schema_dir(&schema_dir).expect("parse failed");
        let mut workout = entities.iter().find(|e| e.name == "Workout").expect("Workout entity not found").clone();
        workout.fields.push(FieldDef::new(
            "sets",
            FieldType::VecString,
            FieldRole::Relation(RelationInfo {
                kind: RelationKind::HasMany,
                target: "WorkoutSet".to_string(),
                junction: None,
                foreign_key: Some("workout_id".to_string()),
            }),
        ));

        for backend in [crate::ir::Backend::Seaorm(None), markdown_backend()] {
            let tmp = tempfile::tempdir().expect("tempdir");
            let out_dir = tmp.path().join("generated");
            let config = StoreConfig {
                output_dir: out_dir.clone(),
                hooks_dir: None,
                schema_module_path: "crate::schema".to_string(),
                backend: backend.clone(),
                wikilink_policy: None,
                id_strategy: crate::ir::IdStrategy::Provided,
            };

            let err = store::generate(&crate::schema::schema_of(std::slice::from_ref(&workout)), &config)
                .expect_err("a cross-entity has_many must fail");
            let msg = format!("{err}");
            assert!(msg.contains("`Workout.sets`: has_many target `WorkoutSet`"), "{backend:?}: {msg}");
            assert!(!out_dir.exists(), "validation failures must not write files");
        }
    }
}
