//! Building links (§4.2, §4.3, §7.2).
//!
//! Links are relative references built from the route template, never from
//! the request URI, so equal requests produce byte-equal links.

use std::collections::BTreeMap;

use crate::{
    document::{Links, PaginationLinks},
    request::collapse_duplicates,
};

/// Percent-encodes `segment` as one RFC 3986 path segment: every byte
/// outside `A-Z a-z 0-9 - . _ ~` is encoded (§4.2).
pub fn encode_path_segment(segment: &str) -> String {
    let mut out = String::with_capacity(segment.len());
    encode_into(&mut out, segment, false);
    out
}

/// Percent-encodes a query value as §4.3 writes it: like a path segment,
/// except that `,` stays literal because it separates `sort` and `include`
/// items.
pub fn encode_query_value(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    encode_into(&mut out, value, true);
    out
}

fn encode_into(out: &mut String, s: &str, keep_comma: bool) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for &byte in s.as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') || (keep_comma && byte == b',') {
            out.push(byte as char);
        } else {
            out.push('%');
            out.push(HEX[usize::from(byte >> 4)] as char);
            out.push(HEX[usize::from(byte & 0x0F)] as char);
        }
    }
}

/// The JSON:API query parameters of a request, in the canonical form that
/// `links.self` and the pagination links repeat (§4.3).
///
/// Parameters are written in a fixed order whatever order they were set in:
/// every `filter[…]` by ascending byte order of member name, then `sort`,
/// `include`, `page[offset]`, `page[limit]`. A parameter never set is
/// absent.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CanonicalQuery {
    filter: BTreeMap<String, String>,
    sort: Option<Vec<String>>,
    include: Option<Vec<String>>,
    page: Option<(u64, u64)>,
}

impl CanonicalQuery {
    /// An empty query: [`href`](Self::href) is the bare path.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets `filter[member]`. Setting a member again replaces its value.
    pub fn set_filter(&mut self, member: impl Into<String>, value: impl Into<String>) -> &mut Self {
        self.filter.insert(member.into(), value.into());
        self
    }

    /// Sets `sort` to its parsed items (`-created`, `title`), written joined
    /// by `,`.
    pub fn set_sort<I, S>(&mut self, items: I) -> &mut Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.sort = Some(items.into_iter().map(Into::into).collect());
        self
    }

    /// Sets `include` to its parsed paths. A repeated path keeps its first
    /// position (§4.3 rule 4). No paths writes `include=`, which a request
    /// for an empty `included` carries (§7.5).
    pub fn set_include<I, S>(&mut self, paths: I) -> &mut Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.include = Some(collapse_duplicates(paths.into_iter().map(Into::into).collect()));
        self
    }

    /// Sets `page[offset]` and `page[limit]` to the effective values.
    pub fn set_page(&mut self, offset: u64, limit: u64) -> &mut Self {
        self.page = Some((offset, limit));
        self
    }

    /// The query string, without the leading `?`; empty when no parameter
    /// is set.
    pub fn query_string(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        for (member, value) in &self.filter {
            parts.push(format!("filter%5B{}%5D={}", encode_path_segment(member), encode_query_value(value)));
        }
        if let Some(items) = &self.sort {
            parts.push(format!("sort={}", join_encoded(items)));
        }
        if let Some(paths) = &self.include {
            parts.push(format!("include={}", join_encoded(paths)));
        }
        if let Some((offset, limit)) = self.page {
            parts.push(format!("page%5Boffset%5D={offset}"));
            parts.push(format!("page%5Blimit%5D={limit}"));
        }
        parts.join("&")
    }

    /// `path` followed by the query, or the bare path when the query is
    /// empty (§4.3 rule 6).
    pub fn href(&self, path: &str) -> String {
        let query = self.query_string();
        if query.is_empty() { path.to_owned() } else { format!("{path}?{query}") }
    }
}

