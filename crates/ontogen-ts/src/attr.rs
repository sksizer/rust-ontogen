//! Serde-attribute extraction on `syn::Attribute` lists.
//!
//! Phase-1 supports the rename family (`rename`, `rename_all`, `skip`) plus
//! field-level `default` (which maps to a TS-optional `?`), and rejects
//! shape-changing attrs (`tag`, `content`, `untagged`, `flatten`) plus
//! split-rename (`rename(serialize = "...", deserialize = "...")`) with an
//! [`EmitError::UnsupportedSerdeAttr`] carrying a hint at the symmetric form
//! or `#[ontogen::ts_opaque]`. Other serde attrs that don't change TS shape
//! (`borrow`, `bound`, `with`, `serialize_with`, etc.) are silently ignored.
//!
//! Parsing uses [`syn::Attribute::parse_nested_meta`] — the same primitive
//! `serde_derive`'s own parser uses — so the exact syntax we accept matches
//! what serde itself accepts.

use crate::types::{EmitError, RenameAll, TypePath};

/// Attributes on a container (struct or enum).
#[derive(Debug, Clone, Default)]
pub(crate) struct ContainerAttrs {
    /// `#[serde(rename_all = "...")]`.
    pub rename_all: Option<RenameAll>,
    /// `#[serde(rename = "...")]` on the container itself — overrides the
    /// container's TS name. Phase-1 emits structs/enums under their Rust
    /// ident; this field is parsed so future PRs can act on it.
    pub rename: Option<String>,
}

/// Attributes on a struct field.
#[derive(Debug, Clone, Default)]
pub(crate) struct FieldAttrs {
    /// `#[serde(rename = "...")]` on the field.
    pub rename: Option<String>,
    /// `#[serde(skip)]` (or `skip_serializing` / `skip_deserializing` — any of
    /// the three drops the field from TS emission since we can't represent a
    /// field that's serialized but not deserialized as a single TS type).
    pub skip: bool,
    /// `#[serde(default)]` or `#[serde(default = "path::to::fn")]`. The
    /// deserializer substitutes a default when the field is absent, so the
    /// wire contract treats the field as optional — the emitter renders it as
    /// a TS-optional `field?: T`.
    pub default: bool,
}

/// Attributes on an enum variant.
#[derive(Debug, Clone, Default)]
pub(crate) struct VariantAttrs {
    /// `#[serde(rename = "...")]` on the variant.
    pub rename: Option<String>,
    /// `#[serde(skip)]`.
    pub skip: bool,
}

impl RenameAll {
    /// Parse a serde `rename_all` literal (the string after `=`) into a
    /// [`RenameAll`] variant. Returns `None` if the literal isn't one of
    /// serde's eight recognized modes.
    pub(crate) fn from_serde_str(s: &str) -> Option<Self> {
        Some(match s {
            "lowercase" => Self::Lowercase,
            "UPPERCASE" => Self::Uppercase,
            "PascalCase" => Self::PascalCase,
            "camelCase" => Self::CamelCase,
            "snake_case" => Self::SnakeCase,
            "SCREAMING_SNAKE_CASE" => Self::ScreamingSnakeCase,
            "kebab-case" => Self::KebabCase,
            "SCREAMING-KEBAB-CASE" => Self::ScreamingKebabCase,
            _ => return None,
        })
    }
}

/// Phase-1 attrs that ontogen-ts rejects outright as "shape-changing serde
/// attributes" — these alter the JSON wire shape in ways that need a
/// dedicated emission path (phase 2 / OF-015 phase 2).
const REJECTED_SHAPE_ATTRS: &[&str] = &["tag", "content", "untagged", "flatten"];

