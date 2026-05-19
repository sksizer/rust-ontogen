//! Case-transform implementation for serde's eight `rename_all` modes.
//!
//! Mirrors `serde_derive_internals::case` so a property test that round-trips
//! a fixture value through `serde_json::to_string` and compares the resulting
//! JSON keys / discriminants to what ontogen-ts emits is a tight check, not a
//! best-effort approximation.
//!
//! Serde has two entry points and the rules differ:
//!
//! - [`RenameAll::apply_to_field`] assumes the input is **snake_case** (Rust
//!   field idents are snake_case by convention). It splits on `_` to recover
//!   words, then re-emits them in the target case.
//! - [`RenameAll::apply_to_variant`] assumes the input is **PascalCase**
//!   (Rust variant idents are PascalCase by convention). It splits on
//!   uppercase-letter boundaries (one letter per word, naively — there's no
//!   acronym detection, so `HTMLParser` becomes `h_t_m_l_parser` under
//!   `snake_case`, exactly as serde does it).
//!
//! `heck` is deliberately NOT used here: its acronym handling diverges from
//! serde's (`heck` smart-splits `HTMLParser` into `html_parser` for
//! `snake_case`, which differs from serde's `h_t_m_l_parser` literal output).
//! Mirroring serde means our emitted TS field names match what `serde_json`
//! actually puts on the wire — which is what the consumers reading our
//! generated `.d.ts`-equivalents need.

use crate::types::RenameAll;

impl RenameAll {
    /// Apply this `rename_all` mode to a **field** ident (assumed snake_case).
    ///
    /// Examples (mirroring serde's `RenameRule::apply_to_field`):
    ///
    /// ```text
    /// "parse_url_v2"  → camelCase           → "parseUrlV2"
    /// "parse_url_v2"  → PascalCase          → "ParseUrlV2"
    /// "parse_url_v2"  → SCREAMING_SNAKE_CASE → "PARSE_URL_V2"
    /// "parse_url_v2"  → kebab-case          → "parse-url-v2"
    /// ```
    pub fn apply_to_field(self, field: &str) -> String {
        match self {
            // For snake-case-ish inputs, these are identity (snake_case) or
            // simple ascii-case (Lowercase = lowercase the whole thing,
            // which leaves snake-case unchanged because `_` is unchanged
            // and ascii letters are already lower).
            Self::Lowercase | Self::SnakeCase => field.to_owned(),
            Self::Uppercase => field.to_ascii_uppercase(),
            Self::PascalCase => snake_to_pascal(field),
            Self::CamelCase => {
                let pascal = snake_to_pascal(field);
                lowercase_first_char(&pascal)
            }
            Self::ScreamingSnakeCase => field.to_ascii_uppercase(),
            Self::KebabCase => field.replace('_', "-"),
            Self::ScreamingKebabCase => field.to_ascii_uppercase().replace('_', "-"),
        }
    }

    /// Apply this `rename_all` mode to a **variant** ident (assumed PascalCase).
    ///
    /// Examples (mirroring serde's `RenameRule::apply_to_variant`):
    ///
    /// ```text
    /// "HTMLParser"  → snake_case  → "h_t_m_l_parser"  (no acronym detection!)
    /// "HTMLParser"  → camelCase   → "hTMLParser"      (just lowercase first ch)
    /// "ApiClient"   → snake_case  → "api_client"
    /// "ApiClient"   → kebab-case  → "api-client"
    /// ```
    pub fn apply_to_variant(self, variant: &str) -> String {
        match self {
            // PascalCase is the assumed input form — identity.
            Self::PascalCase => variant.to_owned(),
            Self::Lowercase => variant.to_ascii_lowercase(),
            Self::Uppercase => variant.to_ascii_uppercase(),
            // serde's camelCase on a variant just lowercases the first char.
            Self::CamelCase => lowercase_first_char(variant),
            Self::SnakeCase => pascal_to_snake(variant),
            Self::ScreamingSnakeCase => pascal_to_snake(variant).to_ascii_uppercase(),
            Self::KebabCase => pascal_to_snake(variant).replace('_', "-"),
            Self::ScreamingKebabCase => pascal_to_snake(variant).to_ascii_uppercase().replace('_', "-"),
        }
    }
}

/// `parse_url_v2` → `ParseUrlV2`.
///
/// Treats `_` as a word separator and uppercases the first letter of each
/// word; everything else passes through unchanged.
fn snake_to_pascal(snake: &str) -> String {
    let mut out = String::with_capacity(snake.len());
    let mut capitalize_next = true;
    for ch in snake.chars() {
        if ch == '_' {
            capitalize_next = true;
        } else if capitalize_next {
            out.push(ch.to_ascii_uppercase());
            capitalize_next = false;
        } else {
            out.push(ch);
        }
    }
    out
}

/// `HTMLParser` → `h_t_m_l_parser`. Inserts `_` before each non-leading
/// uppercase letter and lowercases the whole string. No acronym detection —
/// this is intentional, matching serde's literal output so round-trip tests
/// against `serde_json::to_string` line up exactly.
fn pascal_to_snake(pascal: &str) -> String {
    let mut out = String::with_capacity(pascal.len() + 4);
    for (i, ch) in pascal.char_indices() {
        if i > 0 && ch.is_ascii_uppercase() {
            out.push('_');
        }
        out.push(ch.to_ascii_lowercase());
    }
    out
}

