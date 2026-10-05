//! The backend-independent ordering block of a store module (ADR 0006 §1,
//! §3): `{Entity}SortField` and `sort_{plural}`.
//!
//! Both backends emit this block from here, so its text is identical across
//! them. The markdown `list_*` orders with `sort_{plural}`; the SeaORM one
//! states the same rule in SQL (`order_{plural}_query`), and the runtime
//! parity suite holds the two to the same result.

use crate::schema::model::{EntityDef, EnumDef, FieldType};
use crate::schema::sort::{SortFieldSpec, SortKind, sort_fields};
use crate::store::helpers::{pluralize, to_snake_case};

/// The `{Entity}SortField` enum's name.
pub(crate) fn sort_field_type(entity: &EntityDef) -> String {
    format!("{}SortField", entity.name)
}

/// Emit `{Entity}SortField`, its `SortField` impl, `sort_{plural}` and the
/// comparator behind it. The entity must have an id field (the store stage
/// refuses one without).
pub(crate) fn generate_order_block(code: &mut String, entity: &EntityDef, enums: &[EnumDef], schema_path: &str) {
    let specs = sort_fields(entity, enums);
    let name = &entity.name;
    let plural = pluralize(&to_snake_case(name));
    let sort_field = sort_field_type(entity);

    code.push_str(&format!(
        "/// The fields `list_{plural}` can order by. `name()` is the sort key the transports accept.\n"
    ));
    code.push_str("#[derive(Debug, Clone, Copy, PartialEq, Eq)]\n");
    code.push_str(&format!("pub enum {sort_field} {{\n"));
    for spec in &specs {
        code.push_str(&format!("    {},\n", spec.variant));
    }
    code.push_str("}\n\n");

    let all: Vec<String> = specs.iter().map(|s| format!("Self::{}", s.variant)).collect();
    code.push_str(&format!("impl ontogen_core::order::SortField for {sort_field} {{\n"));
    code.push_str(&format!("    const ALL: &'static [Self] = &[{}];\n", all.join(", ")));
    code.push_str("    const ID: Self = Self::Id;\n\n");
    code.push_str("    fn name(self) -> &'static str {\n");
    code.push_str("        match self {\n");
    for spec in &specs {
        code.push_str(&format!("            Self::{} => {:?},\n", spec.variant, spec.name));
    }
    code.push_str("        }\n");
    code.push_str("    }\n");
    code.push_str("}\n\n");

    code.push_str(&format!(
        "/// Orders `items` as `list_{plural}` does on either backend: by each key of `order`, then by\n"
    ));
    code.push_str("/// id. A hand-written list that filters in memory sorts with this before it cuts a page.\n");
    code.push_str(&format!("pub fn sort_{plural}(items: &mut [{name}], order: &[OrderBy<{sort_field}>]) {{\n"));
    code.push_str("    let keys = ontogen_core::order::effective(order);\n");
    code.push_str("    items.sort_by(|a, b| {\n");
    code.push_str("        keys.iter()\n");
    code.push_str(&format!("            .map(|key| key.direction.apply(compare_{plural}(a, b, key.field)))\n"));
    code.push_str("            .find(|ord| ord.is_ne())\n");
    code.push_str("            .unwrap_or(::std::cmp::Ordering::Equal)\n");
    code.push_str("    });\n");
    code.push_str("}\n\n");

    code.push_str(&format!(
        "/// One ascending key of [`sort_{plural}`]: `None` first, floats with `-0.0` equal to `0.0`, enums\n"
    ));
    code.push_str("/// by the string they are stored as, which is what SQL compares.\n");
    code.push_str(&format!(
        "fn compare_{plural}(a: &{name}, b: &{name}, field: {sort_field}) -> ::std::cmp::Ordering {{\n"
    ));
    code.push_str("    match field {\n");
    for spec in &specs {
        code.push_str(&format!("        {sort_field}::{} => {},\n", spec.variant, comparison(spec)));
    }
    code.push_str("    }\n");
    code.push_str("}\n\n");

    let mut emitted: Vec<&str> = Vec::new();
    for spec in &specs {
        let SortKind::Enum(def) = spec.kind else { continue };
        if emitted.contains(&def.name.as_str()) {
            continue;
        }
        emitted.push(&def.name);
        emit_stored_string_fn(code, def, schema_path);
    }
}

