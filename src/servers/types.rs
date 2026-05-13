//! Type normalization, transformation, and import collection utilities.

use std::collections::{HashMap, HashSet};

use syn::{GenericArgument, PathArguments, Type, TypeParamBound};

use crate::servers::parse::ApiModule;

/// Normalize a syn Type to a clean string (no extra spaces around ::, <, >).
pub fn norm_type(ty: &Type) -> String {
    normalize_spaces(&norm_tokens(ty))
}

/// Quote a `ToTokens` value and normalize its string representation.
pub fn norm_tokens<T: quote::ToTokens>(t: &T) -> String {
    let tokens = quote::quote!(#t);
    normalize_spaces(&tokens.to_string())
}

/// Remove extra spaces that syn/quote inserts around ::, <, >, &.
pub fn normalize_spaces(s: &str) -> String {
    s.replace(" :: ", "::")
        .replace(":: ", "::")
        .replace(" ::", "::")
        .replace(" <", "<")
        .replace("< ", "<")
        .replace(" >", ">")
        .replace("> ", ">")
        .replace("& ", "&")
}

/// Capitalize the first character of a string.
pub fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
    }
}

/// Strip the leading `&` from a type string.
pub fn strip_ref(ty: &str) -> String {
    let t = ty.trim();
    if let Some(rest) = t.strip_prefix('&') { rest.trim().to_string() } else { t.to_string() }
}

/// Extract the input type name from a param type like `CreateNodeInput` or `&CreateNodeInput`.
pub fn extract_input_type(ty: &str) -> String {
    strip_ref(ty)
}

/// For a return type like `Vec<GraphNode>`, extract the inner type `GraphNode`.
/// For `GraphNode`, return `GraphNode`. For `()`, return `()`.
pub fn inner_type(ty: &str) -> String {
    if ty.starts_with("Vec<") && ty.ends_with('>') { ty[4..ty.len() - 1].to_string() } else { ty.to_string() }
}

/// Wrappers we peel through to find the underlying types that need importing.
///
/// Single-arg wrappers (`Option<T>`, `Vec<T>`, etc.) and multi-arg containers
/// (`HashMap<K, V>`, `Result<T, E>`) are *both* in this set. The recursive
/// walker treats them uniformly: it never imports the head, but always recurses
/// into every generic argument.
const KNOWN_CONTAINERS: &[&str] = &[
    "Option", "Vec", "Box", "Arc", "Rc", "Cow", "Result", "HashMap", "BTreeMap", "HashSet", "BTreeSet", "IndexMap",
    "IndexSet",
];

/// Names that should never be added to the import list — primitives, prelude
/// scalars, and a few path types that occasionally appear in return positions.
fn is_prelude_scalar(name: &str) -> bool {
    matches!(
        name,
        "bool"
            | "char"
            | "i8"
            | "i16"
            | "i32"
            | "i64"
            | "i128"
            | "isize"
            | "u8"
            | "u16"
            | "u32"
            | "u64"
            | "u128"
            | "usize"
            | "f32"
            | "f64"
            | "String"
            | "str"
            | "PathBuf"
            | "Path"
    )
}