/// Lowercase the first character of `s`; pass the rest through.
fn lowercase_first_char(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => {
            let mut out = String::with_capacity(s.len());
            out.push(first.to_ascii_lowercase());
            out.extend(chars);
            out
        }
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use crate::types::RenameAll;

    // ── apply_to_field ────────────────────────────────────────────────────

    #[test]
    fn field_lowercase_is_identity() {
        assert_eq!(RenameAll::Lowercase.apply_to_field("parse_url_v2"), "parse_url_v2");
    }

    #[test]
    fn field_snake_case_is_identity() {
        assert_eq!(RenameAll::SnakeCase.apply_to_field("parse_url_v2"), "parse_url_v2");
    }

    #[test]
    fn field_uppercase() {
        assert_eq!(RenameAll::Uppercase.apply_to_field("parse_url_v2"), "PARSE_URL_V2");
    }

    #[test]
    fn field_pascal_case() {
        assert_eq!(RenameAll::PascalCase.apply_to_field("parse_url_v2"), "ParseUrlV2");
        assert_eq!(RenameAll::PascalCase.apply_to_field("html_parser"), "HtmlParser");
        assert_eq!(RenameAll::PascalCase.apply_to_field("single"), "Single");
    }

    #[test]
    fn field_camel_case() {
        assert_eq!(RenameAll::CamelCase.apply_to_field("parse_url_v2"), "parseUrlV2");
        assert_eq!(RenameAll::CamelCase.apply_to_field("html_parser"), "htmlParser");
        assert_eq!(RenameAll::CamelCase.apply_to_field("single"), "single");
    }

    #[test]
    fn field_screaming_snake_case() {
        assert_eq!(RenameAll::ScreamingSnakeCase.apply_to_field("parse_url_v2"), "PARSE_URL_V2");
    }

    #[test]
    fn field_kebab_case() {
        assert_eq!(RenameAll::KebabCase.apply_to_field("parse_url_v2"), "parse-url-v2");
    }

    #[test]
    fn field_screaming_kebab_case() {
        assert_eq!(RenameAll::ScreamingKebabCase.apply_to_field("parse_url_v2"), "PARSE-URL-V2");
    }

    // ── apply_to_variant ──────────────────────────────────────────────────

    #[test]
    fn variant_pascal_case_is_identity() {
        assert_eq!(RenameAll::PascalCase.apply_to_variant("ApiClient"), "ApiClient");
    }

    #[test]
    fn variant_lowercase() {
        assert_eq!(RenameAll::Lowercase.apply_to_variant("ApiClient"), "apiclient");
    }

    #[test]
    fn variant_uppercase() {
        assert_eq!(RenameAll::Uppercase.apply_to_variant("ApiClient"), "APICLIENT");
    }

    #[test]
    fn variant_camel_case() {
        // Just lowercase the first character — matches serde.
        assert_eq!(RenameAll::CamelCase.apply_to_variant("ApiClient"), "apiClient");
        assert_eq!(RenameAll::CamelCase.apply_to_variant("HTMLParser"), "hTMLParser");
    }

    #[test]
    fn variant_snake_case() {
        assert_eq!(RenameAll::SnakeCase.apply_to_variant("ApiClient"), "api_client");
        // Naive split — no acronym detection. Matches serde.
        assert_eq!(RenameAll::SnakeCase.apply_to_variant("HTMLParser"), "h_t_m_l_parser");
        assert_eq!(RenameAll::SnakeCase.apply_to_variant("Single"), "single");
    }

    #[test]
    fn variant_screaming_snake_case() {
        assert_eq!(RenameAll::ScreamingSnakeCase.apply_to_variant("ApiClient"), "API_CLIENT");
        assert_eq!(RenameAll::ScreamingSnakeCase.apply_to_variant("HTMLParser"), "H_T_M_L_PARSER");
    }

    #[test]
    fn variant_kebab_case() {
        assert_eq!(RenameAll::KebabCase.apply_to_variant("ApiClient"), "api-client");
    }

    #[test]
    fn variant_screaming_kebab_case() {
        assert_eq!(RenameAll::ScreamingKebabCase.apply_to_variant("ApiClient"), "API-CLIENT");
    }

    // ── edge cases ────────────────────────────────────────────────────────

    #[test]
    fn empty_input_pass_through() {
        // Every mode must handle empty input without panicking.
        for mode in [
            RenameAll::Lowercase,
            RenameAll::Uppercase,
            RenameAll::PascalCase,
            RenameAll::CamelCase,
            RenameAll::SnakeCase,
            RenameAll::ScreamingSnakeCase,
            RenameAll::KebabCase,
            RenameAll::ScreamingKebabCase,
        ] {
            assert_eq!(mode.apply_to_field(""), "");
            assert_eq!(mode.apply_to_variant(""), "");
        }
    }

    #[test]
    fn field_with_digit_segment_camel_case() {
        // Field `parse_v2` — snake-case input with a digit-only word.
        // PascalCase: "parse" → "Parse", "v2" → "V2" → "ParseV2"
        // camelCase: lowercase the first char → "parseV2"
        assert_eq!(RenameAll::PascalCase.apply_to_field("parse_v2"), "ParseV2");
        assert_eq!(RenameAll::CamelCase.apply_to_field("parse_v2"), "parseV2");
    }
}
