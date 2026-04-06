//! Type normalization, transformation, and import collection utilities.

use std::collections::HashMap;

use syn::Type;

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

/// Collect the simple type names that need importing from the types module.
/// Skips Vec wrappers, entity-qualified types, and built-in types.
pub fn collect_type_import(ty: &str, imports: &mut Vec<String>) {
    let inner = inner_type(ty);
    if inner == "()" || inner.is_empty() {
        return;
    }
    if inner.contains("::") {
        // Entity-qualified: e.g. relation::Model — handled via entity import
        return;
    }
    // Skip Rust primitives — they don't need importing.
    if matches!(
        inner.as_str(),
        "bool"
            | "i8"
            | "i16"
            | "i32"
            | "i64"
            | "i128"
            | "u8"
            | "u16"
            | "u32"
            | "u64"
            | "u128"
            | "f32"
            | "f64"
            | "String"
            | "str"
            | "&str"
    ) {
        return;
    }
    if !imports.contains(&inner) {
        imports.push(inner);
    }
}

/// Convert a function name like `graph_updated` to an event name like `graph-updated`.
pub fn event_name(fn_name: &str) -> String {
    fn_name.replace('_', "-")
}

/// Convert a snake_case string to PascalCase.
pub fn to_pascal_case(s: &str) -> String {
    s.split('_').map(|w| capitalize(w)).collect::<String>()
}

/// Convert a Rust param type to its owned form for struct fields.
pub fn param_to_owned_type(ty: &str) -> String {
    if ty.starts_with("Option<") {
        // Option<&str> → Option<String>
        let inner = &ty[7..ty.len() - 1];
        let owned_inner = param_to_owned_type(inner);
        format!("Option<{}>", owned_inner)
    } else if ty == "&str" || ty == "& str" {
        "String".to_string()
    } else {
        strip_ref(ty)
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

/// Naming configuration for modules — handles pluralization and URL singularization.
#[derive(Debug, Clone, Default)]
pub struct NamingConfig {
    /// When true, derive singular/plural from the module filename heuristically:
    /// names ending in 's' are treated as already plural (singular = strip trailing 's'),
    /// names not ending in 's' are treated as singular (plural = append 's').
    /// This matches the convention where `agents.rs` → singular "agent", plural "agents",
    /// but `skill.rs` → singular "skill", plural "skills".
    pub auto_pluralize: bool,
    /// Overrides for module → plural form (e.g., "evidence" → "evidence").
    pub plural_overrides: HashMap<String, String>,
    /// Overrides for module → URL singular form (e.g., "work_session" → "session").
    pub singular_overrides: HashMap<String, String>,
    /// Overrides for module → human label (e.g., "work_session" → "Work Session").
    pub label_overrides: HashMap<String, String>,
    /// Overrides for module → human plural label (e.g., "evidence" → "Evidence").
    pub plural_label_overrides: HashMap<String, String>,
}

impl NamingConfig {
    /// Get the plural form of a module name.
    pub fn module_plural(&self, module: &str) -> String {
        if let Some(override_val) = self.plural_overrides.get(module) {
            return override_val.clone();
        }
        if self.auto_pluralize {
            if module.ends_with('s') {
                // Already plural (e.g., "agents" → "agents")
                module.to_string()
            } else {
                // Singular, need to pluralize (e.g., "skill" → "skills")
                format!("{}s", module)
            }
        } else {
            format!("{}s", module)
        }
    }

    /// Get the URL singular form of a module name.
    pub fn url_singular(&self, module: &str) -> String {
        if let Some(override_val) = self.singular_overrides.get(module) {
            return override_val.clone();
        }
        if self.auto_pluralize && module.ends_with('s') {
            // Derive singular by stripping trailing 's' (e.g., "agents" → "agent")
            module.strip_suffix('s').unwrap_or(module).to_string()
        } else {
            module.to_string()
        }
    }

    /// Get the plural form for URL paths (kebab-case).
    /// e.g., "agents" → "agents", "skill_files" → "skill-files".
    pub fn url_plural(&self, module: &str) -> String {
        self.module_plural(module).replace('_', "-")
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
        format!("{}s", label)
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
