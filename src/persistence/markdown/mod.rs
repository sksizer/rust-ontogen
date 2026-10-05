//! Markdown filesystem persistence generator.
//!
//! Emits one `{Entity}Frontmatter` module per entity — the typed boundary
//! between schema entities and on-disk YAML frontmatter, built on the
//! `markdown-store` runtime crate. This replaced the former emit-everything
//! model (hand-rolled YAML writers, a `serde_yaml_ng` parser dispatcher and
//! `fs_ops` helpers bound to module paths nothing generated): file I/O,
//! atomic writes, walking, and the lossless `Document` round-trip are the
//! runtime crate's job now; the generated code is a thin typed shim. The
//! old vault-scan dispatcher (`OntologyElement`/`ParseResult`) had no
//! generic consumer and was retired with it — bulk scanning returns as a
//! follow-up on top of `VaultHandle` if a consumer earns it.
//!
//! Their `mod.rs` also carries `open_vault`, which builds that
//! `VaultHandle` from the build-time configuration, and `VAULT_ROOT`.

pub mod gen_frontmatter;
pub mod gen_vault;
pub(crate) mod okf;

use std::collections::HashSet;

use crate::ir::{MarkdownEntityMeta, MarkdownIoOutput, MarkdownLayout, SchemaOutput};
use crate::store::helpers::to_snake_case;
use crate::{CodegenError, MarkdownIoConfig};

/// Generate the markdown I/O code: per-entity `{Entity}Frontmatter` modules
/// and a `mod.rs` that declares them and holds `VAULT_ROOT` and
/// `open_vault`, which builds the runtime `VaultHandle` from `config`.
///
/// Checks run first and fail generation before anything is written: under
/// [`MarkdownLayout::PerEntityDir`], every entity directory against the
/// Windows device names (see `check_directories`); the
/// `okf.generated_by` actor (see `okf::check_generated_by`); and every
/// frontmatter key against the keys OKF reserves (see
/// `okf::check_frontmatter_keys`). Each key warning is printed as a
/// `cargo:warning=` line.
///
/// Returns the [`MarkdownIoOutput`] metadata `gen_store` consumes when the
/// store backend is [`crate::ir::Backend::Markdown`] (ADR 0001): one
/// [`MarkdownEntityMeta`] row per entity derived from the schema IR. The
/// vault configuration stays out of it: only `open_vault` applies it, and
/// the store never builds a vault.
pub fn generate(schema: &SchemaOutput, config: &MarkdownIoConfig) -> Result<MarkdownIoOutput, CodegenError> {
    let entities = &schema.entities;
    let mut errors = check_directories(entities, config.layout);
    if let Some(actor) = &config.okf.generated_by
        && let Err(e) = okf::check_generated_by(actor)
    {
        errors.push(e);
    }
    let diagnostics = okf::check_frontmatter_keys(entities, &schema.enums, &config.okf);
    errors.extend(diagnostics.errors);
    if !errors.is_empty() {
        return Err(CodegenError::Persistence(errors.join("\n")));
    }
    for warning in &diagnostics.warnings {
        println!("cargo:warning={warning}");
    }

    write_output(entities, config).map_err(CodegenError::Persistence)?;

    let entity_meta = entities
        .iter()
        .map(|entity| MarkdownEntityMeta {
            entity_name: entity.name.clone(),
            type_name: entity.type_name.clone(),
            dir_segment: entity.directory.clone(),
            body_field: entity.body_field().map(|f| f.name.clone()),
            // v1 contract: every many_to_many field is authoritative on the
            // declaring side; the reverse view is a derived walk.
            authoritative_m2m: entity.junction_relations().map(|(field, _)| field.name.clone()).collect(),
        })
        .collect();

    Ok(MarkdownIoOutput { module_path: "crate::persistence::markdown::generated".to_string(), entities: entity_meta })
}

/// One error per entity whose directory is a Windows device name, under
/// [`MarkdownLayout::PerEntityDir`], where the directory is a path segment.
/// Windows cannot create a `con` directory, so such a vault could not be
/// checked out there; the runtime refuses the segment too
/// (`markdown_store::layout::validate_segment`). A flat vault has no
/// entity directories.
fn check_directories(entities: &[crate::EntityDef], layout: MarkdownLayout) -> Vec<String> {
    if layout != MarkdownLayout::PerEntityDir {
        return Vec::new();
    }
    entities
        .iter()
        .filter(|entity| ontogen_core::id::is_device_name(&entity.directory))
        .map(|entity| {
            let (name, dir) = (&entity.name, &entity.directory);
            let source = if *dir == to_snake_case(name) {
                format!("its directory `{dir}` (from `directory = \"{dir}\"` or, without one, the entity name)")
            } else {
                format!("`directory = \"{dir}\"`")
            };
            format!(
                "entity `{name}`: {source} is a Windows device name (con, prn, aux, nul, com0-com9, lpt0-lpt9, in any \
                 case), and Windows cannot create a directory with that name: set `#[ontology(entity, directory = \
                 \"...\")]` to another name"
            )
        })
        .collect()
}

