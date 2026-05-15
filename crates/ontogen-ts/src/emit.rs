//! Per-type and top-level emission entry points.
//!
//! PR 1 lands the per-type emission machinery (`emit_type`, `emit_struct`,
//! `emit_enum`) for the phase-1 supported subset. The top-level [`emit`]
//! entry point's body is a `todo!()` stub — PR 4 wires type collection,
//! validation, and ordering through it.

use std::collections::BTreeMap;

use syn::{GenericArgument, PathArguments, Type, TypeArray, TypePath as SynTypePath, TypeReference, TypeSlice};

use crate::types::{BigIntBehavior, EmitConfig, EmitError, TypePath};

/// Emit TypeScript source for `roots` and everything they transitively reach
/// in `type_pool`, honoring `config`.
///
/// PR 1 leaves the body as `todo!()` — the full composition (collection →
/// validation → ordering → emission → aggregation) lands in PR 4 (AC-8).
/// The signature is fixed today so downstream consumers (and the rest of
/// this PR series) compile against a stable shape.
pub fn emit(
    _roots: &[TypePath],
    _type_pool: &BTreeMap<TypePath, syn::Item>,
    _config: &EmitConfig,
) -> Result<String, Vec<EmitError>> {
    todo!("PR 4 implements the top-level emit composition (OF-015 AC-8)")
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

    // 6. Fall-through — render as the terminal ident. PR 3 replaces this
    // with a pool / external-types lookup. Multi-segment paths (`foo::Bar`)
    // collapse to their terminal ident here; PR 3's canonicalization will
    // make that lookup honest.
    let terminal =
        path.path.segments.last().map(|s| s.ident.to_string()).ok_or_else(|| EmitError::UnsupportedShape {
            type_path: referenced_by.clone(),
            reason: "type path had no segments".to_string(),
        })?;
    Ok(terminal)
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
        ("Vec", [inner]) => Some(Container::Vec(inner)),
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
        "String" | "str" => Some("string"),
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
        let mut config = EmitConfig::default();
        config.bigint_behavior = BigIntBehavior::BigInt;
        let rendered = emit_type(&ty("u64"), &config, &tp("Test")).unwrap();
        assert_eq!(rendered, "bigint");

        config.bigint_behavior = BigIntBehavior::String;
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
}
