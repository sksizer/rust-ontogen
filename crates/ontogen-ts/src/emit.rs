//! Per-type and top-level emission entry points.
//!
//! - [`emit`] is the public entry point. It walks the type pool, resolves
//!   names + ontogen attrs, runs Kahn's topological sort, and emits TS
//!   for each reachable type via the per-type emitters below.
//! - [`emit_type`] renders a `syn::Type` as TS.
//! - [`emit_struct`] / [`emit_enum`] render a `syn::ItemStruct` /
//!   `syn::ItemEnum`. Their `_named` siblings accept an explicit name
//!   override (used when `#[ts_name = "..."]` is set on the type).

use std::collections::BTreeMap;

use syn::{
    Fields, GenericArgument, ItemEnum, ItemStruct, PathArguments, Type, TypeArray, TypePath as SynTypePath,
    TypeReference, TypeSlice, TypeTuple,
};

use crate::attr::{
    ContainerAttrs, FieldAttrs, VariantAttrs, extract_container_attrs, extract_field_attrs, extract_ontogen_attrs,
    extract_variant_attrs,
};
use crate::order;
use crate::resolve::ModuleImports;
use crate::types::{BigIntBehavior, EmitConfig, EmitError, RenameAll, TypePath};

/// Emit TypeScript source for `roots` and everything they transitively reach
/// in `type_pool`, honoring `config`.
///
/// Pipeline (PR 4):
///
/// 1. Build the dependency graph over `type_pool`.
/// 2. Compute transitive closure from `roots`.
/// 3. Resolve a TS name for each reachable type (honoring
///    `#[ts_name = "..."]` overrides).
/// 4. Detect name collisions on the reachable set — two types resolving
///    to the same TS name produce [`EmitError::NameCollision`].
/// 5. Topologically order the reachable set (cycle members co-emit at the
///    end; TS type aliases accept forward references).
/// 6. For each type in order:
///    - Read ontogen attrs. If `#[ts_opaque(target = "...")]` is set, emit
///      `export type Name = <target>;` and skip recursion into fields.
///    - Otherwise dispatch to [`emit_struct_named`] / [`emit_enum_named`]
///      with the resolved name.
///
/// All errors are collected into `Vec<EmitError>` before failing — never
/// first-error fail-fast, so a build surfaces every problem at once.
///
/// This convenience entry point resolves bare single-segment references
/// without per-module `use` tables (same-module and unique-terminal matching
/// only). To resolve references that come in through cross-module `use`
/// imports — including multi-level re-export chains — call
/// [`emit_with_imports`] with the [`ModuleImports`] returned by
/// [`crate::pool::scan_src_dir_with_imports`].
pub fn emit(
    roots: &[TypePath],
    type_pool: &BTreeMap<TypePath, syn::Item>,
    config: &EmitConfig,
) -> Result<String, Vec<EmitError>> {
    emit_with_imports(roots, type_pool, &ModuleImports::default(), config)
}

/// Like [`emit`], but resolves bare single-segment references through
/// `imports` — each referencing module's `use` table — so a type pulled in
/// via `use` (possibly through several re-export hops) links to the right
/// pool key even when several modules define same-terminal types.
pub fn emit_with_imports(
    roots: &[TypePath],
    type_pool: &BTreeMap<TypePath, syn::Item>,
    imports: &ModuleImports,
    config: &EmitConfig,
) -> Result<String, Vec<EmitError>> {
    let mut errors: Vec<EmitError> = Vec::new();

    // 1-2: dep graph + reachable closure.
    let graph = order::dependency_graph_with_imports(type_pool, imports);
    let reachable = order::reachable_from(roots, &graph);

    // Surface any root that isn't in the pool — caller passed a TypePath
    // we can't emit. Hard error; the build can't produce a meaningful
    // output without it.
    for root in roots {
        if !type_pool.contains_key(root) {
            errors.push(EmitError::UnresolvedReference {
                name: format!("root type `{root}` is not present in the type pool"),
                referenced_by: root.clone(),
            });
        }
    }

    // 3: resolve names. Walk reachable items; if extract_ontogen_attrs
    // surfaces an EmitError (malformed attr), collect it and use the
    // ident-derived fallback name.
    let mut names: BTreeMap<TypePath, String> = BTreeMap::new();
    for path in &reachable {
        let Some(item) = type_pool.get(path) else {
            continue;
        };
        let attrs = item_attrs(item);
        match extract_ontogen_attrs(attrs, path) {
            Ok(ontogen) => {
                let name = ontogen.ts_name.unwrap_or_else(|| path.terminal().to_string());
                names.insert(path.clone(), name);
            }
            Err(err) => {
                errors.push(err);
                names.insert(path.clone(), path.terminal().to_string());
            }
        }
    }

    // 4: name-collision detection (post-`ts_name` resolution).
    {
        let mut by_name: BTreeMap<String, Vec<TypePath>> = BTreeMap::new();
        for (path, name) in &names {
            by_name.entry(name.clone()).or_default().push(path.clone());
        }
        for (name, paths) in by_name {
            if paths.len() > 1 {
                errors.push(EmitError::NameCollision { name, paths });
            }
        }
    }

    // 5: topological order.
    let ordered = order::topo_order(&graph, &reachable);

    // 6: per-type emission.
    let mut outputs: Vec<String> = Vec::with_capacity(ordered.len());
    for path in &ordered {
        let Some(item) = type_pool.get(path) else {
            continue;
        };

        let ontogen_attrs = match extract_ontogen_attrs(item_attrs(item), path) {
            Ok(a) => a,
            Err(_) => continue, // already collected above
        };
        let resolved_name = names.get(path).cloned().unwrap_or_else(|| path.terminal().to_string());

        if let Some(target) = ontogen_attrs.ts_opaque {
            outputs.push(format!("export type {resolved_name} = {target};"));
            continue;
        }

        match item {
            syn::Item::Struct(s) => match emit_struct_named(s, config, Some(&resolved_name)) {
                Ok(ts) => outputs.push(ts),
                Err(e) => errors.push(e),
            },
            syn::Item::Enum(e) => match emit_enum_named(e, config, Some(&resolved_name)) {
                Ok(ts) => outputs.push(ts),
                Err(err) => errors.push(err),
            },
            syn::Item::Type(t) => {
                // Type alias: emit as `export type Name = <inner_ts>;`.
                // The walker would normally recurse, but for a top-level
                // alias the surface is the inner type directly.
                let synthetic_path = TypePath::new(vec![path.terminal().to_string()]).expect("non-empty");
                match emit_type(&t.ty, config, &synthetic_path) {
                    Ok(inner) => outputs.push(format!("export type {resolved_name} = {inner};")),
                    Err(err) => errors.push(err),
                }
            }
            _ => {
                // Pool walker only inserts struct/enum/type aliases, so
                // this branch shouldn't fire in practice.
            }
        }
    }

    if !errors.is_empty() {
        return Err(errors);
    }

    Ok(outputs.join("\n\n"))
}

/// Pull the attribute list off any `syn::Item` shape the pool stores.
fn item_attrs(item: &syn::Item) -> &[syn::Attribute] {
    match item {
        syn::Item::Struct(s) => &s.attrs,
        syn::Item::Enum(e) => &e.attrs,
        syn::Item::Type(t) => &t.attrs,
        _ => &[],
    }
}

/// Diagnostic stand-in for the "type whose field we're rendering" when a type
/// is rendered on its own, outside any declaration. See [`render_type`].
const STANDALONE: &str = "<standalone type>";

/// Synthetic [`TypePath`] used as `referenced_by` for standalone renders.
fn standalone_path() -> TypePath {
    TypePath::new(vec![STANDALONE.to_string()]).expect("non-empty")
}

/// Render one Rust type as TypeScript, with no type pool and no surrounding
/// declaration.
///
/// This is the same classifier [`emit`] uses for struct fields and enum
/// payloads — smart-pointer peeling, container generics, primitives, and the
/// external-types table — exposed for callers that hold a single type rather
/// than a pool of declarations. User-defined types render as their terminal
/// ident, exactly as they do inside a declaration; resolving that ident to a
/// definition is the caller's business.
///
/// ontogen's API-signature emitter (`rust_type_to_ts`) delegates here, which
/// is what keeps the two TypeScript emitters from disagreeing on a shape they
/// both have to render into the same generated file.
pub fn render_type(ty: &Type, config: &EmitConfig) -> Result<String, EmitError> {
    emit_type(ty, config, &standalone_path())
}

/// Parse `rust_ty` as a Rust type expression and render it via
/// [`render_type`].
///
/// For callers whose type arrived as text — a rendered token stream, a
/// signature scraped from source — rather than as a `syn::Type`. Spacing is
/// whatever `syn` tolerates, so token-stream renderings like `Vec < String >`
/// parse fine.
///
/// Returns [`EmitError::UnsupportedShape`] if the text doesn't parse as a
/// type.
pub fn render_type_str(rust_ty: &str, config: &EmitConfig) -> Result<String, EmitError> {
    let parsed: Type = syn::parse_str(rust_ty).map_err(|err| EmitError::UnsupportedShape {
        type_path: standalone_path(),
        reason: format!("`{rust_ty}` does not parse as a Rust type expression: {err}"),
    })?;
    render_type(&parsed, config)
}

