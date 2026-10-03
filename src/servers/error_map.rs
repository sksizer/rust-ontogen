//! The consumer's `AppError`, read from its schema directory, mapped to HTTP
//! statuses and JSON:API error codes (wire contract §13.4).
//!
//! Only variant names are read. A status comes from the name's suffix and the
//! `code` is the name in snake_case, so mapped and unmapped variants follow
//! one rule.

use std::fs;
use std::path::{Path, PathBuf};

use ontogen_core::naming::to_snake_case;
use ontogen_jsonapi::ErrorCode;

/// The consumer's `AppError` enum, one entry per variant in declaration order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ErrorMap {
    /// The file the enum was found in.
    pub source: PathBuf,
    pub variants: Vec<ErrorVariant>,
}

/// One `AppError` variant and the error object it becomes on the wire.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ErrorVariant {
    /// The Rust variant name (`TaskNotFound`).
    pub name: String,
    /// The wire `code`: the variant name in snake_case (`task_not_found`).
    pub code: String,
    pub status: u16,
    pub shape: VariantShape,
}

/// What a match pattern for the variant has to look like.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VariantShape {
    Unit,
    /// A tuple variant with this many fields.
    Tuple(usize),
    Struct,
}

impl ErrorVariant {
    /// A pattern matching this variant whatever its fields, e.g.
    /// `AppError::TaskNotFound(..)` for `enum_path` `AppError`.
    pub fn pattern(&self, enum_path: &str) -> String {
        match self.shape {
            VariantShape::Unit => format!("{enum_path}::{}", self.name),
            VariantShape::Tuple(_) => format!("{enum_path}::{}(..)", self.name),
            VariantShape::Struct => format!("{enum_path}::{} {{ .. }}", self.name),
        }
    }
}

/// Status for a variant name. The suffixes are the variants the generated
/// store constructs (`{Entity}NotFound`, `{Entity}IdRequired`,
/// `{Entity}AlreadyExists`, `{Child}ParentRequired`); anything else is the
/// consumer's own failure and maps to `500`.
fn status_for(variant: &str) -> u16 {
    const SUFFIXES: [(&str, u16); 4] =
        [("NotFound", 404), ("IdRequired", 400), ("AlreadyExists", 409), ("ParentRequired", 403)];
    SUFFIXES.iter().find(|(suffix, _)| variant.ends_with(suffix)).map_or(500, |(_, status)| *status)
}

/// Scan the top-level `*.rs` files of `dir` for `enum AppError`.
///
/// Files are read in sorted path order so the result does not depend on
/// directory iteration order. Enums nested in inline `mod` blocks are not
/// searched. Returns `None` when no file declares the enum, which is the
/// scan-dirs-only case: every `AppError`-typed site then maps to `500`.
///
/// # Errors
///
/// When the directory or a file cannot be read or parsed, when more than one
/// file declares `AppError`, or when a variant's code equals one of the codes
/// the generated server raises itself.
pub(crate) fn scan(dir: &Path) -> Result<Option<ErrorMap>, String> {
    let entries = fs::read_dir(dir)
        .map_err(|e| format!("ontogen: failed to read the AppError source directory {}: {e}", dir.display()))?;
    let mut paths: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && path.extension().is_some_and(|ext| ext == "rs"))
        .collect();
    paths.sort();

    let mut found: Option<(PathBuf, syn::ItemEnum)> = None;
    for path in paths {
        let source =
            fs::read_to_string(&path).map_err(|e| format!("ontogen: failed to read {}: {e}", path.display()))?;
        let file = syn::parse_file(&source).map_err(|e| format!("ontogen: failed to parse {}: {e}", path.display()))?;
        for item in file.items {
            let syn::Item::Enum(item) = item else { continue };
            if item.ident != "AppError" {
                continue;
            }
            if let Some((first, _)) = &found {
                return Err(format!(
                    "ontogen: `enum AppError` is declared more than once ({} and {}); declare it once so each error \
                     maps to one HTTP status",
                    first.display(),
                    path.display()
                ));
            }
            found = Some((path.clone(), item));
        }
    }

    let Some((source, item)) = found else { return Ok(None) };
    let variants = item.variants.iter().map(variant_of).collect::<Vec<_>>();
    check_codes(&variants, &source)?;
    Ok(Some(ErrorMap { source, variants }))
}

fn variant_of(variant: &syn::Variant) -> ErrorVariant {
    let name = variant.ident.to_string();
    let shape = match &variant.fields {
        syn::Fields::Unit => VariantShape::Unit,
        syn::Fields::Unnamed(fields) => VariantShape::Tuple(fields.unnamed.len()),
        syn::Fields::Named(_) => VariantShape::Struct,
    };
    ErrorVariant { code: to_snake_case(&name), status: status_for(&name), name, shape }
}