/// Collect the simple type names that need importing from the types module.
///
/// Walks the `syn::Type` AST recursively. Skips:
/// - prelude/primitive types (`String`, `i64`, `bool`, …),
/// - qualified paths (`crate::schema::Foo`, `relation::Model`) — those are
///   handled by the entity-import path elsewhere,
/// - known container heads (`Option`, `Vec`, `HashMap`, …) — recurses into
///   their generic args instead,
/// - `dyn Trait` and `impl Trait`.
///
/// Unknown generic heads (e.g., user-defined `MyContainer<T>`) still have their
/// args walked defensively, but the head itself is not imported — that case is
/// rare in service-fn returns and the head usually points at a std container we
/// don't know about yet.
pub fn collect_type_import(ty: &Type, imports: &mut Vec<String>) {
    match ty {
        // References: &T, &mut T — peel and recurse.
        Type::Reference(r) => collect_type_import(&r.elem, imports),

        // Tuples: recurse into each element. The unit type `()` is a tuple
        // with no elements and naturally produces no imports.
        Type::Tuple(t) => {
            for elem in &t.elems {
                collect_type_import(elem, imports);
            }
        }

        // Path types: the interesting case.
        Type::Path(tp) => {
            // Qualified paths like `crate::schema::Foo` or `relation::Model`
            // are handled by the entity-import path elsewhere.
            if tp.qself.is_some() || tp.path.segments.len() > 1 {
                // Still recurse into the *last* segment's generic args, in
                // case a user wrote `crate::schema::Vec<MyType>` (unusual but
                // harmless to handle).
                if let Some(last) = tp.path.segments.last() {
                    walk_path_args(&last.arguments, imports);
                }
                return;
            }

            let Some(seg) = tp.path.segments.last() else { return };
            let name = seg.ident.to_string();

            if KNOWN_CONTAINERS.contains(&name.as_str()) {
                walk_path_args(&seg.arguments, imports);
                return;
            }

            // Unknown generic head: still recurse into its args so we don't
            // miss imports buried inside an unfamiliar wrapper.
            if !matches!(seg.arguments, PathArguments::None) {
                walk_path_args(&seg.arguments, imports);
                return;
            }

            if is_prelude_scalar(&name) {
                return;
            }

            if !imports.contains(&name) {
                imports.push(name);
            }
        }

        // `dyn Trait` / `impl Trait` — skip entirely, but walk into bounds if
        // any of them are concrete trait objects with generic args. For safety
        // we simply do nothing — these shapes essentially never appear in DTO
        // return positions.
        Type::TraitObject(to) => {
            // Defensive: a bound like `dyn AsRef<MyType>` would still benefit
            // from walking the args.
            for bound in &to.bounds {
                if let TypeParamBound::Trait(t) = bound
                    && let Some(last) = t.path.segments.last()
                {
                    walk_path_args(&last.arguments, imports);
                }
            }
        }
        Type::ImplTrait(it) => {
            for bound in &it.bounds {
                if let TypeParamBound::Trait(t) = bound
                    && let Some(last) = t.path.segments.last()
                {
                    walk_path_args(&last.arguments, imports);
                }
            }
        }

        // Groups / parens just wrap an inner type.
        Type::Group(g) => collect_type_import(&g.elem, imports),
        Type::Paren(p) => collect_type_import(&p.elem, imports),

        // Arrays and slices: recurse into the element type.
        Type::Array(a) => collect_type_import(&a.elem, imports),
        Type::Slice(s) => collect_type_import(&s.elem, imports),
        Type::Ptr(p) => collect_type_import(&p.elem, imports),

        // BareFn, Infer, Macro, Never, TraitObject, Verbatim, etc. — nothing
        // useful to import.
        _ => {}
    }
}

/// Recurse into the generic arguments of a path segment, calling
/// `collect_type_import` on each type argument.
fn walk_path_args(args: &PathArguments, imports: &mut Vec<String>) {
    if let PathArguments::AngleBracketed(ab) = args {
        for arg in &ab.args {
            if let GenericArgument::Type(t) = arg {
                collect_type_import(t, imports);
            }
        }
    }
}

/// Convert a function name like `graph_updated` to an event name like `graph-updated`.
pub fn event_name(fn_name: &str) -> String {
    fn_name.replace('_', "-")
}

/// Convert a snake_case string to PascalCase.
pub fn to_pascal_case(s: &str) -> String {
    s.split('_').map(capitalize).collect::<String>()
}

/// Convert a service-fn parameter type to the owned form used in the generated
/// handler struct / handler-arg declaration.
///
/// Rules, mirroring [`forward_arg_expr`]'s `.as_deref()` allowlist
/// ([`owned_form_derefs_to`]):
///
/// - `&str` → `String`, `&[T]` → `Vec<T>`, `&Path` → `PathBuf`,
///   `&CStr` → `CString`, `&OsStr` → `OsString` — the five DST refs that the
///   forwarding side handles via `.as_deref()` get their sized companions here.
/// - `&T` (any other) → `T` — sized targets just lose the ref.
/// - `Option<U>` → `Option<owned(U)>` (recurse).
/// - everything else → rendered as-is.
///
/// The two sides MUST stay in lockstep: any `&T` that gets `.as_deref()` on the
/// forwarding side must have a sensible owned companion here, otherwise the
/// generated handler fails to compile (the forwarding call expects an
/// `Option<OwnedCompanion>` to deref from).
pub fn param_to_owned_type(ty: &Type) -> String {
    match ty {
        Type::Reference(r) => owned_form_of_ref_target(&r.elem),
        Type::Path(tp) if last_segment_is(tp, "Option") => match option_inner(tp) {
            Some(inner) => format!("Option<{}>", param_to_owned_type(inner)),
            None => norm_type(ty),
        },
        Type::Group(g) => param_to_owned_type(&g.elem),
        Type::Paren(p) => param_to_owned_type(&p.elem),
        _ => norm_type(ty),
    }
}

