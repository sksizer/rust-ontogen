//! Each fn-level macro removes the `#[ontogen(...)]` markers on the fn's
//! arguments, which Rust would otherwise reject; these fns only compile if
//! it does.

use ontogen_macros::{get, ontogen, post, stateless};

#[ontogen]
pub fn bare(#[ontogen(query)] path: &str) -> &str {
    path
}

#[ontogen(rename = "renamed")]
pub fn renamed(id: u32, #[ontogen(query)] path: &str) -> String {
    format!("{id}:{path}")
}

#[stateless]
pub fn without_state(#[ontogen(query)] n: u32) -> u32 {
    n
}

#[get]
pub fn forced_get(#[ontogen(query)] n: u32, #[allow(unused_variables)] m: u32) -> u32 {
    n
}

#[post]
pub fn forced_post(n: u32) -> u32 {
    n
}

#[test]
fn the_marked_fns_keep_their_signatures() {
    assert_eq!(bare("a/b.md"), "a/b.md");
    assert_eq!(renamed(1, "a/b"), "1:a/b");
    assert_eq!(without_state(2), 2);
    assert_eq!(forced_get(3, 4), 3);
    assert_eq!(forced_post(5), 5);
}
