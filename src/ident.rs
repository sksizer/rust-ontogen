//! Rust identifiers for names derived from a consumer's schema.
//!
//! A snake-cased entity name or a relationship name can be a Rust keyword
//! (`Match` → `match`, `Type` → `type`). Every place the generator writes
//! such a name as Rust code (a module, a binding, a field) goes through
//! [`rust_ident`]; file names, wire keys and string literals use the bare
//! name.

/// Strict and reserved keywords of every edition up to 2024, all of which
/// can be written as raw identifiers.
const KEYWORDS: &[&str] = &[
    "abstract", "as", "async", "await", "become", "box", "break", "const", "continue", "do", "dyn", "else", "enum",
    "extern", "false", "final", "fn", "for", "gen", "if", "impl", "in", "let", "loop", "macro", "match", "mod", "move",
    "mut", "override", "priv", "pub", "ref", "return", "static", "struct", "trait", "true", "try", "type", "typeof",
    "unsafe", "unsized", "use", "virtual", "where", "while", "yield",
];

/// Keywords that cannot be raw identifiers: a schema name that snake-cases
/// to one of these has no Rust spelling and must be refused.
pub(crate) const UNRAWABLE: &[&str] = &["crate", "self", "super", "Self"];

/// `name` as Rust code: a keyword gets its `r#` prefix, anything else
/// (including a name already written `r#…`) is returned unchanged.
pub(crate) fn rust_ident(name: &str) -> String {
    if KEYWORDS.contains(&name) { format!("r#{name}") } else { name.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keywords_are_raw_and_other_names_unchanged() {
        assert_eq!(rust_ident("match"), "r#match");
        assert_eq!(rust_ident("type"), "r#type");
        assert_eq!(rust_ident("gen"), "r#gen");
        assert_eq!(rust_ident("r#loop"), "r#loop");
        assert_eq!(rust_ident("matches"), "matches");
        assert_eq!(rust_ident("union"), "union");
    }
}
