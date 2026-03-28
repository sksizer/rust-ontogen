/// Strip wikilink brackets from a string: "[[foo]]" → "foo", "bar" → "bar".
/// No-op stub for projects that don't use markdown persistence.
pub fn strip_wikilink(s: &str) -> String {
    s.trim_start_matches("[[").trim_end_matches("]]").to_string()
}

/// Strip wikilink brackets from an optional string.
pub fn strip_wikilink_opt(s: Option<String>) -> Option<String> {
    s.map(|v| strip_wikilink(&v))
}

/// Strip wikilink brackets from each string in a vec.
pub fn strip_wikilinks_vec(v: Vec<String>) -> Vec<String> {
    v.into_iter().map(|s| strip_wikilink(&s)).collect()
}