/// Ontogen-specific attributes on a type definition. Both attrs are
/// no-ops at Rust compile time (the proc-macro implementations in
/// `ontogen-macros` pass the annotated item through unchanged); ontogen-ts
/// reads them via this extractor during scanning.
#[derive(Debug, Clone, Default)]
pub(crate) struct OntogenAttrs {
    /// `#[ts_opaque(target = "...")]` — emitter treats the type as terminal
    /// and emits `target` verbatim at every reference site.
    pub ts_opaque: Option<String>,
    /// `#[ts_name = "..."]` — overrides the TS name emitted for the type.
    /// JSON wire is unaffected.
    pub ts_name: Option<String>,
}

/// Extract `#[ts_opaque(target = "...")]` and `#[ts_name = "..."]` from a
/// `&[syn::Attribute]`. Matches on the terminal segment of the attribute
/// path so the attrs work whether imported bare, via `ontogen_macros::`,
/// or via `ontogen::` (the umbrella re-export).
pub(crate) fn extract_ontogen_attrs(
    attrs: &[syn::Attribute],
    referenced_by: &TypePath,
) -> Result<OntogenAttrs, EmitError> {
    let mut out = OntogenAttrs::default();
    for attr in attrs {
        let terminal = match attr.path().segments.last() {
            Some(seg) => seg.ident.to_string(),
            None => continue,
        };
        match terminal.as_str() {
            "ts_opaque" => {
                // Shape: `#[ts_opaque(target = "literal")]`. The macro
                // validates this at Rust compile time, so we expect a
                // well-formed input — but parse defensively.
                let mut target: Option<String> = None;
                attr.parse_nested_meta(|meta| {
                    if meta.path.is_ident("target") {
                        let value = meta.value()?;
                        let lit: syn::LitStr = value.parse()?;
                        target = Some(lit.value());
                        Ok(())
                    } else {
                        Err(meta.error("ts_opaque expects `target = \"...\"`"))
                    }
                })
                .map_err(|err| EmitError::UnsupportedSerdeAttr {
                    type_path: referenced_by.clone(),
                    attr: format!("could not parse #[ts_opaque(...)]: {err}"),
                })?;
                out.ts_opaque = target;
            }
            "ts_name" => {
                // Shape: `#[ts_name = "literal"]` (bare string literal arg).
                let value = match &attr.meta {
                    syn::Meta::NameValue(nv) => &nv.value,
                    _ => {
                        return Err(EmitError::UnsupportedSerdeAttr {
                            type_path: referenced_by.clone(),
                            attr: "#[ts_name = \"...\"] expects the `= \"literal\"` form".to_string(),
                        });
                    }
                };
                let lit = match value {
                    syn::Expr::Lit(expr_lit) => match &expr_lit.lit {
                        syn::Lit::Str(s) => s.value(),
                        _ => {
                            return Err(EmitError::UnsupportedSerdeAttr {
                                type_path: referenced_by.clone(),
                                attr: "#[ts_name = ...] value must be a string literal".to_string(),
                            });
                        }
                    },
                    _ => {
                        return Err(EmitError::UnsupportedSerdeAttr {
                            type_path: referenced_by.clone(),
                            attr: "#[ts_name = ...] value must be a string literal".to_string(),
                        });
                    }
                };
                out.ts_name = Some(lit);
            }
            _ => {}
        }
    }
    Ok(out)
}