/// Map the target type of a reference (`&T`) to its owned form.
fn owned_form_of_ref_target(target: &Type) -> String {
    match target {
        Type::Slice(s) => format!("Vec<{}>", norm_type(&s.elem)),
        Type::Group(g) => owned_form_of_ref_target(&g.elem),
        Type::Paren(p) => owned_form_of_ref_target(&p.elem),
        Type::Path(tp) => match dst_owned_companion(tp) {
            Some(owned) => owned.to_string(),
            None => norm_type(target),
        },
        _ => norm_type(target),
    }
}

/// If `tp` is a single-segment path naming one of the recognized unsized DSTs,
/// return its sized owned companion. Otherwise None.
fn dst_owned_companion(tp: &syn::TypePath) -> Option<&'static str> {
    if tp.qself.is_some() || tp.path.segments.len() > 1 {
        return None;
    }
    let seg = tp.path.segments.last()?;
    if !matches!(seg.arguments, PathArguments::None) {
        return None;
    }
    match seg.ident.to_string().as_str() {
        "str" => Some("String"),
        "Path" => Some("PathBuf"),
        "CStr" => Some("CString"),
        "OsStr" => Some("OsString"),
        _ => None,
    }
}

/// Generate the forwarding expression for a handler parameter.
///
/// The handler holds an owned value (of `param_to_owned_type(user_ty)`). This
/// function produces the expression that converts that owned value back into
/// what the user-written service-fn signature wants:
///
/// - `&T` → `&name` (Rust deref-coerces `&String → &str`, `&Vec<T> → &[T]`).
/// - `Option<&str>` / `Option<&[T]>` / `Option<&Path>` / `Option<&CStr>` /
///   `Option<&OsStr>` → `name.as_deref()` (handler holds `Option<String>` /
///   `Option<Vec<T>>` / `Option<PathBuf>` / etc. whose inner `Deref` target is
///   the requested reference type).
/// - `Option<&UserType>` (anything else under an `Option<&T>`) → `name.as_ref()`,
///   producing `Option<&UserType>` without requiring `T: Deref`.
/// - `Option<T>` (owned `T`) → `name` (handler also holds `Option<T>`).
/// - everything else → `name` (handler owns it, user wants it owned).
///
/// This replaces the older substring heuristics on the rendered parameter type
/// (which broke `Option<u8>` by always emitting `.as_deref()`).
pub fn forward_arg_expr(name: &str, user_ty: &Type) -> String {
    match user_ty {
        // User wrote `&T` (or `&mut T`) — handler owns T → pass `&name`.
        // Rust's deref coercion handles `&String → &str`, `&Vec<T> → &[T]`,
        // `&PathBuf → &Path`, etc.
        Type::Reference(_) => format!("&{name}"),

        // User wrote `Option<...>` — inspect the inner type.
        Type::Path(tp) if last_segment_is(tp, "Option") => match option_inner(tp) {
            // User wrote `Option<&InnerRef>` where `InnerRef`'s owned form
            // has a `Deref` impl yielding `InnerRef` (str/[T]/Path/CStr/OsStr).
            // Handler holds `Option<OwnedInner>` → `.as_deref()` converts it.
            Some(Type::Reference(r)) if owned_form_derefs_to(&r.elem) => format!("{name}.as_deref()"),
            // User wrote `Option<&UserType>` (no Deref) — handler holds
            // `Option<UserType>`. `.as_ref()` produces `Option<&UserType>`.
            Some(Type::Reference(_)) => format!("{name}.as_ref()"),
            // User wrote `Option<T>` (owned inner) — handler also has
            // `Option<T>`. Pass by value.
            _ => name.to_string(),
        },

        // Everything else: user wants owned T, handler has owned T.
        _ => name.to_string(),
    }
}