/// Write every module into `config.output_dir`, remove stale ones, and
/// list them all in its `mod.rs`, followed by `VAULT_ROOT` and `open_vault`.
fn write_output(entities: &[crate::EntityDef], config: &MarkdownIoConfig) -> Result<(), String> {
    let output_dir = &config.output_dir;
    std::fs::create_dir_all(output_dir).map_err(|e| format!("create {}: {e}", output_dir.display()))?;

    let mut modules: Vec<(String, String)> = entities
        .iter()
        .map(|entity| (to_snake_case(&entity.name), gen_frontmatter::generate_frontmatter_module(entity)))
        .collect();
    modules.sort_by(|a, b| a.0.cmp(&b.0));

    let expected: HashSet<String> =
        modules.iter().map(|(name, _)| format!("{name}.rs")).chain(std::iter::once("mod.rs".to_string())).collect();
    crate::clean_generated_dir(output_dir, &expected);

    let mut mod_rs = String::from("//! Generated by ontogen. DO NOT EDIT.\n\n");
    for (name, code) in &modules {
        crate::write_and_format(&output_dir.join(format!("{name}.rs")), code)
            .map_err(|e| format!("write {name}.rs: {e}"))?;
        mod_rs.push_str(&format!("pub mod {};\n", crate::ident::rust_ident(name)));
    }
    mod_rs.push('\n');
    mod_rs.push_str(&gen_vault::generate_open_vault(config));
    crate::write_and_format(&output_dir.join("mod.rs"), &mod_rs).map_err(|e| format!("write mod.rs: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EntityDef, OkfOptions};

    fn entity(name: &str, directory: &str) -> EntityDef {
        EntityDef {
            name: name.into(),
            doc: String::new(),
            directory: directory.into(),
            table: "rows".into(),
            type_name: name.into(),
            prefix: "row".into(),
            id_strategy: None,
            fields: vec![],
        }
    }

    fn generate_with(entities: Vec<EntityDef>, layout: MarkdownLayout) -> (tempfile::TempDir, Result<(), String>) {
        let tmp = tempfile::tempdir().unwrap();
        let config = crate::MarkdownIoConfig {
            output_dir: tmp.path().join("generated"),
            vault_root: "data/vault".into(),
            layout,
            list_cap: 10_000,
            okf: OkfOptions::default(),
        };
        let schema = SchemaOutput { entities, enums: vec![] };
        let result = match generate(&schema, &config) {
            Ok(_) => Ok(()),
            Err(CodegenError::Persistence(msg)) => Err(msg),
            Err(other) => panic!("expected a persistence error, got {other:?}"),
        };
        (tmp, result)
    }

    #[test]
    fn a_device_name_directory_fails_the_build_and_names_the_attribute() {
        let (tmp, result) = generate_with(vec![entity("Thing", "con")], MarkdownLayout::PerEntityDir);
        assert_eq!(
            result.unwrap_err(),
            "entity `Thing`: `directory = \"con\"` is a Windows device name (con, prn, aux, nul, com0-com9, \
             lpt0-lpt9, in any case), and Windows cannot create a directory with that name: set \
             `#[ontology(entity, directory = \"...\")]` to another name"
        );
        assert!(!tmp.path().join("generated").exists(), "nothing is generated");
    }

    #[test]
    fn a_directory_derived_from_a_device_name_entity_fails_the_build() {
        let (_tmp, result) =
            generate_with(vec![entity("Con", "con"), entity("Lpt0", "lpt0")], MarkdownLayout::PerEntityDir);
        let msg = result.unwrap_err();
        let lines: Vec<&str> = msg.lines().collect();
        assert_eq!(lines.len(), 2, "one error per entity: {msg}");
        assert!(
            lines[0].starts_with(
                "entity `Con`: its directory `con` (from `directory = \"con\"` or, without one, the entity name) is \
                 a Windows device name"
            ),
            "{msg}"
        );
        assert!(lines[1].starts_with("entity `Lpt0`: its directory `lpt0`"), "{msg}");
    }

    #[test]
    fn device_name_directories_are_checked_in_any_case_and_only_where_they_are_paths() {
        let (_tmp, result) = generate_with(vec![entity("Thing", "AUX")], MarkdownLayout::PerEntityDir);
        assert!(result.unwrap_err().starts_with("entity `Thing`: `directory = \"AUX\"` is a Windows device name"));
        let (_tmp, result) = generate_with(vec![entity("Con", "con")], MarkdownLayout::Flat);
        result.expect("a flat vault has no entity directories");
        let near = vec![entity("Console", "console"), entity("Com10", "com10"), entity("Contact", "con_tacts")];
        let (_tmp, result) = generate_with(near, MarkdownLayout::PerEntityDir);
        result.expect("near misses are ordinary directories");
    }

    #[test]
    fn a_keyword_entity_module_is_raw_and_its_methods_bare() {
        let (tmp, result) = generate_with(crate::schema::hostile_entities(), MarkdownLayout::PerEntityDir);
        result.expect("generate");
        let dir = tmp.path().join("generated");
        for file in ["mod.rs", "doc.rs", "order.rs", "match.rs"] {
            let code = std::fs::read_to_string(dir.join(file)).expect("read");
            syn::parse_file(&code).unwrap_or_else(|e| panic!("{file} is not Rust: {e}\n{code}"));
        }
        let mod_rs = std::fs::read_to_string(dir.join("mod.rs")).expect("read");
        assert!(mod_rs.contains("pub mod r#match;"), "{mod_rs}");
        let r#match = std::fs::read_to_string(dir.join("match.rs")).expect("read");
        assert!(r#match.contains("pub fn into_match(self, id: String) -> Match {"), "{}", r#match);
    }
}