/// Extract container-level serde attributes (struct or enum).
pub(crate) fn extract_container_attrs(
    attrs: &[syn::Attribute],
    referenced_by: &TypePath,
) -> Result<ContainerAttrs, EmitError> {
    let mut out = ContainerAttrs::default();
    for attr in attrs {
        if !attr.path().is_ident("serde") {
            continue;
        }
        walk_serde(attr, |meta_kind| {
            match meta_kind {
                MetaKind::RenameLit(value) => {
                    out.rename = Some(value);
                    Ok(())
                }
                MetaKind::RenameAllLit(value) => {
                    let mode = RenameAll::from_serde_str(&value).ok_or_else(|| EmitError::UnsupportedSerdeAttr {
                        type_path: referenced_by.clone(),
                        attr: format!("rename_all = \"{value}\" (not one of serde's eight recognized modes)"),
                    })?;
                    out.rename_all = Some(mode);
                    Ok(())
                }
                MetaKind::SplitRename | MetaKind::SplitRenameAll => Err(EmitError::UnsupportedSerdeAttr {
                    type_path: referenced_by.clone(),
                    attr: "split-rename (rename(serialize = \"...\", deserialize = \"...\")) is not supported in \
                           phase 1 — use the symmetric form #[serde(rename = \"...\")] or \
                           #[ontogen::ts_opaque(target = \"...\")] if the serde asymmetry must be preserved for \
                           non-ontogen-ts consumers"
                        .to_string(),
                }),
                MetaKind::RejectedShape(name) => Err(EmitError::UnsupportedSerdeAttr {
                    type_path: referenced_by.clone(),
                    attr: format!(
                        "serde({name}) — shape-changing attrs (tag/content/untagged/flatten) are phase 2 work; \
                         use #[ontogen::ts_opaque(target = \"...\")] if a custom TS rendering is needed"
                    ),
                }),
                MetaKind::Skip => Ok(()), // ignore at container level
                // Container-level `#[serde(default)]` is out of scope (it would
                // make every field optional); only field-level default maps to
                // a TS `?`. Ignore here.
                MetaKind::Default => Ok(()),
                MetaKind::Unknown => Ok(()),
            }
        })?;
    }
    Ok(out)
}

/// Extract field-level serde attributes.
pub(crate) fn extract_field_attrs(attrs: &[syn::Attribute], referenced_by: &TypePath) -> Result<FieldAttrs, EmitError> {
    let mut out = FieldAttrs::default();
    for attr in attrs {
        if !attr.path().is_ident("serde") {
            continue;
        }
        walk_serde(attr, |meta_kind| {
            match meta_kind {
                MetaKind::RenameLit(value) => {
                    out.rename = Some(value);
                    Ok(())
                }
                MetaKind::RenameAllLit(_) => Ok(()), // not meaningful on a field
                MetaKind::Skip => {
                    out.skip = true;
                    Ok(())
                }
                MetaKind::Default => {
                    out.default = true;
                    Ok(())
                }
                MetaKind::SplitRename => Err(EmitError::UnsupportedSerdeAttr {
                    type_path: referenced_by.clone(),
                    attr: "split-rename (rename(serialize = \"...\", deserialize = \"...\")) on a field is not \
                           supported in phase 1 — use the symmetric form #[serde(rename = \"...\")] or \
                           #[ontogen::ts_opaque(target = \"...\")] on the parent type"
                        .to_string(),
                }),
                MetaKind::SplitRenameAll | MetaKind::RejectedShape(_) | MetaKind::Unknown => Ok(()),
            }
        })?;
    }
    Ok(out)
}

/// Extract variant-level serde attributes.
pub(crate) fn extract_variant_attrs(
    attrs: &[syn::Attribute],
    referenced_by: &TypePath,
) -> Result<VariantAttrs, EmitError> {
    let mut out = VariantAttrs::default();
    for attr in attrs {
        if !attr.path().is_ident("serde") {
            continue;
        }
        walk_serde(attr, |meta_kind| match meta_kind {
            MetaKind::RenameLit(value) => {
                out.rename = Some(value);
                Ok(())
            }
            MetaKind::Skip => {
                out.skip = true;
                Ok(())
            }
            MetaKind::SplitRename => Err(EmitError::UnsupportedSerdeAttr {
                type_path: referenced_by.clone(),
                attr: "split-rename on a variant is not supported in phase 1 — use the symmetric form \
                           #[serde(rename = \"...\")]"
                    .to_string(),
            }),
            MetaKind::RenameAllLit(_)
            | MetaKind::SplitRenameAll
            | MetaKind::RejectedShape(_)
            | MetaKind::Default
            | MetaKind::Unknown => Ok(()),
        })?;
    }
    Ok(out)
}