/// Return true when the last path segment's ident matches `name`. We only ever
/// see single-segment `Option` paths in practice (the prelude one); this lets
/// us match it regardless of any leading `::std::option::` prefix the user
/// might have typed.
fn last_segment_is(tp: &syn::TypePath, name: &str) -> bool {
    tp.path.segments.last().is_some_and(|seg| seg.ident == name)
}

/// Extract the inner type `T` from an `Option<T>` path. Returns `None` if the
/// path isn't an `Option`-shaped path with a single type arg.
fn option_inner(tp: &syn::TypePath) -> Option<&Type> {
    let seg = tp.path.segments.last()?;
    let PathArguments::AngleBracketed(args) = &seg.arguments else { return None };
    for arg in &args.args {
        if let GenericArgument::Type(t) = arg {
            return Some(t);
        }
    }
    None
}

/// Return true when `T`'s owned form (as produced by `param_to_owned_type`)
/// has a `Deref` impl yielding `&T`. Drives the `Option<&T>` →
/// `name.as_deref()` rule.
///
/// Allowlist by leaf-identifier; deliberately small. Slices (`[T]`) are also
/// accepted via `Type::Slice` since `Vec<T>: Deref<Target = [T]>`.
fn owned_form_derefs_to(elem: &Type) -> bool {
    match elem {
        Type::Slice(_) => true,
        Type::Path(tp) => {
            let Some(seg) = tp.path.segments.last() else { return false };
            // Only single-segment paths qualify — a user-qualified path like
            // `my::Path` is not the prelude `Path` and we shouldn't assume it
            // has a Deref impl.
            if tp.qself.is_some() || tp.path.segments.len() > 1 {
                return false;
            }
            matches!(seg.ident.to_string().as_str(), "str" | "Path" | "CStr" | "OsStr")
        }
        _ => false,
    }
}

/// Convert snake_case to camelCase.
pub fn snake_to_camel(s: &str) -> String {
    let mut result = String::new();
    let mut capitalize_next = false;
    for c in s.chars() {
        if c == '_' {
            capitalize_next = true;
        } else if capitalize_next {
            result.push(c.to_ascii_uppercase());
            capitalize_next = false;
        } else {
            result.push(c);
        }
    }
    result
}

/// Map a Rust return type to a TypeScript type string.
pub fn rust_type_to_ts(ty: &str) -> String {
    let ty = ty.trim();
    if ty == "()" {
        return "null".to_string();
    }
    if ty == "String" || ty == "&str" || ty == "str" {
        return "string".to_string();
    }
    if ty == "i32" || ty == "i64" || ty == "u32" || ty == "u64" || ty == "f32" || ty == "f64" {
        return "number".to_string();
    }
    if ty == "bool" {
        return "boolean".to_string();
    }
    if let Some(rest) = ty.strip_prefix('&') {
        return rust_type_to_ts(rest.trim());
    }
    if ty.starts_with("Option<") && ty.ends_with('>') {
        let inner = &ty[7..ty.len() - 1];
        return format!("{} | null", rust_type_to_ts(inner));
    }
    if ty.starts_with("Vec<") && ty.ends_with('>') {
        let inner = &ty[4..ty.len() - 1];
        return format!("{}[]", rust_type_to_ts(inner));
    }
    // Entity-qualified types like `relation::Model` → `RelationModel`
    if ty.contains("::") {
        let parts: Vec<&str> = ty.split("::").collect();
        if parts.len() == 2 && parts[1] == "Model" {
            return format!("{}{}", capitalize(parts[0]), parts[1]);
        }
        return parts.last().unwrap_or(&ty).to_string();
    }
    ty.to_string()
}

/// Collect TS type imports (skip primitives and null).
pub fn collect_ts_import(ts_type: &str, imports: &mut Vec<String>) {
    if ts_type.contains(" | ") {
        for part in ts_type.split(" | ") {
            collect_ts_import(part.trim(), imports);
        }
        return;
    }
    let base = ts_type.trim_end_matches("[]");
    if base == "null" || base == "void" || base == "string" || base == "number" || base == "boolean" || base.is_empty()
    {
        return;
    }
    if !imports.contains(&base.to_string()) {
        imports.push(base.to_string());
    }
}