/// The expression comparing `a` and `b` on one field, ascending.
fn comparison(spec: &SortFieldSpec<'_>) -> String {
    let f = &spec.field.name;
    match spec.kind {
        SortKind::Id | SortKind::String | SortKind::Integer | SortKind::Bool => format!("a.{f}.cmp(&b.{f})"),
        SortKind::Float => {
            let widen = |v: &str| match spec.field.field_type {
                FieldType::F32 | FieldType::OptionF32 => format!("f64::from({v})"),
                _ => v.to_string(),
            };
            if spec.optional {
                format!(
                    "match (a.{f}, b.{f}) {{ (Some(x), Some(y)) => ontogen_core::order::cmp_f64({}, {}), (x, y) => x.is_some().cmp(&y.is_some()) }}",
                    widen("x"),
                    widen("y")
                )
            } else {
                format!("ontogen_core::order::cmp_f64({}, {})", widen(&format!("a.{f}")), widen(&format!("b.{f}")))
            }
        }
        SortKind::Enum(def) => {
            let stored = stored_string_fn(def);
            if spec.optional {
                format!("a.{f}.as_ref().map({stored}).cmp(&b.{f}.as_ref().map({stored}))")
            } else {
                format!("{stored}(&a.{f}).cmp({stored}(&b.{f}))")
            }
        }
    }
}

fn stored_string_fn(def: &EnumDef) -> String {
    format!("{}_stored", to_snake_case(&def.name))
}