/// Classified shape of a single nested serde meta item.
enum MetaKind {
    /// `rename = "wireName"` — symmetric.
    RenameLit(String),
    /// `rename_all = "camelCase"` — symmetric.
    RenameAllLit(String),
    /// `rename(serialize = "...", deserialize = "...")` — rejected.
    SplitRename,
    /// `rename_all(serialize = "...", deserialize = "...")` — rejected.
    SplitRenameAll,
    /// `tag`, `content`, `untagged`, `flatten` — rejected.
    RejectedShape(String),
    /// `skip`, `skip_serializing`, `skip_deserializing` — fold all three.
    Skip,
    /// `default` or `default = "path::to::fn"` — field is optional on the wire.
    Default,
    /// Anything we don't recognize is silently ignored.
    Unknown,
}

/// Helper for the outer walker: when an unknown / split-form inner meta has
/// a `= "lit"` value, consume the literal so the outer parser keeps going.
/// Used as the callback to `meta.parse_nested_meta(...)` when the outer code
/// doesn't care about the inner contents.
fn consume_inner_value(inner: syn::meta::ParseNestedMeta<'_>) -> syn::Result<()> {
    if let Ok(value) = inner.value() {
        let _: syn::Lit = value.parse()?;
    }
    Ok(())
}

/// Walk a `#[serde(...)]` attribute and call `f` on each classified nested
/// meta. Each invocation of `f` can return `Err(EmitError)` to short-circuit.
fn walk_serde<F>(attr: &syn::Attribute, mut f: F) -> Result<(), EmitError>
where
    F: FnMut(MetaKind) -> Result<(), EmitError>,
{
    // We collect both classification + any classifier-level EmitError, then
    // dispatch outside the parse_nested_meta closure (its return type is
    // syn::Result, not Result<_, EmitError>).
    let mut callbacks: Vec<MetaKind> = Vec::new();
    let parse_result = attr.parse_nested_meta(|meta| {
        let ident = match meta.path.get_ident() {
            Some(id) => id.to_string(),
            None => {
                callbacks.push(MetaKind::Unknown);
                return Ok(());
            }
        };
        match ident.as_str() {
            "rename" => {
                // Two shapes: `rename = "lit"` (symmetric) or `rename(...)`
                // (split). `meta.value()` returns Ok iff the next token is
                // `=`; an `Err` here means we're looking at the list form.
                match meta.value() {
                    Ok(value) => {
                        let lit: syn::LitStr = value.parse().map_err(|_| meta.error("expected string literal"))?;
                        callbacks.push(MetaKind::RenameLit(lit.value()));
                    }
                    Err(_) => {
                        // List form — split-rename. Consume the parens AND
                        // each inner `serialize = "..."` / `deserialize = "..."`
                        // so the outer parser doesn't choke. We don't care
                        // about the contents.
                        meta.parse_nested_meta(consume_inner_value)?;
                        callbacks.push(MetaKind::SplitRename);
                    }
                }
                Ok(())
            }
            "rename_all" => {
                match meta.value() {
                    Ok(value) => {
                        let lit: syn::LitStr = value.parse().map_err(|_| meta.error("expected string literal"))?;
                        callbacks.push(MetaKind::RenameAllLit(lit.value()));
                    }
                    Err(_) => {
                        meta.parse_nested_meta(consume_inner_value)?;
                        callbacks.push(MetaKind::SplitRenameAll);
                    }
                }
                Ok(())
            }
            "skip" | "skip_serializing" | "skip_deserializing" => {
                callbacks.push(MetaKind::Skip);
                Ok(())
            }
            "default" => {
                // Two shapes: bare `default` (no value) or the path form
                // `default = "module::fn"`. Both mean the same thing for TS
                // emission — the field may be absent on the wire — so consume
                // the value if present and classify both as `Default`.
                if let Ok(value) = meta.value() {
                    let _: syn::LitStr = value.parse().map_err(|_| meta.error("expected string literal"))?;
                }
                callbacks.push(MetaKind::Default);
                Ok(())
            }
            other if REJECTED_SHAPE_ATTRS.contains(&other) => {
                // These attrs may carry values; consume them if present so the
                // parser doesn't bail out.
                if let Ok(value) = meta.value() {
                    let _: syn::LitStr = value.parse().map_err(|_| meta.error("expected string literal"))?;
                }
                callbacks.push(MetaKind::RejectedShape(other.to_string()));
                Ok(())
            }
            _ => {
                // Unknown attr — consume any value or nested list form so the
                // outer parser stays in sync.
                if let Ok(value) = meta.value() {
                    let _: syn::Lit = value.parse().map_err(|_| meta.error("expected literal"))?;
                } else {
                    let _ = meta.parse_nested_meta(consume_inner_value);
                }
                callbacks.push(MetaKind::Unknown);
                Ok(())
            }
        }
    });
    if let Err(err) = parse_result {
        // syn parse errors on serde attrs are pretty rare in practice (well-
        // formed Rust source means the attribute parsed; this branch fires
        // only when serde syntax is malformed). Bubble it up as a generic
        // EmitError so callers can show the user where the problem is.
        return Err(EmitError::UnsupportedSerdeAttr {
            type_path: TypePath::new(vec!["<unknown>".to_string()]).expect("non-empty"),
            attr: format!("could not parse #[serde(...)]: {err}"),
        });
    }
    for cb in callbacks {
        f(cb)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{EmitError, TypePath};

    fn tp(name: &str) -> TypePath {
        TypePath::new(vec![name.to_string()]).expect("non-empty")
    }

    /// Parse a struct from source and return its attrs.
    fn struct_attrs(src: &str) -> Vec<syn::Attribute> {
        let item: syn::ItemStruct = syn::parse_str(src).expect("parse struct");
        item.attrs
    }

    /// Parse an enum from source and return its attrs.
    fn enum_attrs(src: &str) -> Vec<syn::Attribute> {
        let item: syn::ItemEnum = syn::parse_str(src).expect("parse enum");
        item.attrs
    }

    /// Parse the first named field of a struct and return its attrs.
    fn first_field_attrs(src: &str) -> Vec<syn::Attribute> {
        let item: syn::ItemStruct = syn::parse_str(src).expect("parse struct");
        let syn::Fields::Named(named) = item.fields else {
            panic!("test fixture must use named fields");
        };
        named.named.into_iter().next().expect("at least one field").attrs
    }

    /// Parse the first variant of an enum and return its attrs.
    fn first_variant_attrs(src: &str) -> Vec<syn::Attribute> {
        let item: syn::ItemEnum = syn::parse_str(src).expect("parse enum");
        item.variants.into_iter().next().expect("at least one variant").attrs
    }

    // ── Container: rename_all ─────────────────────────────────────────────

    #[test]
    fn container_rename_all_camel_case() {
        let attrs = struct_attrs(
            r#"
            #[serde(rename_all = "camelCase")]
            struct Foo { a: u32 }
            "#,
        );
        let out = extract_container_attrs(&attrs, &tp("Foo")).unwrap();
        assert_eq!(out.rename_all, Some(RenameAll::CamelCase));
    }

    #[test]
    fn container_rename_all_all_eight_modes() {
        let pairs = [
            ("lowercase", RenameAll::Lowercase),
            ("UPPERCASE", RenameAll::Uppercase),
            ("PascalCase", RenameAll::PascalCase),
            ("camelCase", RenameAll::CamelCase),
            ("snake_case", RenameAll::SnakeCase),
            ("SCREAMING_SNAKE_CASE", RenameAll::ScreamingSnakeCase),
            ("kebab-case", RenameAll::KebabCase),
            ("SCREAMING-KEBAB-CASE", RenameAll::ScreamingKebabCase),
        ];
        for (src, expected) in pairs {
            let attrs = struct_attrs(&format!(r#"#[serde(rename_all = "{src}")] struct Foo {{ a: u32 }}"#));
            let out = extract_container_attrs(&attrs, &tp("Foo")).unwrap();
            assert_eq!(out.rename_all, Some(expected), "rename_all = \"{src}\"");
        }
    }

    #[test]
    fn container_rename_all_unknown_mode_rejected() {
        let attrs = struct_attrs(
            r#"
            #[serde(rename_all = "Train-Case")]
            struct Foo { a: u32 }
            "#,
        );
        let err = extract_container_attrs(&attrs, &tp("Foo")).unwrap_err();
        match err {
            EmitError::UnsupportedSerdeAttr { attr, .. } => {
                assert!(attr.contains("Train-Case"), "attr was: {attr}");
                assert!(attr.contains("recognized modes"), "attr was: {attr}");
            }
            other => panic!("expected UnsupportedSerdeAttr, got {other:?}"),
        }
    }

    #[test]
    fn container_split_rename_all_rejected() {
        let attrs = struct_attrs(
            r#"
            #[serde(rename_all(serialize = "camelCase", deserialize = "snake_case"))]
            struct Foo { a: u32 }
            "#,
        );
        let err = extract_container_attrs(&attrs, &tp("Foo")).unwrap_err();
        match err {
            EmitError::UnsupportedSerdeAttr { attr, .. } => {
                assert!(attr.contains("split-rename"), "attr was: {attr}");
            }
            other => panic!("expected UnsupportedSerdeAttr, got {other:?}"),
        }
    }

    // ── Container: tag/content/untagged/flatten rejected ──────────────────

    #[test]
    fn container_tag_rejected() {
        let attrs = enum_attrs(
            r#"
            #[serde(tag = "type")]
            enum Msg { Click, Hover }
            "#,
        );
        let err = extract_container_attrs(&attrs, &tp("Msg")).unwrap_err();
        match err {
            EmitError::UnsupportedSerdeAttr { attr, .. } => {
                assert!(attr.contains("tag"), "attr was: {attr}");
                assert!(attr.contains("phase 2"), "attr was: {attr}");
            }
            other => panic!("expected UnsupportedSerdeAttr, got {other:?}"),
        }
    }

    #[test]
    fn container_untagged_rejected() {
        let attrs = enum_attrs(
            r#"
            #[serde(untagged)]
            enum U { A(u32), B(String) }
            "#,
        );
        let err = extract_container_attrs(&attrs, &tp("U")).unwrap_err();
        assert!(matches!(err, EmitError::UnsupportedSerdeAttr { .. }));
    }

    // ── Field: rename ─────────────────────────────────────────────────────

    #[test]
    fn field_rename() {
        let attrs = first_field_attrs(
            r#"
            struct Foo {
                #[serde(rename = "wireName")]
                pub a: u32,
            }
            "#,
        );
        let out = extract_field_attrs(&attrs, &tp("Foo")).unwrap();
        assert_eq!(out.rename.as_deref(), Some("wireName"));
        assert!(!out.skip);
    }

    #[test]
    fn field_skip() {
        let attrs = first_field_attrs(
            r#"
            struct Foo {
                #[serde(skip)]
                pub a: u32,
            }
            "#,
        );
        let out = extract_field_attrs(&attrs, &tp("Foo")).unwrap();
        assert!(out.skip);
    }

    #[test]
    fn field_skip_serializing_treated_as_skip() {
        let attrs = first_field_attrs(
            r#"
            struct Foo {
                #[serde(skip_serializing)]
                pub a: u32,
            }
            "#,
        );
        let out = extract_field_attrs(&attrs, &tp("Foo")).unwrap();
        assert!(out.skip);
    }

    #[test]
    fn field_split_rename_rejected() {
        let attrs = first_field_attrs(
            r#"
            struct Foo {
                #[serde(rename(serialize = "wire_name", deserialize = "wireName"))]
                pub a: u32,
            }
            "#,
        );
        let err = extract_field_attrs(&attrs, &tp("Foo")).unwrap_err();
        match err {
            EmitError::UnsupportedSerdeAttr { attr, .. } => {
                assert!(attr.contains("split-rename"), "attr was: {attr}");
                assert!(attr.contains("on a field"), "attr was: {attr}");
            }
            other => panic!("expected UnsupportedSerdeAttr, got {other:?}"),
        }
    }

    #[test]
    fn field_no_serde_attrs_returns_default() {
        let attrs = first_field_attrs(
            r#"
            struct Foo {
                pub a: u32,
            }
            "#,
        );
        let out = extract_field_attrs(&attrs, &tp("Foo")).unwrap();
        assert!(out.rename.is_none());
        assert!(!out.skip);
    }

    #[test]
    fn field_default_bare_sets_flag() {
        // `#[serde(default)]` marks the field optional on the wire.
        let attrs = first_field_attrs(
            r#"
            struct Foo {
                #[serde(default)]
                pub a: u32,
            }
            "#,
        );
        let out = extract_field_attrs(&attrs, &tp("Foo")).unwrap();
        assert!(out.default, "bare #[serde(default)] should set the default flag");
        assert!(out.rename.is_none());
        assert!(!out.skip);
    }

    #[test]
    fn field_default_path_form_sets_flag() {
        // `#[serde(default = "path")]` means the same thing for TS emission.
        let attrs = first_field_attrs(
            r#"
            struct Foo {
                #[serde(default = "defaults::a")]
                pub a: u32,
            }
            "#,
        );
        let out = extract_field_attrs(&attrs, &tp("Foo")).unwrap();
        assert!(out.default, "path-form #[serde(default = \"...\")] should set the default flag");
    }

    #[test]
    fn field_without_default_leaves_flag_unset() {
        let attrs = first_field_attrs(
            r#"
            struct Foo {
                pub a: u32,
            }
            "#,
        );
        let out = extract_field_attrs(&attrs, &tp("Foo")).unwrap();
        assert!(!out.default);
    }

    #[test]
    fn container_default_is_ignored() {
        // Container-level `#[serde(default)]` is out of scope — it doesn't make
        // every field individually optional in our emission.
        let attrs = struct_attrs(
            r#"
            #[serde(default)]
            struct Foo { a: u32 }
            "#,
        );
        // Parses without error; no field-level effect to assert here.
        extract_container_attrs(&attrs, &tp("Foo")).unwrap();
    }

    // ── Variant: rename ───────────────────────────────────────────────────

    #[test]
    fn variant_rename() {
        let attrs = first_variant_attrs(
            r#"
            enum Color {
                #[serde(rename = "rouge")]
                Red,
            }
            "#,
        );
        let out = extract_variant_attrs(&attrs, &tp("Color")).unwrap();
        assert_eq!(out.rename.as_deref(), Some("rouge"));
    }

    #[test]
    fn variant_split_rename_rejected() {
        let attrs = first_variant_attrs(
            r#"
            enum Color {
                #[serde(rename(serialize = "Red", deserialize = "red"))]
                Red,
            }
            "#,
        );
        let err = extract_variant_attrs(&attrs, &tp("Color")).unwrap_err();
        match err {
            EmitError::UnsupportedSerdeAttr { attr, .. } => {
                assert!(attr.contains("split-rename"), "attr was: {attr}");
                assert!(attr.contains("variant"), "attr was: {attr}");
            }
            other => panic!("expected UnsupportedSerdeAttr, got {other:?}"),
        }
    }

    // ── Combined: container + field interaction (parser side only) ────────

    #[test]
    fn container_rename_and_rename_all_both_parsed() {
        let attrs = struct_attrs(
            r#"
            #[serde(rename = "FooDto", rename_all = "camelCase")]
            struct Foo { a: u32 }
            "#,
        );
        let out = extract_container_attrs(&attrs, &tp("Foo")).unwrap();
        assert_eq!(out.rename.as_deref(), Some("FooDto"));
        assert_eq!(out.rename_all, Some(RenameAll::CamelCase));
    }
}