/// A `code` always means one thing, so a variant may not reuse a code the
/// generated server raises itself (wire contract §13.4, "Code clashes").
fn check_codes(variants: &[ErrorVariant], source: &Path) -> Result<(), String> {
    for v in variants {
        if ErrorCode::ALL.iter().any(|c| c.as_str() == v.code) {
            return Err(format!(
                "ontogen: `AppError::{}` in {} has the error code `{}`, which the generated JSON:API server already \
                 uses for its own errors; rename the variant",
                v.name,
                source.display(),
                v.code
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan_files(files: &[(&str, &str)]) -> Result<Option<ErrorMap>, String> {
        let dir = tempfile::tempdir().unwrap();
        for (name, body) in files {
            fs::write(dir.path().join(name), body).unwrap();
        }
        scan(dir.path())
    }

    fn summary(map: &ErrorMap) -> Vec<(&str, &str, u16, VariantShape)> {
        map.variants.iter().map(|v| (v.name.as_str(), v.code.as_str(), v.status, v.shape)).collect()
    }

    #[test]
    fn variants_map_by_suffix_in_declaration_order() {
        let map = scan_files(&[(
            "mod.rs",
            "#[derive(Debug)]\npub enum AppError {\n    TaskNotFound(String),\n    TaskIdRequired(String),\n    \
             TaskAlreadyExists(String),\n    SectionParentRequired(String),\n    DbError(String),\n    Md(String),\n}\n",
        )])
        .unwrap()
        .expect("AppError found");
        assert_eq!(
            summary(&map),
            vec![
                ("TaskNotFound", "task_not_found", 404, VariantShape::Tuple(1)),
                ("TaskIdRequired", "task_id_required", 400, VariantShape::Tuple(1)),
                ("TaskAlreadyExists", "task_already_exists", 409, VariantShape::Tuple(1)),
                ("SectionParentRequired", "section_parent_required", 403, VariantShape::Tuple(1)),
                ("DbError", "db_error", 500, VariantShape::Tuple(1)),
                ("Md", "md", 500, VariantShape::Tuple(1)),
            ]
        );
        assert!(map.source.ends_with("mod.rs"));
    }

    #[test]
    fn an_enum_with_no_mapped_variant_still_yields_its_codes() {
        let map = scan_files(&[("error.rs", "pub enum AppError { DbError(String), Other }")]).unwrap().unwrap();
        assert_eq!(
            summary(&map),
            vec![("DbError", "db_error", 500, VariantShape::Tuple(1)), ("Other", "other", 500, VariantShape::Unit)]
        );
    }

    #[test]
    fn variant_shapes_give_matching_patterns() {
        let map = scan_files(&[(
            "mod.rs",
            "pub enum AppError { Gone, Pair(String, u32), Detailed { id: String }, NoteNotFound(String) }",
        )])
        .unwrap()
        .unwrap();
        let patterns: Vec<String> = map.variants.iter().map(|v| v.pattern("crate::schema::AppError")).collect();
        assert_eq!(
            patterns,
            vec![
                "crate::schema::AppError::Gone",
                "crate::schema::AppError::Pair(..)",
                "crate::schema::AppError::Detailed { .. }",
                "crate::schema::AppError::NoteNotFound(..)",
            ]
        );
        assert_eq!(map.variants[1].shape, VariantShape::Tuple(2));
        assert_eq!(map.variants[2].shape, VariantShape::Struct);
    }

    #[test]
    fn no_app_error_is_none() {
        assert_eq!(scan_files(&[("task.rs", "pub struct Task; pub enum Status { Open }")]).unwrap(), None);
        assert_eq!(scan_files(&[]).unwrap(), None);
    }

    #[test]
    fn nested_and_non_rust_declarations_are_ignored() {
        let found = scan_files(&[
            ("mod.rs", "mod inner { pub enum AppError { NoteNotFound(String) } }"),
            ("notes.txt", "pub enum AppError { Md(String) }"),
        ])
        .unwrap();
        assert_eq!(found, None);
    }

    #[test]
    fn two_declarations_are_an_error_naming_both_files() {
        let err = scan_files(&[
            ("a.rs", "pub enum AppError { Md(String) }"),
            ("b.rs", "pub enum AppError { DbError(String) }"),
        ])
        .unwrap_err();
        assert!(err.contains("a.rs") && err.contains("b.rs"), "{err}");
    }

    #[test]
    fn a_variant_reusing_an_ontogen_code_is_an_error() {
        for variant in ["InvalidDocument", "RelatedResourceNotFound", "InternalError", "MethodNotAllowed"] {
            let err = scan_files(&[("mod.rs", &format!("pub enum AppError {{ {variant}(String) }}"))]).unwrap_err();
            assert!(err.contains(&format!("AppError::{variant}")) && err.contains(&to_snake_case(variant)), "{err}");
        }
    }

    #[test]
    fn an_unreadable_directory_or_file_is_an_error() {
        assert!(scan(Path::new("/nonexistent/ontogen/error-source")).is_err());
        assert!(scan_files(&[("mod.rs", "pub enum AppError {")]).unwrap_err().contains("mod.rs"));
    }

    #[test]
    fn every_in_tree_app_error_scans() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        for dir in [
            "examples/tasks-tracker/src/schema",
            "examples/notes-kb/src/schema",
            "examples/iron-log-md/src/schema",
            "examples/iron-log/src-tauri/src/schema",
            "crates/markdown-pilot/src/schema",
        ] {
            let map = scan(&root.join(dir)).unwrap_or_else(|e| panic!("{dir}: {e}"));
            assert!(map.is_some_and(|m| !m.variants.is_empty()), "{dir}: no AppError found");
        }
    }
}