/// Naming configuration for modules - handles pluralization and URL singularization.
///
/// Uses `cruet` for Rails-style inflection (handles irregular words like
/// "dependencies" → "dependency"). Override maps take precedence over cruet.
#[derive(Debug, Clone, Default)]
pub struct NamingConfig {
    /// Overrides for module → plural form (e.g., "evidence" → "evidence").
    pub plural_overrides: HashMap<String, String>,
    /// Overrides for module → singular form (e.g., "work_sessions" → "session").
    pub singular_overrides: HashMap<String, String>,
    /// Overrides for module → human label (e.g., "work_session" → "Work Session").
    pub label_overrides: HashMap<String, String>,
    /// Overrides for module → human plural label (e.g., "evidence" → "Evidence").
    pub plural_label_overrides: HashMap<String, String>,
    /// Module names declared as singletons via build-config. Source-side
    /// `// ontogen:singleton` markers reach the same effect via
    /// [`ApiModule::is_singleton`](crate::servers::ApiModule); both inputs
    /// are OR'd together by `apply_singleton_overlay` before generators run.
    pub singleton_modules: HashSet<String>,
}

impl NamingConfig {
    /// Get the plural form of a module name.
    ///
    /// Checks `plural_overrides` first, then uses `cruet::to_plural`.
    pub fn module_plural(&self, module: &str) -> String {
        if let Some(override_val) = self.plural_overrides.get(module) {
            return override_val.clone();
        }
        cruet::to_plural(module)
    }

    /// Get the singular form of a module name.
    ///
    /// Checks `singular_overrides` first, then uses `cruet::to_singular`.
    pub fn url_singular(&self, module: &str) -> String {
        if let Some(override_val) = self.singular_overrides.get(module) {
            return override_val.clone();
        }
        cruet::to_singular(module)
    }

    /// Get the plural form for URL paths (kebab-case).
    /// e.g., "agents" → "agents", "skill_files" → "skill-files".
    pub fn url_plural(&self, module: &str) -> String {
        self.module_plural(module).replace('_', "-")
    }

    /// Compute the URL path segment for an [`ApiModule`].
    ///
    /// Singleton modules (`module.is_singleton == true`) return the singular
    /// kebab-case form of the module name — `database` stays `database`,
    /// `auto_start` becomes `auto-start`. Entity (collection) modules go
    /// through [`url_plural`](Self::url_plural).
    ///
    /// Takes `&ApiModule` rather than `&str` because the source-side singleton
    /// marker lives on the IR; the config-side overlay
    /// (`singleton_modules`) has already been merged into that bit by
    /// `apply_singleton_overlay` before generators run, so callers don't need
    /// to consult [`NamingConfig`] separately.
    pub fn url_for_module(&self, module: &ApiModule) -> String {
        if module.is_singleton { module.name.replace('_', "-") } else { self.url_plural(&module.name) }
    }

    /// Get the human-readable label for a module.
    pub fn label(&self, module: &str) -> String {
        if let Some(override_val) = self.label_overrides.get(module) {
            return override_val.clone();
        }
        capitalize(module)
    }

    /// Get the human-readable plural label for a module.
    pub fn plural_label(&self, module: &str) -> String {
        if let Some(override_val) = self.plural_label_overrides.get(module) {
            return override_val.clone();
        }
        let label = self.label(module);
        cruet::to_plural(&label)
    }

    /// Derive the URL action segment for a custom function.
    pub fn derive_action(&self, module: &str, fn_name: &str) -> String {
        let mut action = fn_name.to_string();

        if let Some(rest) = action.strip_prefix("get_") {
            action = rest.to_string();
        }

        let plural = self.module_plural(module);

        if action == module || action == plural {
            return String::new();
        }

        if let Some(rest) = action.strip_prefix(&format!("{}_", module)) {
            action = rest.to_string();
        }

        if let Some(rest) = action.strip_suffix(&format!("_{}", plural)) {
            action = rest.to_string();
        }

        if let Some(rest) = action.strip_suffix(&format!("_{}", module)) {
            action = rest.to_string();
        }

        action.replace('_', "-")
    }
}