/// Render a `syn::Type` as its TypeScript equivalent.
///
/// Classification order (matches the OF-015 design pass):
///
/// 1. **Smart-pointer peel** — `Box<T>`, `Rc<T>`, `Arc<T>`, `Cow<'_, T>`,
///    `Pin<P>` are stripped and the inner type is re-classified. All five
///    are transparent to `serde_json` at runtime.
/// 2. **Runtime-coordination rejection** — `RefCell<T>`, `Mutex<T>`,
///    `RwLock<T>` produce [`EmitError::UnsupportedShape`]. These shouldn't
///    appear in wire types.
/// 3. **Container generics** — `Option<T>` → `T | null`, `Vec<T>` → `T[]`,
///    `HashMap<K, V>` / `BTreeMap<K, V>` → `Record<K, V>` (key validated as
///    `String` or id-like primitive).
/// 4. **Reference types** — `&T` recurses on `T`; `&[T]` recurses as
///    `Vec<T>`; `&str` lands on the `str` primitive path which renders as
///    `string`.
/// 5. **Primitives** — `bool` → `boolean`; integer types → `number` (or
///    `bigint`/`string` for 64-bit ints if [`EmitConfig::bigint_behavior`]
///    requests it); `f32`/`f64` → `number`; `String`/`str` → `string`.
/// 6. **Fall-through** — anything else (a user-defined struct/enum ident)
///    is rendered as the terminal ident verbatim. PR 3 replaces this with
///    real pool / external-types lookup; the placeholder here lets per-type
///    unit tests run without the full walking infrastructure.
///
/// `referenced_by` names the type whose field we're classifying — it
/// surfaces in `EmitError`s for diagnostic context. Phase 1 doesn't have a
/// "synthetic path" mechanism, so unit tests pass a contrived single-segment
/// path.
pub(crate) fn emit_type(ty: &Type, config: &EmitConfig, referenced_by: &TypePath) -> Result<String, EmitError> {
    // 1. Peel smart-pointer wrappers before any further classification.
    if let Some(inner) = peel_smart_pointer(ty) {
        return emit_type(inner, config, referenced_by);
    }

    // 4a. References — `&T` recurses on `T`; `&[T]` becomes `Vec<T>`-ish.
    if let Type::Reference(TypeReference { elem, .. }) = ty {
        // `&[T]` → render like `Vec<T>` for wire equivalence.
        if let Type::Slice(TypeSlice { elem: slice_elem, .. }) = elem.as_ref() {
            let inner = emit_type(slice_elem, config, referenced_by)?;
            return Ok(format!("{inner}[]"));
        }
        return emit_type(elem, config, referenced_by);
    }

    // `[T; N]` is treated like a slice — same wire shape (a JSON array).
    if let Type::Array(TypeArray { elem, .. }) = ty {
        let inner = emit_type(elem, config, referenced_by)?;
        return Ok(format!("{inner}[]"));
    }

    // Bare `[T]` — can show up as the inner of a peeled `Cow<'a, [T]>`. Same
    // wire shape as `Vec<T>`.
    if let Type::Slice(TypeSlice { elem, .. }) = ty {
        let inner = emit_type(elem, config, referenced_by)?;
        return Ok(format!("{inner}[]"));
    }

    // The unit type is `null` on the wire — serde serializes `()` as JSON
    // `null`. It reaches here from handler signatures that return nothing.
    // Non-empty tuples stay unsupported: they serialize as JSON arrays, but
    // TS tuple syntax needs an element-by-element rendering that phase 1's
    // field-name-driven model has no place for.
    if let Type::Tuple(TypeTuple { elems, .. }) = ty {
        if elems.is_empty() {
            return Ok("null".to_string());
        }
        return Err(EmitError::UnsupportedShape {
            type_path: referenced_by.clone(),
            reason: format!("tuple type `{}` is not supported; use a named struct", quote::quote!(#ty)),
        });
    }

    // Everything else lives on a `syn::TypePath`.
    let path = match ty {
        Type::Path(p) => p,
        other => {
            return Err(EmitError::UnsupportedShape {
                type_path: referenced_by.clone(),
                reason: format!("type expression `{}` is not supported in phase 1", quote::quote!(#other)),
            });
        }
    };

    // 2. Runtime-coordination wrappers are rejected hard. Match on terminal
    // ident regardless of whether the user wrote generic args explicitly.
    if let Some(name) = terminal_ident(path)
        && matches!(name.as_str(), "RefCell" | "Mutex" | "RwLock")
    {
        return Err(EmitError::UnsupportedShape {
            type_path: referenced_by.clone(),
            reason: format!(
                "{name}<T> is a runtime-coordination primitive and shouldn't appear in wire types; refactor or \
                 use #[ontogen::ts_opaque]"
            ),
        });
    }

    // 3. Container generics with hardcoded TS renderings.
    if let Some(container) = match_container(path) {
        return emit_container(container, config, referenced_by);
    }

    // 5. Primitives by terminal ident.
    if let Some(name) = single_segment_ident(path)
        && let Some(rendered) = primitive_ts(&name, config)
    {
        return Ok(rendered.to_string());
    }

    // 6. Fall-through — check the external-types table first, then the
    // terminal ident as a last resort. PR 3 added `crate::external` for
    // the lookup; PR 4 wires it here. Multi-segment paths like
    // `chrono::DateTime<Utc>` strip generic args and consult the table,
    // returning `"string"` (the default for chrono::DateTime). Anything
    // not in the table falls back to the terminal ident, leaving the
    // top-level `emit` composition to look it up in the type pool.
    let segments: Vec<String> = path.path.segments.iter().map(|s| s.ident.to_string()).collect();
    if segments.is_empty() {
        return Err(EmitError::UnsupportedShape {
            type_path: referenced_by.clone(),
            reason: "type path had no segments".to_string(),
        });
    }

    // Strip leading `crate::` so external-types lookup uses canonical form
    // (the table keys are full canonical paths like `chrono::DateTime`).
    let mut canonical_segs = segments.clone();
    if canonical_segs.first().map(String::as_str) == Some("crate") {
        canonical_segs.remove(0);
    }
    if let Ok(canonical) = TypePath::new(canonical_segs)
        && let Some(rendering) = crate::external::resolve(&canonical, &config.external_types)
    {
        return Ok(rendering);
    }

    // Final fall-through: the terminal ident verbatim. emit's top-level
    // composition handles pool lookup at the type-declaration level;
    // here we just emit the name and trust that downstream renders cover
    // the rest.
    Ok(segments.last().expect("non-empty after the early return above").clone())
}

/// Emit a `syn::ItemStruct` as a TypeScript `export type Name = { ... };`
/// declaration.
///
/// Only named-field structs are supported in phase 1. Tuple structs
/// (`struct Foo(u32, u32)`) and unit structs (`struct Bar;`) return
/// [`EmitError::UnsupportedShape`] — the OF-014 spike survey showed neither
/// shape carries enough name information to round-trip cleanly through TS
/// without inventing field names. Users can wrap in a named-field struct or
/// reach for `#[ontogen::ts_opaque]`.
///
/// Serde renames ARE applied as of PR 2. Precedence (matching serde's own
/// rules so wire names round-trip through `serde_json::to_string`):
///
/// 1. `#[serde(rename = "wireName")]` on the field — wins outright.
/// 2. `#[serde(rename_all = "...")]` on the container — applied to the field
///    ident.
/// 3. `EmitConfig::case_default` — applied to the field ident if neither of
///    the above is set.
/// 4. Field ident verbatim.
///
/// `#[serde(skip)]` (and the `skip_serializing` / `skip_deserializing` siblings)
/// drops the field entirely.
///
/// `#[serde(default)]` (bare or the `default = "path"` form) renders the field
/// as TS-optional (`field?: T`) — the deserializer accepts partial JSON for
/// the field, so the emitted contract matches the wire. It composes with
/// `Option<T>` → `T | null` to produce `field?: T | null`.
///
/// The same attribute on the *container* fills every absent field from the
/// struct's `Default`, so it marks every field optional — a struct that
/// accepts `{}` on the wire emits a TS type whose properties are all `?`.
///
/// `#[serde(flatten)]` splices the field type's keys into the parent object
/// instead of nesting them under the field name, so the field becomes a TS
/// intersection member rather than a property:
///
/// ```text
/// #[serde(flatten)] meta: StepMeta,   →   export type Step =
/// program: String,                        StepMeta & { program: string };
/// ```
///
/// Flattened members are emitted in field-declaration order ahead of the
/// property object; if every field is flattened, the (empty) object is
/// dropped and the type is the bare intersection. See [`flatten_member`] for
/// which field types are admissible.
#[allow(dead_code)] // tests-only convenience wrapper; production calls _named directly.
pub(crate) fn emit_struct(item: &ItemStruct, config: &EmitConfig) -> Result<String, EmitError> {
    emit_struct_named(item, config, None)
}

/// Emit a struct with an optional TS name override (used by the top-level
/// composition when `#[ts_name = "..."]` is present on the type).
pub(crate) fn emit_struct_named(
    item: &ItemStruct,
    config: &EmitConfig,
    name_override: Option<&str>,
) -> Result<String, EmitError> {
    let raw_name = item.ident.to_string();
    let name = name_override.map(str::to_string).unwrap_or_else(|| raw_name.clone());
    let referenced_by = TypePath::new(vec![raw_name]).expect("single segment is non-empty");

    let container = extract_container_attrs(&item.attrs, &referenced_by)?;
    let effective_rename_all = container.rename_all.or(config.case_default);

    match &item.fields {
        Fields::Named(fields) => {
            let collected =
                collect_named_fields(fields, config, &referenced_by, effective_rename_all, container.default)?;
            // `struct Foo {}` — or all fields skipped/flattened. `None` here
            // means "no property object at all": with flattened members it
            // drops out of the intersection, without them it renders `{}`
            // rather than multi-line empties.
            let object = (!collected.properties.is_empty()).then(|| {
                let body = collected
                    .properties
                    .iter()
                    .map(|(key, opt, ty_ts)| format!("  {key}{opt}: {ty_ts};"))
                    .collect::<Vec<_>>()
                    .join("\n");
                format!("{{\n{body}\n}}")
            });
            Ok(format!("export type {name} = {};", intersect(&collected.intersections, object)))
        }
        Fields::Unnamed(_) => Err(EmitError::UnsupportedShape {
            type_path: referenced_by,
            reason: "tuple structs are not supported in phase 1; wrap in a named-field struct or use \
                     #[ontogen::ts_opaque]"
                .to_string(),
        }),
        Fields::Unit => Err(EmitError::UnsupportedShape {
            type_path: referenced_by,
            reason: "unit structs are not supported in phase 1; use a named-field struct or #[ontogen::ts_opaque]"
                .to_string(),
        }),
    }
}

/// A named-field group — a struct body or an enum struct-variant body —
/// split into the two pieces TypeScript renders differently.
struct NamedFields {
    /// TS type expressions contributed by `#[serde(flatten)]` fields, in
    /// declaration order. Empty in the common case, which is what keeps
    /// non-flatten output byte-identical to the pre-flatten emitter.
    intersections: Vec<String>,
    /// `(ts_key, optional_marker, ts_type)` for each surviving property.
    properties: Vec<(String, &'static str, String)>,
}

/// Classify each field of a named-field group into a flattened intersection
/// member or an ordinary property, applying `#[serde(skip)]`, the rename
/// family, and `#[serde(default)]` along the way.
///
/// `container_default` is `#[serde(default)]` on the struct itself. Serde
/// fills every absent field from the struct's `Default`, so it makes the
/// whole body optional — each field is treated exactly as if it carried its
/// own `#[serde(default)]`.
fn collect_named_fields(
    fields: &syn::FieldsNamed,
    config: &EmitConfig,
    referenced_by: &TypePath,
    rename_all: Option<RenameAll>,
    container_default: bool,
) -> Result<NamedFields, EmitError> {
    let mut out = NamedFields { intersections: Vec::new(), properties: Vec::with_capacity(fields.named.len()) };
    for field in &fields.named {
        let field_attrs = extract_field_attrs(&field.attrs, referenced_by)?;
        if field_attrs.skip {
            continue;
        }
        // Either scope of `default` makes this field absent-able on the wire.
        let defaulted = field_attrs.default || container_default;
        if field_attrs.flatten {
            // A flattened field's own name never reaches the wire, so the
            // rename family is moot here — serde ignores it too.
            out.intersections.push(flatten_member(&field.ty, defaulted, config, referenced_by)?);
            continue;
        }
        let raw_ident = field.ident.as_ref().expect("Fields::Named guarantees a field ident").to_string();
        let wire_name = field_wire_name(&raw_ident, &field_attrs, rename_all);
        let key = format_ts_key(&wire_name);
        let ty_ts = emit_type(&field.ty, config, referenced_by)?;
        // A defaulted field may be absent on the wire — the deserializer
        // fills in a default. Emit it as TS-optional. Composes with
        // `Option<T>` → `T | null` to give `field?: T | null` for an
        // optional, nullable field.
        let opt = if defaulted { "?" } else { "" };
        out.properties.push((key, opt, ty_ts));
    }
    Ok(out)
}

/// Fold flattened intersection `members` together with the group's property
/// `object` (already rendered; `None` when the group has no properties).
///
/// With no flattened members this returns the object unchanged — or `{}` for
/// an empty group — so output for the overwhelmingly common case is exactly
/// what the emitter produced before `flatten` was supported. `A & {}` is
/// just `A`, so an empty object drops out of a non-empty intersection.
fn intersect(members: &[String], object: Option<String>) -> String {
    match (members.is_empty(), object) {
        (true, Some(object)) => object,
        (true, None) => "{}".to_string(),
        (false, Some(object)) => format!("{} & {object}", members.join(" & ")),
        (false, None) => members.join(" & "),
    }
}

/// Render the TS intersection member contributed by a `#[serde(flatten)]`
/// field, or reject the field with a hard error.
///
/// Serde splices the flattened type's keys into the parent object, and the
/// structural equivalent in TypeScript is an intersection. That only works
/// when the field type renders to something object-shaped, so everything
/// else is rejected rather than silently emitted:
///
/// - **`Option<T>`, and `#[serde(flatten, default)]`** — both make the whole
///   flattened group absent-or-present as a unit. An intersection can't say
///   that, and `Partial<T>` would wrongly admit any subset of the keys.
/// - **`Vec<T>` / sets / primitives / `serde_json::Value`** — these render
///   as `T[]`, `string`, `unknown`, and so on. `X & unknown` is a silent
///   no-op and `X & string` collapses to `never`, so either way the emitted
///   type would stop describing the wire.
///
/// String-keyed maps ARE admissible, which covers serde's catch-all idiom:
/// `#[serde(flatten)] extra: HashMap<String, Value>` renders as
/// `Record<string, unknown>` and intersects correctly.
///
/// Smart-pointer wrappers are peeled first — `Box<StepMeta>` flattens
/// exactly like `StepMeta` does, since serde sees through both.
///
/// One case this can't catch: flattening a field whose type is an *enum*.
/// `emit_type` only yields the referenced name, not its definition, so the
/// emitter can't tell `StepMeta` (struct) from `StepKind` (enum) here. The
/// resulting intersection is well-formed TS that resolves to `never`, which
/// surfaces at the consumer's `tsc` rather than at emit time.
fn flatten_member(
    ty: &Type,
    defaulted: bool,
    config: &EmitConfig,
    referenced_by: &TypePath,
) -> Result<String, EmitError> {
    if defaulted {
        return Err(EmitError::UnsupportedShape {
            type_path: referenced_by.clone(),
            reason: "a defaulted #[serde(flatten)] field (whether from `#[serde(flatten, default)]` or a container \
                     `#[serde(default)]`) makes the whole flattened group absent-or-present as a unit, which a TS \
                     intersection can't express; drop the `default` or use #[ontogen::ts_opaque(target = \"...\")]"
                .to_string(),
        });
    }

    let mut inner = ty;
    while let Some(peeled) = peel_smart_pointer(inner) {
        inner = peeled;
    }

    if let Type::Path(path) = inner
        && matches!(match_container(path), Some(Container::Option(_)))
    {
        return Err(EmitError::UnsupportedShape {
            type_path: referenced_by.clone(),
            reason: "#[serde(flatten)] on an Option<T> makes the whole flattened group absent-or-present as a unit, \
                     which a TS intersection can't express; flatten a non-Option field or use \
                     #[ontogen::ts_opaque(target = \"...\")]"
                .to_string(),
        });
    }

    let rendered = emit_type(inner, config, referenced_by)?;
    if !is_object_shaped(&rendered) {
        return Err(EmitError::UnsupportedShape {
            type_path: referenced_by.clone(),
            reason: format!(
                "#[serde(flatten)] needs a field type that renders to a TS object, but this one renders as \
                 `{rendered}`; intersecting that would void or silently drop the parent type. Flatten a struct or a \
                 map, or use #[ontogen::ts_opaque(target = \"...\")]"
            ),
        });
    }
    Ok(rendered)
}

/// TS keywords that never denote a useful intersection member for a
/// flattened field. `object` is included: it's nominally an object type but
/// carries no keys, so `X & object` says nothing about the wire.
const NON_OBJECT_TS_KEYWORDS: &[&str] = &[
    "any",
    "bigint",
    "boolean",
    "never",
    "null",
    "number",
    "object",
    "string",
    "symbol",
    "undefined",
    "unknown",
    "void",
];

/// True iff `rendered` is a TS type expression that can meaningfully take
/// part in an object intersection — a `Record<...>` mapped type, or a named
/// type reference. Unions (`T | null`), arrays (`T[]`) and the primitive
/// keywords all fail, since `is_valid_ts_ident` rejects the first two and
/// [`NON_OBJECT_TS_KEYWORDS`] the third.
fn is_object_shaped(rendered: &str) -> bool {
    if rendered.starts_with("Record<") {
        return true;
    }
    is_valid_ts_ident(rendered) && !NON_OBJECT_TS_KEYWORDS.contains(&rendered)
}

/// Compute the on-the-wire name for a struct field given the field's serde
/// attrs and the container's effective rename_all mode.
fn field_wire_name(raw_ident: &str, attrs: &FieldAttrs, rename_all: Option<RenameAll>) -> String {
    if let Some(explicit) = &attrs.rename {
        return explicit.clone();
    }
    if let Some(mode) = rename_all {
        return mode.apply_to_field(raw_ident);
    }
    raw_ident.to_string()
}

/// Pick the rename mode that governs the FIELD names inside a struct
/// variant.
///
/// Serde keeps two independent axes on an enum, and conflating them emits TS
/// that disagrees with the wire in a way that still looks plausible:
///
/// | attribute                              | renames                       |
/// |----------------------------------------|-------------------------------|
/// | `#[serde(rename_all)]` on the enum     | the variants                  |
/// | `#[serde(rename_all_fields)]` on the enum | fields of every struct variant |
/// | `#[serde(rename_all)]` on a variant    | fields of that variant        |
///
/// So the enum's own `rename_all` is deliberately absent here — it governs
/// the variant key and nothing else. The closer scope wins between the
/// remaining two.
///
/// [`EmitConfig::case_default`] is deliberately absent too. It stands in for
/// "this crate annotates its types with `rename_all`", and a crate that does
/// exactly that still gets verbatim struct-variant field names out of serde
/// unless it also writes `rename_all_fields`. Folding `case_default` in here
/// would reintroduce the same mismatch from the config side.
fn variant_field_rename_all(container: &ContainerAttrs, variant: &VariantAttrs) -> Option<RenameAll> {
    variant.rename_all.or(container.rename_all_fields)
}

/// Compute the on-the-wire name for an enum variant given its serde attrs
/// and the container's effective rename_all mode.
fn variant_wire_name(raw_ident: &str, attrs: &VariantAttrs, rename_all: Option<RenameAll>) -> String {
    if let Some(explicit) = &attrs.rename {
        return explicit.clone();
    }
    if let Some(mode) = rename_all {
        return mode.apply_to_variant(raw_ident);
    }
    raw_ident.to_string()
}

/// Render `name` as a TypeScript object-literal key. Bare-ident if it's a
/// valid JS/TS identifier, double-quoted-string otherwise.
fn format_ts_key(name: &str) -> String {
    if is_valid_ts_ident(name) {
        name.to_string()
    } else {
        // Quote-escape: backslash and double-quote get escaped; other JSON
        // escapes aren't necessary because serde rename targets in practice
        // are well-behaved ASCII strings.
        let escaped = name.replace('\\', "\\\\").replace('"', "\\\"");
        format!("\"{escaped}\"")
    }
}

/// Render `s` as a TS string literal using the [`QuoteStyle`] declared on
/// `config`. Single-quoted by default; consumers wanting double-quoted
/// output (e.g. to match Prettier's default or pre-ontogen-ts specta
/// emission) flip [`EmitConfig::quote_style`] to [`QuoteStyle::Double`].
///
/// Centralizes the one-place-to-change for every quoted-literal emit site
/// in the emitter — today that's enum variant wire names in string-literal
/// unions. Wire names are bare-ident-shaped in the supported phase-1
/// subset (or `#[serde(rename = "...")]` outputs that we already format
/// via [`format_ts_key`] for keys); embedded matching quote characters
/// don't appear in practice, so no escaping is performed here. If a
/// future phase admits arbitrary user-controlled strings into a literal
/// position, escaping belongs here.
fn quote(config: &EmitConfig, s: &str) -> String {
    let d = config.quote_style.delimiter();
    format!("{d}{s}{d}")
}

/// True iff `s` is a valid TypeScript identifier (ASCII subset — `[A-Za-z_$]`
/// followed by `[A-Za-z0-9_$]*`). Conservative — strict TS allows more
/// Unicode in idents but for serde rename targets the ASCII subset is the
/// realistic surface.
fn is_valid_ts_ident(s: &str) -> bool {
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_' || first == '$') {
        return false;
    }
    chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '$')
}

/// Emit a `syn::ItemEnum` as a TypeScript union type.
///
/// Variant shape determines the rendering:
///
/// - **All variants C-style (no payload)** — emits a string-literal union:
///   `export type Color = 'Red' | 'Green' | 'Blue';`
/// - **One or more variants carry a payload** — emits the externally-tagged
///   shape that `serde_json` produces by default for non-`#[serde(tag)]`
///   enums:
///   `export type Msg = { Click: ClickPayload } | { Hover: HoverPayload }
///   | 'Ping';` (where `'Ping'` is the C-style variant).
///
/// Externally-tagged is the right default because `serde_json` emits
/// `{"VariantName": payload}` for variant-with-payload values when no
/// `#[serde(tag = "...")]` is set. Internally / adjacently / untagged enum
/// representations are phase-2 work (gated behind `#[serde(tag)]` /
/// `#[serde(untagged)]` — PR 2 rejects these attrs, full support is OF-015
/// phase 2).
///
/// Empty enums (`enum Foo {}`) emit as `never` since they have no
/// inhabitants — matches `serde_json::to_string`'s effective behavior
/// (calling code can't ever construct a value).
///
/// Struct variants run the same named-field collector as
/// [`emit_struct_named`], so `#[serde(flatten)]` and `#[serde(default)]`
/// behave identically inside a variant payload: a flattened field becomes an
/// intersection member on the payload (`{ Move: Base & { x: number } }`) and
/// a defaulted field becomes TS-optional.
///
/// Renaming inside a struct variant follows a *different* policy from the
/// variant key — the enum's `rename_all` renames variants only, never their
/// fields. See [`variant_field_rename_all`] for the full precedence table.
#[allow(dead_code)] // tests-only convenience wrapper; production calls _named directly.
pub(crate) fn emit_enum(item: &ItemEnum, config: &EmitConfig) -> Result<String, EmitError> {
    emit_enum_named(item, config, None)
}

/// Emit an enum with an optional TS name override.
pub(crate) fn emit_enum_named(
    item: &ItemEnum,
    config: &EmitConfig,
    name_override: Option<&str>,
) -> Result<String, EmitError> {
    let raw_name = item.ident.to_string();
    let name = name_override.map(str::to_string).unwrap_or_else(|| raw_name.clone());
    let referenced_by = TypePath::new(vec![raw_name]).expect("single segment is non-empty");

    let container = extract_container_attrs(&item.attrs, &referenced_by)?;
    let effective_rename_all = container.rename_all.or(config.case_default);

    if item.variants.is_empty() {
        return Ok(format!("export type {name} = never;"));
    }

    let mut variant_lines: Vec<String> = Vec::with_capacity(item.variants.len());
    for variant in &item.variants {
        let variant_attrs = extract_variant_attrs(&variant.attrs, &referenced_by)?;
        if variant_attrs.skip {
            continue;
        }
        let raw_ident = variant.ident.to_string();
        let wire_name = variant_wire_name(&raw_ident, &variant_attrs, effective_rename_all);
        match &variant.fields {
            Fields::Unit => {
                // C-style — string-literal variant. Quote style follows
                // `config.quote_style` (single by default; consumers flip
                // to double via `EmitConfig::quote_style`). Bare-ident wire
                // names need no escaping; non-ident ones still fit inside
                // the chosen delimiter (TS string literal syntax accepts
                // them).
                variant_lines.push(quote(config, &wire_name));
            }
            Fields::Unnamed(fields) => {
                // Tuple-style variant. Serde's default external-tag emission
                // wraps a single payload as `{"V": payload}` and a multi-arg
                // tuple as `{"V": [a, b, c]}`. Phase-1 supports the
                // single-payload case; multi-arg tuple variants are rejected
                // as unsupported shape (users can refactor into a struct
                // variant for clarity).
                let key = format_ts_key(&wire_name);
                match fields.unnamed.len() {
                    0 => variant_lines.push(quote(config, &wire_name)),
                    1 => {
                        let payload_ts = emit_type(&fields.unnamed[0].ty, config, &referenced_by)?;
                        variant_lines.push(format!("{{ {key}: {payload_ts} }}"));
                    }
                    _ => {
                        return Err(EmitError::UnsupportedShape {
                            type_path: referenced_by,
                            reason: format!(
                                "enum variant `{raw_ident}` has {} tuple fields; phase-1 supports unit, single-tuple, \
                                 or struct variants (refactor into a struct variant for multi-field payloads)",
                                fields.unnamed.len()
                            ),
                        });
                    }
                }
            }
            Fields::Named(fields) => {
                // Struct-style variant: serde emits `{"V": {field1: ..., field2: ...}}`.
                //
                // The variant KEY is renamed by the enum's `rename_all`
                // (already applied above, in `wire_name`). The variant's
                // FIELD names are a separate axis that the enum's
                // `rename_all` does not touch — see `variant_field_rename_all`.
                let key = format_ts_key(&wire_name);
                let field_rename_all = variant_field_rename_all(&container, &variant_attrs);
                // No container-default to inherit: serde rejects
                // `#[serde(default)]` on an enum outright, so a variant body
                // is only optional field-by-field.
                let collected = collect_named_fields(fields, config, &referenced_by, field_rename_all, false)?;
                let object = (!collected.properties.is_empty()).then(|| {
                    let body = collected
                        .properties
                        .iter()
                        .map(|(field_key, opt, ty_ts)| format!("{field_key}{opt}: {ty_ts}"))
                        .collect::<Vec<_>>()
                        .join("; ");
                    format!("{{ {body} }}")
                });
                let payload = intersect(&collected.intersections, object);
                variant_lines.push(format!("{{ {key}: {payload} }}"));
            }
        }
    }

    // If every variant was `#[serde(skip)]`, fall back to `never`.
    if variant_lines.is_empty() {
        return Ok(format!("export type {name} = never;"));
    }

    let body = variant_lines.join(" | ");
    Ok(format!("export type {name} = {body};"))
}

/// Wrapper types that are silently peeled before re-classification.
const SMART_POINTERS: &[&str] = &["Box", "Rc", "Arc", "Cow", "Pin"];

/// If `ty` is a single-arg generic wrapper in [`SMART_POINTERS`], return its
/// inner type. `Cow<'a, T>` skips the lifetime arg and returns `T`.
fn peel_smart_pointer(ty: &Type) -> Option<&Type> {
    let Type::Path(path) = ty else {
        return None;
    };
    let segment = path.path.segments.last()?;
    let name = segment.ident.to_string();
    if !SMART_POINTERS.contains(&name.as_str()) {
        return None;
    }
    let PathArguments::AngleBracketed(args) = &segment.arguments else {
        return None;
    };
    // Look for the first type-typed argument. `Cow<'a, T>` has a lifetime
    // first, so we skip non-type args. `Pin<P>` and `Box<T>` etc. have a
    // single type arg.
    args.args.iter().find_map(|arg| match arg {
        GenericArgument::Type(inner) => Some(inner),
        _ => None,
    })
}

/// Terminal ident of a path (e.g. `Mutex` from `std::sync::Mutex<T>`).
/// Returns `None` for paths with a `qself`.
fn terminal_ident(path: &SynTypePath) -> Option<String> {
    if path.qself.is_some() {
        return None;
    }
    path.path.segments.last().map(|s| s.ident.to_string())
}

/// True iff `path` is a single-segment ident with no generics. Returns the
/// ident as an owned `String`.
fn single_segment_ident(path: &SynTypePath) -> Option<String> {
    if path.qself.is_some() {
        return None;
    }
    if path.path.segments.len() != 1 {
        return None;
    }
    let segment = &path.path.segments[0];
    if !matches!(segment.arguments, PathArguments::None) {
        return None;
    }
    Some(segment.ident.to_string())
}

/// Container shape — one of the hardcoded phase-1 generics.
enum Container<'a> {
    /// `Option<T>`.
    Option(&'a Type),
    /// `Vec<T>`.
    Vec(&'a Type),
    /// `HashMap<K, V>` or `BTreeMap<K, V>`.
    Map(&'a Type, &'a Type),
    /// `HashSet<T>` or `BTreeSet<T>` — same wire shape as `Vec<T>`.
    Set(&'a Type),
}

/// Match `path` against the hardcoded container generics and return the
/// classified shape if applicable.
fn match_container(path: &SynTypePath) -> Option<Container<'_>> {
    if path.qself.is_some() {
        return None;
    }
    let segment = path.path.segments.last()?;
    let name = segment.ident.to_string();
    let PathArguments::AngleBracketed(args) = &segment.arguments else {
        return None;
    };

    let type_args: Vec<&Type> = args
        .args
        .iter()
        .filter_map(|arg| match arg {
            GenericArgument::Type(t) => Some(t),
            _ => None,
        })
        .collect();

    match (name.as_str(), type_args.as_slice()) {
        ("Option", [inner]) => Some(Container::Option(inner)),
        // `VecDeque` shares `Vec`'s wire shape — serde serializes it as a
        // JSON array. Without it here the type fell through to the terminal
        // ident and emitted a bare `VecDeque`, which is not a TS type.
        ("Vec" | "VecDeque", [inner]) => Some(Container::Vec(inner)),
        ("HashMap" | "BTreeMap", [k, v]) => Some(Container::Map(k, v)),
        ("HashSet" | "BTreeSet", [inner]) => Some(Container::Set(inner)),
        _ => None,
    }
}

/// Render a classified container.
fn emit_container(
    container: Container<'_>,
    config: &EmitConfig,
    referenced_by: &TypePath,
) -> Result<String, EmitError> {
    match container {
        Container::Option(inner) => {
            let rendered = emit_type(inner, config, referenced_by)?;
            // Wrap union shapes in parens to keep `T | null` unambiguous if T
            // itself happens to be a union (e.g. nested `Option<Option<T>>`,
            // which `serde_json` flattens but the schema-known emitter
            // preserves — phase-1 here renders the naive shape).
            if rendered.contains(" | ") { Ok(format!("({rendered}) | null")) } else { Ok(format!("{rendered} | null")) }
        }
        Container::Vec(inner) | Container::Set(inner) => {
            let rendered = emit_type(inner, config, referenced_by)?;
            // Array element types that contain `|` need parens to bind
            // tightly with the `[]` postfix.
            if rendered.contains(" | ") { Ok(format!("({rendered})[]")) } else { Ok(format!("{rendered}[]")) }
        }
        Container::Map(key, value) => {
            // TS `Record<K, V>` only accepts string-like / number-like /
            // symbol keys. Validate the key type renders as `string` or
            // `number`. Anything else is rejected.
            let key_ts = emit_type(key, config, referenced_by)?;
            if !is_record_key_renderable(&key_ts) {
                return Err(EmitError::UnsupportedShape {
                    type_path: referenced_by.clone(),
                    reason: format!(
                        "map key must render to `string` or a number-like primitive for TS `Record<K, V>`; got \
                         `{key_ts}`"
                    ),
                });
            }
            let value_ts = emit_type(value, config, referenced_by)?;
            Ok(format!("Record<{key_ts}, {value_ts}>"))
        }
    }
}

/// True iff the rendered key type is acceptable as a TS `Record<K, V>` key.
fn is_record_key_renderable(rendered: &str) -> bool {
    // `string` covers `String`/`&str`. `number` covers all integer + float
    // types; `bigint` is accepted by TS as a record key as of TS 4.4+.
    matches!(rendered, "string" | "number" | "bigint")
}

/// Map a primitive Rust ident to its TS rendering. Returns `None` for
/// non-primitives (callers fall through to pool / external-types lookup).
fn primitive_ts(name: &str, config: &EmitConfig) -> Option<&'static str> {
    match name {
        "bool" => Some("boolean"),
        // 64-bit-ish integers route through BigIntBehavior. `usize`/`isize`
        // are platform-dependent but treated as 64-bit for safety.
        "u64" | "i64" | "u128" | "i128" | "usize" | "isize" => Some(bigint_rendering(config.bigint_behavior)),
        // ≤32-bit integers and floats always fit `number`.
        "u8" | "u16" | "u32" | "i8" | "i16" | "i32" | "f32" | "f64" => Some("number"),
        // `char` serializes to a single-codepoint JSON string by default.
        "char" => Some("string"),
        // `String` and string slices — `&str` reaches us via the reference
        // arm above, but its inner type is `str` (a bare ident), which we
        // catch here.
        //
        // The remaining entries cover the rest of std's string-like family:
        // owned `PathBuf` / `OsString` / `CString` and their unsized borrow
        // forms `Path` / `OsStr` / `CStr`. All six serde-serialize as JSON
        // strings on the wire, so they render as TS `string`. Matching on
        // the terminal ident (rather than the full canonical path) catches
        // the common `use std::path::PathBuf;` + bare-name reference; the
        // multi-segment forms (`std::path::PathBuf`, etc.) are also covered
        // in `crate::external::DEFAULT_EXTERNAL_TYPES`.
        "String" | "str" | "PathBuf" | "Path" | "OsString" | "OsStr" | "CString" | "CStr" => Some("string"),
        _ => None,
    }
}

/// TS rendering for 64-bit integer types given the configured behavior.
fn bigint_rendering(behavior: BigIntBehavior) -> &'static str {
    match behavior {
        BigIntBehavior::Number => "number",
        BigIntBehavior::BigInt => "bigint",
        BigIntBehavior::String => "string",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::QuoteStyle;

    /// Convenience: build a single-segment `TypePath` for `referenced_by`.
    fn tp(name: &str) -> TypePath {
        TypePath::new(vec![name.to_string()]).expect("non-empty")
    }

    fn ty(src: &str) -> Type {
        syn::parse_str(src).unwrap_or_else(|err| panic!("failed to parse `{src}`: {err}"))
    }

    fn emit(src: &str) -> String {
        let config = EmitConfig::default();
        emit_type(&ty(src), &config, &tp("Test")).unwrap_or_else(|err| panic!("emit_type(`{src}`) errored: {err}"))
    }

    fn emit_err(src: &str) -> EmitError {
        let config = EmitConfig::default();
        emit_type(&ty(src), &config, &tp("Test")).expect_err("expected an EmitError")
    }

    // ── Primitives ──────────────────────────────────────────────────────

    #[test]
    fn primitive_bool() {
        assert_eq!(emit("bool"), "boolean");
    }

    #[test]
    fn primitive_small_integers_render_as_number() {
        for src in ["u8", "u16", "u32", "i8", "i16", "i32"] {
            assert_eq!(emit(src), "number", "{src} should render as number");
        }
    }

    #[test]
    fn primitive_floats_render_as_number() {
        assert_eq!(emit("f32"), "number");
        assert_eq!(emit("f64"), "number");
    }

    #[test]
    fn primitive_big_integers_default_to_number() {
        for src in ["u64", "i64", "u128", "i128", "usize", "isize"] {
            assert_eq!(emit(src), "number", "{src} should default to number");
        }
    }

    #[test]
    fn primitive_big_integers_honor_bigint_behavior() {
        let config = EmitConfig { bigint_behavior: BigIntBehavior::BigInt, ..Default::default() };
        let rendered = emit_type(&ty("u64"), &config, &tp("Test")).unwrap();
        assert_eq!(rendered, "bigint");

        let config = EmitConfig { bigint_behavior: BigIntBehavior::String, ..Default::default() };
        let rendered = emit_type(&ty("i64"), &config, &tp("Test")).unwrap();
        assert_eq!(rendered, "string");
    }

    #[test]
    fn primitive_string_owned_and_borrowed() {
        assert_eq!(emit("String"), "string");
        // `&str` reaches `str` via the reference arm.
        assert_eq!(emit("&str"), "string");
    }

    #[test]
    fn primitive_char_renders_as_string() {
        assert_eq!(emit("char"), "string");
    }

    #[test]
    fn primitive_std_string_like_types_render_as_string() {
        // Owned variants — common in DTO field positions.
        assert_eq!(emit("PathBuf"), "string");
        assert_eq!(emit("OsString"), "string");
        assert_eq!(emit("CString"), "string");
        // Unsized borrow forms — show up via Option<&Path>, etc.
        assert_eq!(emit("Path"), "string");
        assert_eq!(emit("OsStr"), "string");
        assert_eq!(emit("CStr"), "string");
        // Reference forms compose with the reference arm.
        assert_eq!(emit("&Path"), "string");
        assert_eq!(emit("Option<PathBuf>"), "string | null");
        assert_eq!(emit("Vec<PathBuf>"), "string[]");
    }

    #[test]
    fn std_string_like_full_path_resolves_through_external_table() {
        // The fall-through arm canonicalizes multi-segment paths against
        // the external-types table — confirm the `std::path::PathBuf`
        // (etc.) entries resolve to `string`.
        assert_eq!(emit("std::path::PathBuf"), "string");
        assert_eq!(emit("std::path::Path"), "string");
        assert_eq!(emit("std::ffi::OsString"), "string");
        assert_eq!(emit("std::ffi::OsStr"), "string");
        assert_eq!(emit("std::ffi::CString"), "string");
        assert_eq!(emit("std::ffi::CStr"), "string");
    }

    // ── Containers ──────────────────────────────────────────────────────

    #[test]
    fn container_option_renders_union_with_null() {
        assert_eq!(emit("Option<u32>"), "number | null");
        assert_eq!(emit("Option<String>"), "string | null");
    }

    #[test]
    fn container_vec_renders_as_array() {
        assert_eq!(emit("Vec<u32>"), "number[]");
        assert_eq!(emit("Vec<String>"), "string[]");
    }

    #[test]
    fn container_set_renders_as_array() {
        assert_eq!(emit("HashSet<u32>"), "number[]");
        assert_eq!(emit("BTreeSet<String>"), "string[]");
    }

    #[test]
    fn container_vecdeque_renders_as_array() {
        // `VecDeque` serializes as a JSON array like `Vec`, but was missing
        // from the container table, so it fell through to the terminal ident
        // and emitted a bare `VecDeque` — not a TS type.
        assert_eq!(emit("VecDeque<u32>"), "number[]");
        assert_eq!(emit("VecDeque<Option<String>>"), "(string | null)[]");
    }

    #[test]
    fn unit_type_renders_as_null() {
        // serde serializes `()` as JSON `null`.
        assert_eq!(emit("()"), "null");
        // Degenerate but well-defined: TS collapses the duplicate itself.
        assert_eq!(emit("Option<()>"), "null | null");
    }

    #[test]
    fn non_empty_tuple_is_rejected() {
        // A JSON array with positional meaning has no field names to hang a
        // TS object shape on; a named struct is the supported spelling.
        match emit_err("(String, u32)") {
            EmitError::UnsupportedShape { reason, .. } => {
                assert!(reason.contains("tuple"), "reason was: {reason}");
            }
            other => panic!("expected UnsupportedShape, got {other:?}"),
        }
    }

    #[test]
    fn container_hashmap_renders_as_record() {
        assert_eq!(emit("HashMap<String, u32>"), "Record<string, number>");
        assert_eq!(emit("BTreeMap<String, bool>"), "Record<string, boolean>");
    }

    #[test]
    fn container_hashmap_accepts_numeric_keys() {
        assert_eq!(emit("HashMap<u32, String>"), "Record<number, string>");
    }

    #[test]
    fn container_hashmap_rejects_unsupported_keys() {
        // A user-defined struct used as a key falls through emit_type to its
        // terminal-ident rendering, which isn't acceptable as a Record key.
        match emit_err("HashMap<MyKey, u32>") {
            EmitError::UnsupportedShape { reason, .. } => {
                assert!(reason.contains("map key"), "reason was: {reason}");
            }
            other => panic!("expected UnsupportedShape, got {other:?}"),
        }
    }

    #[test]
    fn container_nested_option_in_option() {
        // Naive phase-1 rendering — schema-known emitter handles the
        // `Option<Option<T>>` flattening separately.
        let rendered = emit("Option<Option<u32>>");
        assert_eq!(rendered, "(number | null) | null");
    }

    #[test]
    fn container_vec_of_options() {
        let rendered = emit("Vec<Option<u32>>");
        assert_eq!(rendered, "(number | null)[]");
    }

    // ── Smart-pointer peel ──────────────────────────────────────────────

    #[test]
    fn smart_pointer_box_is_transparent() {
        assert_eq!(emit("Box<u32>"), emit("u32"));
        assert_eq!(emit("Box<String>"), "string");
    }

    #[test]
    fn smart_pointer_rc_arc_are_transparent() {
        assert_eq!(emit("Rc<u32>"), "number");
        assert_eq!(emit("Arc<String>"), "string");
    }

    #[test]
    fn smart_pointer_cow_is_transparent() {
        // `Cow<'a, str>` — the lifetime gets skipped.
        assert_eq!(emit("Cow<'a, str>"), "string");
        assert_eq!(emit("Cow<'static, [u32]>"), "number[]");
    }

    #[test]
    fn smart_pointer_pin_is_transparent() {
        assert_eq!(emit("Pin<Box<u32>>"), "number");
    }

    #[test]
    fn smart_pointer_nested_peels_all_the_way() {
        // Arc<Box<Vec<Option<u32>>>> — every wrapper transparent.
        assert_eq!(emit("Arc<Box<Vec<Option<u32>>>>"), "(number | null)[]");
    }

    // ── References ─────────────────────────────────────────────────────

    #[test]
    fn reference_amp_t_unwraps_to_owned() {
        assert_eq!(emit("&u32"), "number");
        assert_eq!(emit("&String"), "string");
    }

    #[test]
    fn reference_amp_slice_renders_as_array() {
        assert_eq!(emit("&[u32]"), "number[]");
        assert_eq!(emit("&[String]"), "string[]");
    }

    #[test]
    fn reference_array_renders_as_array() {
        // `[u8; 32]` — fixed-size arrays share Vec's wire shape.
        assert_eq!(emit("[u8; 32]"), "number[]");
    }

    // ── Runtime-coordination wrappers ──────────────────────────────────

    #[test]
    fn refcell_is_rejected() {
        match emit_err("RefCell<u32>") {
            EmitError::UnsupportedShape { reason, .. } => {
                assert!(reason.contains("RefCell"), "reason was: {reason}");
            }
            other => panic!("expected UnsupportedShape, got {other:?}"),
        }
    }

    #[test]
    fn mutex_is_rejected() {
        match emit_err("Mutex<u32>") {
            EmitError::UnsupportedShape { reason, .. } => {
                assert!(reason.contains("Mutex"), "reason was: {reason}");
            }
            other => panic!("expected UnsupportedShape, got {other:?}"),
        }
    }

    #[test]
    fn rwlock_is_rejected() {
        match emit_err("RwLock<u32>") {
            EmitError::UnsupportedShape { reason, .. } => {
                assert!(reason.contains("RwLock"), "reason was: {reason}");
            }
            other => panic!("expected UnsupportedShape, got {other:?}"),
        }
    }

    // ── Fall-through ────────────────────────────────────────────────────

    #[test]
    fn unknown_ident_falls_through_to_terminal() {
        // Custom user struct — phase 1 emits the terminal ident as-is. PR 3
        // replaces this with pool / external-types lookup.
        assert_eq!(emit("Workout"), "Workout");
    }

    #[test]
    fn multi_segment_path_collapses_to_terminal_for_now() {
        // PR 3's canonicalization will replace this with a real lookup.
        assert_eq!(emit("crate::models::Workout"), "Workout");
    }

    // ── Standalone rendering (`render_type` / `render_type_str`) ───────

    #[test]
    fn render_type_str_matches_the_in_declaration_renderer() {
        // `render_type_str` is the entry point ontogen's API-signature
        // emitter delegates to. It has to agree with what the same type
        // renders as inside a struct field, or the two halves of one
        // generated file disagree — which is the whole reason it exists.
        let config = EmitConfig::default();
        for src in [
            "String",
            "u8",
            "Vec<Option<String>>",
            "HashMap<String, Vec<Node>>",
            "Cow<'a, str>",
            "chrono::DateTime<Utc>",
            "serde_json::Value",
            "()",
        ] {
            let standalone = render_type_str(src, &config).unwrap_or_else(|err| panic!("`{src}` failed: {err:?}"));
            assert_eq!(standalone, emit(src), "standalone render of `{src}` diverged");
        }
    }

    #[test]
    fn render_type_str_tolerates_token_stream_spacing() {
        // Callers hand it text rendered from a token stream, which carries
        // spaces the source spelling never had.
        let config = EmitConfig::default();
        assert_eq!(render_type_str("Vec < String >", &config).expect("renders"), "string[]");
        assert_eq!(render_type_str("HashMap < String , i32 >", &config).expect("renders"), "Record<string, number>");
    }

    #[test]
    fn render_type_str_rejects_text_that_is_not_a_type() {
        let config = EmitConfig::default();
        match render_type_str("not a type!", &config) {
            Err(EmitError::UnsupportedShape { reason, .. }) => {
                assert!(reason.contains("does not parse"), "reason was: {reason}");
            }
            other => panic!("expected a parse failure, got {other:?}"),
        }
    }

    #[test]
    fn render_type_honors_config() {
        // The config reaches the standalone path too — otherwise a consumer
        // setting `bigint_behavior` would get it applied to the long-tail
        // types and silently not to the signatures.
        let config = EmitConfig { bigint_behavior: BigIntBehavior::BigInt, ..EmitConfig::default() };
        assert_eq!(render_type_str("u64", &config).expect("renders"), "bigint");
        assert_eq!(render_type_str("u64", &EmitConfig::default()).expect("renders"), "number");
    }

    // ── Struct emission ────────────────────────────────────────────────

    fn struct_item(src: &str) -> syn::ItemStruct {
        syn::parse_str(src).unwrap_or_else(|err| panic!("failed to parse struct `{src}`: {err}"))
    }

    fn enum_item(src: &str) -> syn::ItemEnum {
        syn::parse_str(src).unwrap_or_else(|err| panic!("failed to parse enum `{src}`: {err}"))
    }

    /// Drive a positive-case emission test from `tests/fixtures/<scenario>.{rs,ts}`.
    ///
    /// The `.rs` file holds exactly one `pub struct` or `pub enum` declaration
    /// (no `use`, no `mod`, no surrounding code). The `.ts` file holds the
    /// expected emitted TypeScript output, IDE-readable (no YAML wrapping).
    ///
    /// Both sides are `trim`-compared so a single trailing newline in the `.ts`
    /// fixture (the canonical POSIX-friendly form) doesn't trip the assert.
    ///
    /// Setting `UPDATE_TS_FIXTURES=1` regenerates the `.ts` files in place
    /// (with a single trailing newline) and skips the assertion. Run twice in
    /// a row and the working tree should stay clean.
    fn assert_fixture_matches(scenario: &str) {
        let manifest = env!("CARGO_MANIFEST_DIR");
        let rs_path = format!("{manifest}/tests/fixtures/{scenario}.rs");
        let ts_path = format!("{manifest}/tests/fixtures/{scenario}.ts");

        let rs = std::fs::read_to_string(&rs_path).unwrap_or_else(|e| panic!("read {rs_path}: {e}"));
        let parsed: syn::File = syn::parse_str(&rs).unwrap_or_else(|e| panic!("parse {rs_path}: {e}"));
        let item =
            parsed.items.into_iter().next().unwrap_or_else(|| panic!("fixture {scenario} has no top-level item"));

        let config = EmitConfig::default();
        let actual = match &item {
            syn::Item::Struct(s) => emit_struct(s, &config),
            syn::Item::Enum(e) => emit_enum(e, &config),
            _ => panic!("fixture {scenario} top-level item is not a struct or enum"),
        }
        .unwrap_or_else(|e| panic!("emit failed for {scenario}: {e}"));

        if std::env::var("UPDATE_TS_FIXTURES").is_ok() {
            // Canonical form on disk: trimmed body plus a single trailing newline.
            let canonical = format!("{}\n", actual.trim_end());
            std::fs::write(&ts_path, &canonical).unwrap_or_else(|e| panic!("write {ts_path}: {e}"));
            return;
        }

        let expected = std::fs::read_to_string(&ts_path).unwrap_or_default();
        assert_eq!(
            actual.trim(),
            expected.trim(),
            "fixture {scenario} mismatch (run with UPDATE_TS_FIXTURES=1 to refresh)"
        );
    }

    #[test]
    fn struct_named_fields_emit_export_type() {
        assert_fixture_matches("struct_named_fields_emit_export_type");
    }

    #[test]
    fn struct_with_all_primitive_field_types() {
        assert_fixture_matches("struct_with_all_primitive_field_types");
    }

    #[test]
    fn struct_field_ref_str() {
        assert_fixture_matches("struct_field_ref_str");
    }

    #[test]
    fn struct_field_containers() {
        assert_fixture_matches("struct_field_containers");
    }

    #[test]
    fn struct_field_smart_pointer_box_transparent() {
        assert_fixture_matches("struct_field_smart_pointer_box_transparent");
    }

    #[test]
    fn struct_field_unknown_ident_falls_through() {
        assert_fixture_matches("struct_field_unknown_ident_falls_through");
    }

    #[test]
    fn struct_empty_named_fields() {
        assert_fixture_matches("struct_empty_named_fields");
    }

    #[test]
    fn struct_tuple_is_rejected() {
        let config = EmitConfig::default();
        let item = struct_item("pub struct NewType(pub u32);");
        match emit_struct(&item, &config).expect_err("tuple struct should fail") {
            EmitError::UnsupportedShape { reason, .. } => {
                assert!(reason.contains("tuple"), "reason was: {reason}");
            }
            other => panic!("expected UnsupportedShape, got {other:?}"),
        }
    }

    #[test]
    fn struct_unit_is_rejected() {
        let config = EmitConfig::default();
        let item = struct_item("pub struct Marker;");
        match emit_struct(&item, &config).expect_err("unit struct should fail") {
            EmitError::UnsupportedShape { reason, .. } => {
                assert!(reason.contains("unit"), "reason was: {reason}");
            }
            other => panic!("expected UnsupportedShape, got {other:?}"),
        }
    }

    #[test]
    fn struct_error_propagates_from_field_emission() {
        // A `Mutex<u32>` field should cause emit_struct to surface the
        // UnsupportedShape error.
        let config = EmitConfig::default();
        let item = struct_item(
            "pub struct Bad {
                pub locked: std::sync::Mutex<u32>,
            }",
        );
        let err = emit_struct(&item, &config).expect_err("Mutex field should fail");
        assert!(matches!(err, EmitError::UnsupportedShape { .. }));
    }

    // ── Enum emission ──────────────────────────────────────────────────

    #[test]
    fn enum_c_style_emits_string_literal_union() {
        assert_fixture_matches("enum_c_style_emits_string_literal_union");
    }

    #[test]
    fn enum_c_style_quote_style_single_default() {
        // AC-1 (default arm): the existing single-quoted shape is preserved
        // under `EmitConfig::default()` with the `#[serde(rename_all)]`
        // transform applied.
        let config = EmitConfig::default();
        assert_eq!(config.quote_style, QuoteStyle::Single);
        let item = enum_item(
            "#[serde(rename_all = \"lowercase\")]
            pub enum Letter {
                A,
                B,
            }",
        );
        let ts = emit_enum(&item, &config).expect("emit ok");
        assert_eq!(ts, "export type Letter = 'a' | 'b';");
    }

    #[test]
    fn enum_c_style_quote_style_double() {
        // AC-1 (opt-in arm): flipping `quote_style` to `Double` swaps
        // delimiters without touching anything else.
        let config = EmitConfig { quote_style: QuoteStyle::Double, ..EmitConfig::default() };
        let item = enum_item(
            "#[serde(rename_all = \"lowercase\")]
            pub enum Letter {
                A,
                B,
            }",
        );
        let ts = emit_enum(&item, &config).expect("emit ok");
        assert_eq!(ts, "export type Letter = \"a\" | \"b\";");
    }

    #[test]
    fn enum_tuple_zero_arg_variant_respects_quote_style() {
        // The `Fields::Unnamed` 0-arg arm goes through the same `quote()`
        // helper as the `Fields::Unit` arm — assert both delimiter branches
        // there too. `Foo()` (tuple with no inner types) is unusual but
        // syntactically valid and the emitter renders it as a string
        // literal of the variant name.
        let single = EmitConfig::default();
        let item = enum_item(
            "pub enum E {
                Foo(),
            }",
        );
        let ts = emit_enum(&item, &single).expect("emit ok");
        assert_eq!(ts, "export type E = 'Foo';");

        let double = EmitConfig { quote_style: QuoteStyle::Double, ..EmitConfig::default() };
        let ts = emit_enum(&item, &double).expect("emit ok");
        assert_eq!(ts, "export type E = \"Foo\";");
    }

    #[test]
    fn enum_single_variant_c_style() {
        assert_fixture_matches("enum_single_variant_c_style");
    }

    #[test]
    fn enum_empty_emits_never() {
        assert_fixture_matches("enum_empty_emits_never");
    }

    #[test]
    fn enum_tuple_variant_externally_tagged() {
        // Default serde emission for a tuple-payload variant is the
        // externally-tagged shape: `{"V": payload}`.
        assert_fixture_matches("enum_tuple_variant_externally_tagged");
    }

    #[test]
    fn enum_struct_variant_externally_tagged() {
        assert_fixture_matches("enum_struct_variant_externally_tagged");
    }

    #[test]
    fn enum_tuple_variant_with_primitive_payload() {
        assert_fixture_matches("enum_tuple_variant_with_primitive_payload");
    }

    #[test]
    fn enum_multi_field_tuple_variant_is_rejected() {
        let config = EmitConfig::default();
        let item = enum_item(
            "pub enum Bad {
                Two(u32, u32),
            }",
        );
        let err = emit_enum(&item, &config).expect_err("multi-tuple variant should fail");
        match err {
            EmitError::UnsupportedShape { reason, .. } => {
                assert!(reason.contains("tuple"), "reason was: {reason}");
            }
            other => panic!("expected UnsupportedShape, got {other:?}"),
        }
    }

    #[test]
    fn enum_error_propagates_from_variant_emission() {
        let config = EmitConfig::default();
        let item = enum_item(
            "pub enum Bad {
                Locked(Mutex<u32>),
            }",
        );
        let err = emit_enum(&item, &config).expect_err("Mutex variant payload should fail");
        assert!(matches!(err, EmitError::UnsupportedShape { .. }));
    }

    // ── Serde renames (PR 2) ───────────────────────────────────────────

    #[test]
    fn struct_rename_all_camel_case() {
        assert_fixture_matches("struct_rename_all_camel_case");
    }

    #[test]
    fn struct_field_rename_wins_over_container() {
        assert_fixture_matches("struct_field_rename_wins_over_container");
    }

    #[test]
    fn struct_field_serde_skip_drops_field() {
        assert_fixture_matches("struct_field_serde_skip_drops_field");
    }

    #[test]
    fn struct_field_serde_default_optional() {
        // `#[serde(default)]` (bare and path form) renders TS-optional `?`;
        // composes with `Option<T>` for `field?: T | null`. A plain `Option<T>`
        // without default stays required (`field: T | null`).
        assert_fixture_matches("struct_field_serde_default_optional");
    }

    #[test]
    fn struct_field_rename_with_hyphen_quotes_key() {
        // `kebab-case` mode produces field names with `-`, which aren't valid
        // TS identifiers; they get quoted as object-literal keys.
        assert_fixture_matches("struct_field_rename_with_hyphen_quotes_key");
    }

    #[test]
    fn enum_rename_all_snake_case() {
        assert_fixture_matches("enum_rename_all_snake_case");
    }

    #[test]
    fn enum_variant_rename_wins_over_container() {
        assert_fixture_matches("enum_variant_rename_wins_over_container");
    }

    // ── Struct-variant field renaming (issue #133) ─────────────────────

    #[test]
    fn enum_rename_all_spares_variant_fields() {
        // The repro from issue #133: an enum's `rename_all` renames the
        // VARIANT (`toolCall`) and must leave the variant's field names
        // alone, because serde does.
        assert_fixture_matches("enum_rename_all_spares_variant_fields");
    }

    #[test]
    fn enum_rename_all_fields() {
        // `rename_all_fields` is the attribute that actually asks for the
        // renaming the emitter used to do unprompted.
        assert_fixture_matches("enum_rename_all_fields");
    }

    #[test]
    fn enum_variant_rename_all_wins_over_container() {
        // A variant's own `rename_all` governs that variant's fields and
        // overrides the container's `rename_all_fields`; sibling variants
        // still follow the container.
        assert_fixture_matches("enum_variant_rename_all_wins_over_container");
    }

    #[test]
    fn enum_variant_rename_all_does_not_touch_the_variant_key() {
        // Easy to get backwards: `rename_all` on a VARIANT renames that
        // variant's fields, not the variant's own wire name. The key here
        // comes from the container's `rename_all` alone.
        let config = EmitConfig::default();
        let item = enum_item(
            r#"
            #[serde(rename_all = "camelCase")]
            pub enum Event {
                #[serde(rename_all = "UPPERCASE")]
                ToolCall { prompt_template: String },
            }
            "#,
        );
        let ts = emit_enum(&item, &config).expect("emit ok");
        assert_eq!(ts, "export type Event = { toolCall: { PROMPT_TEMPLATE: string } };");
    }

    #[test]
    fn config_case_default_does_not_reach_variant_fields() {
        // `case_default` stands in for "this crate annotates with
        // rename_all". Such a crate still gets verbatim struct-variant field
        // names out of serde, so applying it here would recreate #133 from
        // the config side. It must still rename the variant key.
        let config = EmitConfig { case_default: Some(crate::types::RenameAll::CamelCase), ..Default::default() };
        let item = enum_item(
            "pub enum Event {
                ToolCall { prompt_template: String },
            }",
        );
        let ts = emit_enum(&item, &config).expect("emit ok");
        assert_eq!(ts, "export type Event = { toolCall: { prompt_template: string } };");
    }

    #[test]
    fn enum_field_rename_wins_over_every_rename_all() {
        // Closest scope of all: an explicit field rename.
        let config = EmitConfig::default();
        let item = enum_item(
            r#"
            #[serde(rename_all_fields = "camelCase")]
            pub enum Event {
                #[serde(rename_all = "UPPERCASE")]
                ToolCall {
                    #[serde(rename = "tmpl")]
                    prompt_template: String,
                },
            }
            "#,
        );
        let ts = emit_enum(&item, &config).expect("emit ok");
        assert!(ts.contains("tmpl: string"), "ts was: {ts}");
    }

    #[test]
    fn struct_rename_all_fields_is_inert() {
        // `rename_all_fields` is an enum attribute. A struct's own fields are
        // governed by its `rename_all`, and the emitter must not let the
        // enum-only attr bleed into the struct path.
        let config = EmitConfig::default();
        let item = struct_item(
            r#"
            #[serde(rename_all_fields = "camelCase")]
            pub struct Foo {
                pub prompt_template: String,
            }
            "#,
        );
        let ts = emit_struct(&item, &config).expect("emit ok");
        assert!(ts.contains("prompt_template: string"), "ts was: {ts}");
    }

    #[test]
    fn enum_rename_all_fields_rejects_unknown_mode() {
        // The error must name `rename_all_fields`, not `rename_all`, or the
        // build log points at the wrong attribute.
        let config = EmitConfig::default();
        let item = enum_item(
            r#"
            #[serde(rename_all_fields = "Train-Case")]
            pub enum Event {
                ToolCall { prompt_template: String },
            }
            "#,
        );
        match emit_enum(&item, &config).expect_err("unknown mode should fail") {
            EmitError::UnsupportedSerdeAttr { attr, .. } => {
                assert!(attr.contains("rename_all_fields"), "attr was: {attr}");
                assert!(attr.contains("Train-Case"), "attr was: {attr}");
            }
            other => panic!("expected UnsupportedSerdeAttr, got {other:?}"),
        }
    }

    // ── Container-level serde(default) ─────────────────────────────────

    #[test]
    fn struct_container_default_optional() {
        // `#[serde(default)]` on the struct makes every field absent-able on
        // the wire, so every field is TS-optional. Emitting them as required
        // forced callers to spell out a value for each one to satisfy `tsc`,
        // even though serde accepts `{}`.
        assert_fixture_matches("struct_container_default_optional");
    }

    #[test]
    fn struct_container_default_composes_with_field_default() {
        // Both scopes say the same thing; the field must not end up with two
        // `?` markers.
        let config = EmitConfig::default();
        let item = struct_item(
            r#"
            #[serde(default)]
            pub struct Settings {
                #[serde(default)]
                pub retries: u32,
            }
            "#,
        );
        let ts = emit_struct(&item, &config).expect("emit ok");
        assert_eq!(ts, "export type Settings = {\n  retries?: number;\n};");
    }

    #[test]
    fn struct_container_default_path_form_is_equivalent() {
        let config = EmitConfig::default();
        let item = struct_item(
            r#"
            #[serde(default = "defaults::settings")]
            pub struct Settings {
                pub retries: u32,
            }
            "#,
        );
        let ts = emit_struct(&item, &config).expect("emit ok");
        assert!(ts.contains("retries?: number"), "ts was: {ts}");
    }

    #[test]
    fn struct_container_default_still_drops_skipped_fields() {
        // `skip` wins: the field isn't on the wire at all, optional or not.
        let config = EmitConfig::default();
        let item = struct_item(
            r#"
            #[serde(default)]
            pub struct Settings {
                pub retries: u32,
                #[serde(skip)]
                pub cached: u32,
            }
            "#,
        );
        let ts = emit_struct(&item, &config).expect("emit ok");
        assert!(ts.contains("retries?: number"), "ts was: {ts}");
        assert!(!ts.contains("cached"), "ts was: {ts}");
    }

    #[test]
    fn struct_container_default_rejects_a_flattened_field() {
        // Same absent-or-present problem as `#[serde(flatten, default)]`, just
        // inherited from the container instead of written on the field.
        let config = EmitConfig::default();
        let item = struct_item(
            r#"
            #[serde(default)]
            pub struct Step {
                #[serde(flatten)]
                pub meta: StepMeta,
            }
            "#,
        );
        match emit_struct(&item, &config).expect_err("container default + flatten should be rejected") {
            EmitError::UnsupportedShape { reason, .. } => {
                assert!(reason.contains("absent-or-present"), "reason was: {reason}");
                assert!(reason.contains("container"), "reason was: {reason}");
            }
            other => panic!("expected UnsupportedShape, got {other:?}"),
        }
    }

    #[test]
    fn enum_container_default_does_not_reach_variant_fields() {
        // Serde rejects `#[serde(default)]` on an enum, so it must not leak
        // into struct-variant bodies via the shared container extractor.
        let config = EmitConfig::default();
        let item = enum_item(
            r#"
            #[serde(default)]
            pub enum Event {
                Move { x: u32 },
            }
            "#,
        );
        let ts = emit_enum(&item, &config).expect("emit ok");
        assert_eq!(ts, "export type Event = { Move: { x: number } };");
    }

    #[test]
    fn struct_without_container_default_keeps_fields_required() {
        // The no-attribute path must be untouched.
        let config = EmitConfig::default();
        let item = struct_item(
            "pub struct Settings {
                pub retries: u32,
                pub notes: Option<String>,
            }",
        );
        let ts = emit_struct(&item, &config).expect("emit ok");
        assert_eq!(ts, "export type Settings = {\n  retries: number;\n  notes: string | null;\n};");
    }

    // ── serde(flatten) → TS intersection ───────────────────────────────

    #[test]
    fn struct_field_flatten_intersection() {
        // The repro from issue #132: the flattened field's keys land in the
        // parent object on the wire, so TS gets an intersection, not a
        // nested `meta` property.
        assert_fixture_matches("struct_field_flatten_intersection");
    }

    #[test]
    fn struct_field_flatten_only() {
        // Every field flattened — the empty property object drops out
        // rather than emitting a pointless `& {}`.
        assert_fixture_matches("struct_field_flatten_only");
    }

    #[test]
    fn struct_field_flatten_catch_all_map() {
        // Serde's catch-all idiom. A string-keyed map is object-shaped, so
        // it intersects cleanly.
        assert_fixture_matches("struct_field_flatten_catch_all_map");
    }

    #[test]
    fn enum_struct_variant_flatten() {
        // Struct variants share the named-field collector, so flatten works
        // inside a variant payload too.
        assert_fixture_matches("enum_struct_variant_flatten");
    }

    #[test]
    fn struct_field_flatten_peels_smart_pointers() {
        // `Box<T>` is transparent to serde, so it flattens like `T`.
        let config = EmitConfig::default();
        let item = struct_item(
            "pub struct Step {
                #[serde(flatten)]
                pub meta: Box<StepMeta>,
                pub program: String,
            }",
        );
        let ts = emit_struct(&item, &config).expect("boxed flatten should emit");
        assert!(ts.starts_with("export type Step = StepMeta & {"), "ts was: {ts}");
    }

    #[test]
    fn struct_field_flatten_respects_rename_all_on_siblings() {
        // The flattened field contributes no key of its own, so rename_all
        // applies to the surviving properties only.
        let config = EmitConfig::default();
        let item = struct_item(
            r#"
            #[serde(rename_all = "camelCase")]
            pub struct Step {
                #[serde(flatten)]
                pub meta: StepMeta,
                pub program_name: String,
            }
            "#,
        );
        let ts = emit_struct(&item, &config).expect("emit ok");
        assert!(ts.contains("StepMeta & {"), "ts was: {ts}");
        assert!(ts.contains("programName: string"), "ts was: {ts}");
    }

    /// Assert a flattened field of type `field_ty` is rejected, and that the
    /// reason mentions `needle`.
    fn assert_flatten_rejected(field_ty: &str, needle: &str) {
        let config = EmitConfig::default();
        let item = struct_item(&format!(
            "pub struct Holder {{
                #[serde(flatten)]
                pub inner: {field_ty},
                pub tail: u32,
            }}"
        ));
        let Err(err) = emit_struct(&item, &config) else {
            panic!("flatten of `{field_ty}` should have been rejected");
        };
        match err {
            EmitError::UnsupportedShape { reason, .. } => {
                assert!(reason.contains(needle), "flatten of `{field_ty}` — reason was: {reason}");
            }
            other => panic!("expected UnsupportedShape for `{field_ty}`, got {other:?}"),
        }
    }

    #[test]
    fn struct_field_flatten_rejects_option() {
        // Serde makes the whole group absent-or-present; an intersection
        // can't say that, and `Partial<T>` would admit any subset.
        assert_flatten_rejected("Option<StepMeta>", "absent-or-present");
        // The same is true through a smart pointer.
        assert_flatten_rejected("Box<Option<StepMeta>>", "absent-or-present");
    }

    #[test]
    fn struct_field_flatten_rejects_non_object_renderings() {
        // `X & string` collapses to `never`; `X & unknown` is a silent
        // no-op; `X & T[]` describes nothing on the wire. All hard errors.
        assert_flatten_rejected("String", "renders as `string`");
        assert_flatten_rejected("u32", "renders as `number`");
        assert_flatten_rejected("Vec<StepMeta>", "renders as `StepMeta[]`");
        assert_flatten_rejected("serde_json::Value", "renders as `unknown`");
    }

    #[test]
    fn struct_field_flatten_rejects_default_combination() {
        let config = EmitConfig::default();
        let item = struct_item(
            "pub struct Holder {
                #[serde(flatten, default)]
                pub inner: StepMeta,
            }",
        );
        match emit_struct(&item, &config).expect_err("flatten + default should be rejected") {
            EmitError::UnsupportedShape { reason, .. } => {
                assert!(reason.contains("flatten, default"), "reason was: {reason}");
            }
            other => panic!("expected UnsupportedShape, got {other:?}"),
        }
    }

    #[test]
    fn struct_field_flatten_and_skip_leave_only_flatten() {
        // A skipped field never reaches the wire; a flattened one has no key
        // of its own. Together they leave a bare intersection.
        let config = EmitConfig::default();
        let item = struct_item(
            "pub struct Holder {
                #[serde(flatten)]
                pub inner: StepMeta,
                #[serde(skip)]
                pub cached: u32,
            }",
        );
        let ts = emit_struct(&item, &config).expect("emit ok");
        assert_eq!(ts, "export type Holder = StepMeta;");
    }

    #[test]
    fn struct_rejects_split_rename_on_field() {
        let config = EmitConfig::default();
        let item = struct_item(
            r#"pub struct Foo {
                #[serde(rename(serialize = "wireName", deserialize = "WIRE_NAME"))]
                pub a: u32,
            }"#,
        );
        let err = emit_struct(&item, &config).expect_err("split-rename should fail");
        match err {
            EmitError::UnsupportedSerdeAttr { attr, .. } => {
                assert!(attr.contains("split-rename"), "attr was: {attr}");
            }
            other => panic!("expected UnsupportedSerdeAttr, got {other:?}"),
        }
    }

    #[test]
    fn enum_rejects_tag_attr_on_container() {
        let config = EmitConfig::default();
        let item = enum_item(
            r#"
            #[serde(tag = "type")]
            pub enum Msg {
                Click,
                Hover,
            }
            "#,
        );
        let err = emit_enum(&item, &config).expect_err("tag-attr should fail");
        match err {
            EmitError::UnsupportedSerdeAttr { attr, .. } => {
                assert!(attr.contains("tag"), "attr was: {attr}");
            }
            other => panic!("expected UnsupportedSerdeAttr, got {other:?}"),
        }
    }

    #[test]
    fn config_case_default_applies_when_container_has_no_rename_all() {
        // If EmitConfig::case_default is set and the container has no
        // rename_all, fields get the config-level transform.
        let config = EmitConfig { case_default: Some(crate::types::RenameAll::CamelCase), ..Default::default() };
        let item = struct_item(
            "pub struct Foo {
                pub user_name: String,
                pub age_years: u32,
            }",
        );
        let ts = emit_struct(&item, &config).unwrap();
        assert!(ts.contains("userName: string"), "ts was: {ts}");
        assert!(ts.contains("ageYears: number"), "ts was: {ts}");
    }

    #[test]
    fn container_rename_all_wins_over_config_case_default() {
        // The container's explicit rename_all overrides the config-level
        // default — closer-scope wins.
        let config = EmitConfig { case_default: Some(crate::types::RenameAll::CamelCase), ..Default::default() };
        let item = struct_item(
            r#"
            #[serde(rename_all = "snake_case")]
            pub struct Foo {
                pub user_name: String,
            }
            "#,
        );
        let ts = emit_struct(&item, &config).unwrap();
        // user_name is already snake_case, so the container's mode is a no-op
        // and we get `user_name`, NOT camelCased `userName`.
        assert!(ts.contains("user_name: string"), "ts was: {ts}");
        assert!(!ts.contains("userName"), "ts was: {ts}");
    }
}