/// Emit `{enum}_stored`, the string a value of `def` is stored as on both
/// backends.
fn emit_stored_string_fn(code: &mut String, def: &EnumDef, schema_path: &str) {
    let ty = format!("{schema_path}::{}", def.name);
    code.push_str(&format!("/// The string a `{}` is stored as on either backend.\n", def.name));
    code.push_str(&format!("fn {}(value: &{ty}) -> &'static str {{\n", stored_string_fn(def)));
    code.push_str("    match value {\n");
    for variant in &def.variants {
        code.push_str(&format!("        {ty}::{} => {:?},\n", variant.name, variant.value));
    }
    // The schema's enum list leaves out `#[serde(skip)]` variants. Such a
    // value cannot be stored, so where it sorts only matters to an
    // in-memory caller, and this arm is unreachable when there are none.
    code.push_str("        // A `#[serde(skip)]` variant is never stored.\n");
    code.push_str("        #[allow(unreachable_patterns)]\n");
    code.push_str("        _ => \"\",\n");
    code.push_str("    }\n");
    code.push_str("}\n\n");
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::schema::parse::{parse_schema_enums_source, parse_schema_source};

    fn block(source: &str, name: &str) -> String {
        let path = Path::new("schema.rs");
        let entities = parse_schema_source(source, path).expect("schema parses");
        let enums = parse_schema_enums_source(source, path).expect("enums parse");
        let entity = entities.iter().find(|e| e.name == name).expect("entity");
        let mut code = String::from("use ontogen_core::order::OrderBy;\n");
        generate_order_block(&mut code, entity, &enums, "crate::schema");
        code
    }

    const PAPER: &str = r#"
        #[serde(rename_all = "kebab-case")]
        pub enum Kind { PeerReviewed, Draft }

        #[derive(OntologyEntity)]
        #[ontology(entity, directory = "papers", table = "papers")]
        pub struct Paper {
            #[ontology(id)]
            pub slug: String,
            pub title: String,
            pub pages: u32,
            pub score: f32,
            pub weight: Option<f64>,
            pub open: Option<bool>,
            pub kind: Kind,
            pub maybe_kind: Option<Kind>,
            pub tags: Vec<String>,
            #[ontology(body)]
            pub body: String,
        }
    "#;

    #[test]
    fn the_sort_field_enum_names_every_sortable_field() {
        let code = block(PAPER, "Paper");
        assert!(code.contains("pub enum PaperSortField {\n    Id,\n    Title,\n    Pages,\n    Score,\n    Weight,\n    Open,\n    Kind,\n    MaybeKind,\n}"), "{code}");
        assert!(code.contains("const ALL: &'static [Self] = &[Self::Id, Self::Title, Self::Pages, Self::Score, Self::Weight, Self::Open, Self::Kind, Self::MaybeKind];"), "{code}");
        assert!(code.contains("const ID: Self = Self::Id;"), "{code}");
        assert!(code.contains("Self::Id => \"id\","), "{code}");
        assert!(code.contains("Self::MaybeKind => \"maybe_kind\","), "{code}");
        assert!(!code.contains("Tags") && !code.contains("Body"), "{code}");
    }

    #[test]
    fn each_kind_compares_as_adr_0006_says() {
        let code = block(PAPER, "Paper");
        for expected in [
            "PaperSortField::Id => a.slug.cmp(&b.slug),",
            "PaperSortField::Title => a.title.cmp(&b.title),",
            "PaperSortField::Pages => a.pages.cmp(&b.pages),",
            "PaperSortField::Score => ontogen_core::order::cmp_f64(f64::from(a.score), f64::from(b.score)),",
            "PaperSortField::Weight => match (a.weight, b.weight) { (Some(x), Some(y)) => ontogen_core::order::cmp_f64(x, y), (x, y) => x.is_some().cmp(&y.is_some()) },",
            "PaperSortField::Open => a.open.cmp(&b.open),",
            "PaperSortField::Kind => kind_stored(&a.kind).cmp(kind_stored(&b.kind)),",
            "PaperSortField::MaybeKind => a.maybe_kind.as_ref().map(kind_stored).cmp(&b.maybe_kind.as_ref().map(kind_stored)),",
        ] {
            assert!(code.contains(expected), "missing `{expected}`:\n{code}");
        }
    }

    #[test]
    fn an_enum_compares_by_its_stored_string_once_per_enum() {
        let code = block(PAPER, "Paper");
        assert_eq!(code.matches("fn kind_stored(").count(), 1, "{code}");
        assert!(code.contains("crate::schema::Kind::PeerReviewed => \"peer-reviewed\","), "{code}");
        assert!(code.contains("crate::schema::Kind::Draft => \"draft\","), "{code}");
        assert!(code.contains("#[allow(unreachable_patterns)]\n        _ => \"\","), "{code}");
    }

    #[test]
    fn the_sort_applies_the_effective_keys_per_direction() {
        let code = block(PAPER, "Paper");
        assert!(code.contains("pub fn sort_papers(items: &mut [Paper], order: &[OrderBy<PaperSortField>]) {"));
        assert!(code.contains("let keys = ontogen_core::order::effective(order);"));
        assert!(code.contains("items.sort_by("), "a stable sort keeps the tie-break the only decider");
        assert!(code.contains(".map(|key| key.direction.apply(compare_papers(a, b, key.field)))"));
    }

    #[test]
    fn an_entity_with_only_an_id_sorts_by_id() {
        let code = block(
            "#[derive(OntologyEntity)]\n#[ontology(entity)]\npub struct Tag { #[ontology(id)] pub id: String, pub links: Vec<String> }",
            "Tag",
        );
        assert!(code.contains("pub enum TagSortField {\n    Id,\n}"), "{code}");
        assert!(!code.contains("_stored"), "no enum, no stored-string fn: {code}");
    }

    #[test]
    fn generated_code_is_valid_rust() {
        let code = block(PAPER, "Paper");
        syn::parse_file(&code).unwrap_or_else(|e| panic!("invalid Rust: {e}\n{code}"));
    }
}