fn join_encoded(items: &[String]) -> String {
    items.iter().map(|item| encode_query_value(item)).collect::<Vec<_>>().join(",")
}

/// The links of a paginated document (§7.2): `self` and all four
/// pagination links, each carrying `query` (whose own page, if any, is
/// replaced) and the page it points at.
///
/// `offset` and `limit` are the effective values after clamping and
/// `total` the filter-aware count. `limit` is at least 1, because
/// `page[limit]=0` is rejected before a page is read.
pub fn pagination_links(path: &str, query: &CanonicalQuery, offset: u32, limit: u32, total: u64) -> Links {
    let (offset, limit) = (u64::from(offset), u64::from(limit.max(1)));
    let last_offset = if total == 0 { 0 } else { (total - 1) / limit * limit };
    let page = |at: u64| {
        let mut q = query.clone();
        q.set_page(at, limit);
        q.href(path)
    };
    let prev = if offset == 0 {
        None
    } else if offset >= total {
        Some(page(last_offset))
    } else {
        Some(page(offset.saturating_sub(limit)))
    };
    let next = if offset + limit >= total { None } else { Some(page(offset + limit)) };
    Links::new(page(offset)).with_pagination(PaginationLinks { first: page(0), prev, next, last: page(last_offset) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_segments_encode_everything_outside_unreserved() {
        assert_eq!(encode_path_segment("ship-the-emitter"), "ship-the-emitter");
        assert_eq!(encode_path_segment("A-Za-z0-9-._~"), "A-Za-z0-9-._~");
        assert_eq!(encode_path_segment("a b/c?d#e%f,g:h@i"), "a%20b%2Fc%3Fd%23e%25f%2Cg%3Ah%40i");
        assert_eq!(encode_path_segment("é"), "%C3%A9");
        assert_eq!(encode_path_segment("[x]"), "%5Bx%5D");
        assert_eq!(encode_path_segment("+&="), "%2B%26%3D");
    }

    #[test]
    fn query_values_keep_commas_literal() {
        assert_eq!(encode_query_value("closed/done"), "closed%2Fdone");
        assert_eq!(encode_query_value("-created,title"), "-created,title");
        assert_eq!(encode_query_value("a b"), "a%20b");
    }

    #[test]
    fn empty_query_is_the_bare_path() {
        assert_eq!(CanonicalQuery::new().href("/api/tasks"), "/api/tasks");
    }

    #[test]
    fn parameters_are_written_in_canonical_order_whatever_the_set_order() {
        let mut q = CanonicalQuery::new();
        q.set_page(0, 20).set_sort(["-created", "title"]).set_filter("status", "open/ready");
        assert_eq!(
            q.href("/api/tasks"),
            "/api/tasks?filter%5Bstatus%5D=open%2Fready&sort=-created,title&page%5Boffset%5D=0&page%5Blimit%5D=20"
        );
    }

    #[test]
    fn filters_sort_by_member_name_bytes() {
        let mut q = CanonicalQuery::new();
        q.set_filter("status", "closed/done").set_filter("epic_id", "markdown-backend").set_page(0, 20);
        assert_eq!(
            q.href("/api/tasks"),
            "/api/tasks?filter%5Bepic_id%5D=markdown-backend&filter%5Bstatus%5D=closed%2Fdone&page%5Boffset%5D=0&page%5Blimit%5D=20"
        );
        let mut by_bytes = CanonicalQuery::new();
        by_bytes.set_filter("b", "1").set_filter("B", "2").set_filter("a", "3");
        assert_eq!(by_bytes.query_string(), "filter%5BB%5D=2&filter%5Ba%5D=3&filter%5Bb%5D=1");
    }

    #[test]
    fn include_drops_duplicates_and_keeps_request_order() {
        let mut q = CanonicalQuery::new();
        q.set_include(["tags", "epic", "tags"]).set_page(0, 20);
        assert_eq!(q.href("/api/tasks"), "/api/tasks?include=tags,epic&page%5Boffset%5D=0&page%5Blimit%5D=20");
        let mut single = CanonicalQuery::new();
        single.set_include(["epic"]);
        assert_eq!(single.href("/api/tasks/ship-the-emitter"), "/api/tasks/ship-the-emitter?include=epic");
    }

    #[test]
    fn empty_include_is_written_with_an_empty_value() {
        let mut q = CanonicalQuery::new();
        q.set_include(Vec::<String>::new());
        assert_eq!(q.href("/api/tasks"), "/api/tasks?include=");
    }

    #[test]
    fn filter_values_and_members_are_encoded() {
        let mut q = CanonicalQuery::new();
        q.set_filter("title", "a,b c&d");
        assert_eq!(q.query_string(), "filter%5Btitle%5D=a,b%20c%26d");
    }

    fn offsets(links: &Links) -> [Option<String>; 5] {
        let p = links.pagination().unwrap();
        [
            Some(links.self_link().to_owned()),
            Some(p.first.clone()),
            p.prev.clone(),
            p.next.clone(),
            Some(p.last.clone()),
        ]
    }

    fn at(offset: u64, limit: u64) -> Option<String> {
        Some(format!("/api/tasks?page%5Boffset%5D={offset}&page%5Blimit%5D={limit}"))
    }

    #[test]
    fn pagination_links_match_the_contract_example() {
        let links = pagination_links("/api/tasks", &CanonicalQuery::new(), 20, 10, 45);
        assert_eq!(offsets(&links), [at(20, 10), at(0, 10), at(10, 10), at(30, 10), at(40, 10)]);
    }

    #[test]
    fn pagination_edges() {
        let q = CanonicalQuery::new();
        // First page of a default request.
        assert_eq!(
            offsets(&pagination_links("/api/tasks", &q, 0, 20, 45)),
            [at(0, 20), at(0, 20), None, at(20, 20), at(40, 20)]
        );
        // Clamped limit swallowing the whole set.
        assert_eq!(
            offsets(&pagination_links("/api/tasks", &q, 0, 100, 45)),
            [at(0, 100), at(0, 100), None, None, at(0, 100)]
        );
        // Empty collection.
        assert_eq!(
            offsets(&pagination_links("/api/tasks", &q, 0, 20, 0)),
            [at(0, 20), at(0, 20), None, None, at(0, 20)]
        );
        // Past the end: prev is the last page.
        assert_eq!(
            offsets(&pagination_links("/api/tasks", &q, 45, 10, 45)),
            [at(45, 10), at(0, 10), at(40, 10), None, at(40, 10)]
        );
        // Inside the last page: prev is the page before.
        assert_eq!(
            offsets(&pagination_links("/api/tasks", &q, 42, 10, 45)),
            [at(42, 10), at(0, 10), at(32, 10), None, at(40, 10)]
        );
        // Offset smaller than the limit: prev clamps at 0.
        assert_eq!(
            offsets(&pagination_links("/api/tasks", &q, 5, 10, 45)),
            [at(5, 10), at(0, 10), at(0, 10), at(15, 10), at(40, 10)]
        );
        // Exact multiple: the last page starts at total - limit.
        assert_eq!(offsets(&pagination_links("/api/tasks", &q, 0, 10, 40)).last().cloned().flatten(), at(30, 10));
    }

    #[test]
    fn pagination_links_carry_the_canonical_query_and_replace_its_page() {
        let mut q = CanonicalQuery::new();
        q.set_filter("status", "open/ready").set_sort(["-created", "title"]).set_page(999, 999);
        let links = pagination_links("/api/tasks", &q, 0, 20, 1);
        assert_eq!(
            links.self_link(),
            "/api/tasks?filter%5Bstatus%5D=open%2Fready&sort=-created,title&page%5Boffset%5D=0&page%5Blimit%5D=20"
        );
    }
}
