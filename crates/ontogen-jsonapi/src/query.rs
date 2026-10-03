//! Query parameters (§3.4, §6, §13.2 step 5).
//!
//! [`QueryParams::parse`] performs the first half of step 5: it rejects the
//! first name, in request order, that the route does not accept. The
//! accessors perform the second half for one parameter each (a repeat, or a
//! malformed page value), so a handler calls them in canonical order and
//! interleaves its own checks of `filter`, `sort` and `include` values where
//! they belong. [`QueryParams::check`] runs every accessor in that order for
//! routes with no checks of their own.
//!
//! The semantics of `filter`, `sort` and `include` are not decided here.

use std::collections::BTreeMap;

use crate::error::{ErrorCode, ErrorObject};

/// The query parameters a route accepts (§6). Anything else is
/// `400 invalid_query_parameter`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuerySpec {
    /// Accepted `filter[…]` members. Empty means the route takes no filter.
    pub filter: &'static [&'static str],
    /// Whether `sort` is accepted.
    pub sort: bool,
    /// Whether `include` is accepted.
    pub include: bool,
    /// Whether `page[offset]` and `page[limit]` are accepted.
    pub page: bool,
    /// Accepted `opArg[…]` members (custom `GET` ops, §10.2).
    pub op_args: &'static [&'static str],
}

impl QuerySpec {
    /// A route that accepts no query parameter.
    pub const NONE: QuerySpec = QuerySpec { filter: &[], sort: false, include: false, page: false, op_args: &[] };

    fn accepts(&self, name: &Name<'_>) -> bool {
        match *name {
            Name::Family("sort") => self.sort,
            Name::Family("include") => self.include,
            Name::Member("filter", member) => self.filter.contains(&member),
            Name::Member("page", "offset" | "limit") => self.page,
            Name::Member("opArg", member) => self.op_args.contains(&member),
            _ => false,
        }
    }
}

/// The parsed name of a query parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Name<'a> {
    /// `sort`
    Family(&'a str),
    /// `filter[status]`
    Member(&'a str, &'a str),
    /// Anything else with a bracket, or an empty name.
    Malformed,
}

fn parse_name(name: &str) -> Name<'_> {
    let Some(open) = name.find('[') else {
        return if name.is_empty() || name.contains(']') { Name::Malformed } else { Name::Family(name) };
    };
    let (family, rest) = (&name[..open], &name[open + 1..]);
    match rest.strip_suffix(']') {
        Some(member) if !family.is_empty() && !member.is_empty() && !member.contains(['[', ']']) => {
            Name::Member(family, member)
        }
        _ => Name::Malformed,
    }
}

/// One accepted parameter: its first value, and whether it was repeated.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Slot {
    value: String,
    repeated: bool,
}

/// A request's JSON:API query parameters, every name accepted by the route.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QueryParams {
    filter: BTreeMap<String, Slot>,
    sort: Option<Slot>,
    include: Option<Slot>,
    page_offset: Option<Slot>,
    page_limit: Option<Slot>,
    op_args: BTreeMap<String, Slot>,
}

impl QueryParams {
    /// Parses a raw query string (without `?`; `None` when the URI has
    /// none) against the route's accepted set.
    ///
    /// Names and values are percent-decoded, with `+` read as a space as
    /// `application/x-www-form-urlencoded` does, so `[`/`]` and `%5B`/`%5D`
    /// are the same name (§3.4). Empty `&`-separated pieces are skipped.
    ///
    /// Fails with `400 invalid_query_parameter` on the first name, in
    /// request order, that is malformed or not accepted, with
    /// `source.parameter` the decoded name.
    pub fn parse(raw: Option<&str>, spec: &QuerySpec) -> Result<QueryParams, ErrorObject> {
        let mut params = QueryParams::default();
        for piece in raw.unwrap_or("").split('&').filter(|p| !p.is_empty()) {
            let (raw_name, raw_value) = piece.split_once('=').unwrap_or((piece, ""));
            let (name, value) = (decode(raw_name), decode(raw_value));
            let parsed = parse_name(&name);
            if parsed == Name::Malformed {
                return Err(invalid(&name, format!("`{name}` is not a well-formed query parameter name")));
            }
            if !spec.accepts(&parsed) {
                return Err(invalid(&name, format!("`{name}` is not a query parameter of this route")));
            }
            match parsed {
                Name::Family("sort") => put(&mut params.sort, value),
                Name::Family(_) => put(&mut params.include, value),
                Name::Member("page", "offset") => put(&mut params.page_offset, value),
                Name::Member("page", _) => put(&mut params.page_limit, value),
                Name::Member("filter", member) => put_member(&mut params.filter, member, value),
                Name::Member(_, member) => put_member(&mut params.op_args, member, value),
                Name::Malformed => unreachable!("rejected above"),
            }
        }
        Ok(params)
    }

    /// Every `filter[…]` member and value, in ascending byte order of member
    /// name. Fails on the first repeated member in that order.
    pub fn filters(&self) -> Result<Vec<(&str, &str)>, ErrorObject> {
        members(&self.filter, "filter")
    }

    /// The raw `sort` value. Fails when `sort` is repeated.
    pub fn sort(&self) -> Result<Option<&str>, ErrorObject> {
        single(self.sort.as_ref(), "sort")
    }

    /// The raw `include` value. Fails when `include` is repeated.
    pub fn include(&self) -> Result<Option<&str>, ErrorObject> {
        single(self.include.as_ref(), "include")
    }

    /// `page[offset]`: one or more ASCII digits, leading zeros allowed, at
    /// most `4294967295` (§7.2).
    pub fn page_offset(&self) -> Result<Option<u32>, ErrorObject> {
        page(self.page_offset.as_ref(), "page[offset]", 0)
    }

    /// `page[limit]`: as `page[offset]`, and at least 1 (§7.2).
    pub fn page_limit(&self) -> Result<Option<u32>, ErrorObject> {
        page(self.page_limit.as_ref(), "page[limit]", 1)
    }

    /// Every `opArg[…]` member and value, in ascending byte order of member
    /// name. Fails on the first repeated member in that order.
    pub fn op_args(&self) -> Result<Vec<(&str, &str)>, ErrorObject> {
        members(&self.op_args, "opArg")
    }

    /// Every check of the accessors, in canonical order (§13.2 step 5):
    /// `filter[…]`, `sort`, `include`, `page[offset]`, `page[limit]`,
    /// `opArg[…]`.
    pub fn check(&self) -> Result<(), ErrorObject> {
        self.filters()?;
        self.sort()?;
        self.include()?;
        self.page_offset()?;
        self.page_limit()?;
        self.op_args()?;
        Ok(())
    }
}

fn put(slot: &mut Option<Slot>, value: String) {
    match slot {
        Some(slot) => slot.repeated = true,
        None => *slot = Some(Slot { value, repeated: false }),
    }
}

fn put_member(slots: &mut BTreeMap<String, Slot>, member: &str, value: String) {
    match slots.get_mut(member) {
        Some(slot) => slot.repeated = true,
        None => {
            slots.insert(member.to_owned(), Slot { value, repeated: false });
        }
    }
}

fn members<'a>(slots: &'a BTreeMap<String, Slot>, family: &str) -> Result<Vec<(&'a str, &'a str)>, ErrorObject> {
    slots
        .iter()
        .map(|(member, slot)| {
            if slot.repeated {
                Err(repeated(&format!("{family}[{member}]")))
            } else {
                Ok((member.as_str(), slot.value.as_str()))
            }
        })
        .collect()
}

fn single<'a>(slot: Option<&'a Slot>, name: &str) -> Result<Option<&'a str>, ErrorObject> {
    match slot {
        None => Ok(None),
        Some(slot) if slot.repeated => Err(repeated(name)),
        Some(slot) => Ok(Some(&slot.value)),
    }
}

fn page(slot: Option<&Slot>, name: &str, min: u32) -> Result<Option<u32>, ErrorObject> {
    let Some(value) = single(slot, name)? else { return Ok(None) };
    match value.bytes().all(|b| b.is_ascii_digit()).then(|| value.parse::<u32>().ok()).flatten() {
        Some(n) if n >= min => Ok(Some(n)),
        _ => Err(invalid(name, format!("{name} must be an integer between {min} and {}", u32::MAX))),
    }
}

fn invalid(name: &str, detail: String) -> ErrorObject {
    ErrorObject::new(ErrorCode::InvalidQueryParameter, detail).with_parameter(name)
}

fn repeated(name: &str) -> ErrorObject {
    invalid(name, format!("`{name}` is given more than once"))
}

/// Percent-decodes a query-string component, reading `+` as a space.
/// Bytes that do not form UTF-8 decode lossily, as `axum::extract::Query`
/// treats them.
fn decode(s: &str) -> String {
    String::from_utf8_lossy(&percent_decode(s, true)).into_owned()
}

/// Percent-decodes `s` to bytes, reading `+` as a space when
/// `plus_as_space` (a query component, not a path segment). A `%` not
/// followed by two hex digits stays literal.
pub(crate) fn percent_decode(s: &str, plus_as_space: bool) -> Vec<u8> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let escaped = match bytes.get(i + 1..i + 3) {
            Some(&[hi, lo]) if bytes[i] == b'%' => hex(hi).zip(hex(lo)).map(|(hi, lo)| hi << 4 | lo),
            _ => None,
        };
        match (escaped, bytes[i]) {
            (Some(byte), _) => {
                out.push(byte);
                i += 3;
            }
            (None, b'+') if plus_as_space => {
                out.push(b' ');
                i += 1;
            }
            (None, byte) => {
                out.push(byte);
                i += 1;
            }
        }
    }
    out
}

fn hex(b: u8) -> Option<u8> {
    char::from(b).to_digit(16).and_then(|d| u8::try_from(d).ok())
}

#[cfg(test)]
mod tests {
    use http::StatusCode;

    use super::*;

    const LIST: QuerySpec =
        QuerySpec { filter: &["status", "epic_id"], sort: true, include: true, page: true, op_args: &[] };
    const GET_OP: QuerySpec = QuerySpec { op_args: &["verbose", "limit"], ..QuerySpec::NONE };

    fn rejected(raw: &str, spec: &QuerySpec) -> String {
        let err = QueryParams::parse(Some(raw), spec).unwrap_err();
        assert_eq!(err.status(), StatusCode::BAD_REQUEST);
        assert_eq!(err.code(), "invalid_query_parameter");
        match err.source() {
            Some(crate::ErrorSource::Parameter(p)) => p.clone(),
            other => panic!("expected a parameter source, got {other:?}"),
        }
    }

    fn checked(raw: &str, spec: &QuerySpec) -> Result<QueryParams, String> {
        let params = QueryParams::parse(Some(raw), spec).map_err(|e| format!("parse: {e}"))?;
        params.check().map_err(|e| match e.source() {
            Some(crate::ErrorSource::Parameter(p)) => p.clone(),
            _ => panic!("no parameter source"),
        })?;
        Ok(params)
    }

    #[test]
    fn no_query_and_empty_query_parse_to_nothing() {
        assert_eq!(QueryParams::parse(None, &QuerySpec::NONE).unwrap(), QueryParams::default());
        assert_eq!(QueryParams::parse(Some(""), &QuerySpec::NONE).unwrap(), QueryParams::default());
        assert_eq!(QueryParams::parse(Some("&&"), &QuerySpec::NONE).unwrap(), QueryParams::default());
    }

    #[test]
    fn accepted_parameters_are_read() {
        let q = checked(
            "filter[status]=closed%2Fdone&sort=-created,title&include=epic&page[offset]=020&page[limit]=10",
            &LIST,
        )
        .unwrap();
        assert_eq!(q.filters().unwrap(), vec![("status", "closed/done")]);
        assert_eq!(q.sort().unwrap(), Some("-created,title"));
        assert_eq!(q.include().unwrap(), Some("epic"));
        assert_eq!(q.page_offset().unwrap(), Some(20));
        assert_eq!(q.page_limit().unwrap(), Some(10));
    }

    #[test]
    fn raw_and_encoded_brackets_are_the_same_name() {
        let raw = QueryParams::parse(Some("filter[status]=a&page[limit]=5"), &LIST).unwrap();
        let encoded = QueryParams::parse(Some("filter%5Bstatus%5D=a&page%5blimit%5d=5"), &LIST).unwrap();
        assert_eq!(raw, encoded);
        // Mixed within one name too.
        let mixed = QueryParams::parse(Some("filter%5Bstatus]=a&page[limit%5D=5"), &LIST).unwrap();
        assert_eq!(raw, mixed);
        // A repeat across spellings is a repeat.
        assert_eq!(checked("page[limit]=5&page%5Blimit%5D=6", &LIST).unwrap_err(), "page[limit]");
    }

    #[test]
    fn unaccepted_names_are_rejected_with_their_decoded_name() {
        assert_eq!(rejected("foo=1", &LIST), "foo");
        assert_eq!(rejected("filter%5Bowner%5D=x", &LIST), "filter[owner]");
        assert_eq!(rejected("page[number]=1", &LIST), "page[number]");
        assert_eq!(rejected("page[size]=1", &LIST), "page[size]");
        assert_eq!(rejected("fields[tasks]=title", &LIST), "fields[tasks]");
        assert_eq!(rejected("limit=10", &LIST), "limit");
        assert_eq!(rejected("Sort=title", &LIST), "Sort");
        assert_eq!(rejected("include=epic", &QuerySpec::NONE), "include");
        assert_eq!(rejected("sort=title", &QuerySpec::NONE), "sort");
        assert_eq!(rejected("page[limit]=1", &QuerySpec { page: false, ..LIST }), "page[limit]");
        assert_eq!(rejected("filter[status]=a", &QuerySpec { filter: &[], ..LIST }), "filter[status]");
        assert_eq!(rejected("verbose=true", &GET_OP), "verbose");
        assert_eq!(rejected("opArg[other]=1", &GET_OP), "opArg[other]");
        assert_eq!(rejected("oparg[verbose]=1", &GET_OP), "oparg[verbose]");
    }

    #[test]
    fn malformed_names_are_rejected() {
        for (raw, name) in [
            ("filter[status][]=a", "filter[status][]"),
            ("filter[status][x]=a", "filter[status][x]"),
            ("filter[]=a", "filter[]"),
            ("filter[status=a", "filter[status"),
            ("filterstatus]=a", "filterstatus]"),
            ("[status]=a", "[status]"),
            ("=a", ""),
            ("filter[a]b]=1", "filter[a]b]"),
        ] {
            assert_eq!(rejected(raw, &LIST), name, "{raw}");
        }
    }

    #[test]
    fn dotted_filter_members_are_unknown_members() {
        assert_eq!(rejected("filter[x.y]=1", &LIST), "filter[x.y]");
    }

    #[test]
    fn the_first_unaccepted_name_in_request_order_wins() {
        assert_eq!(rejected("sort=x&b=1&a=2", &LIST), "b");
        assert_eq!(rejected("page[limit]=0&zzz=1", &LIST), "zzz");
        assert_eq!(rejected("sort=a&sort=b&zzz=1", &LIST), "zzz");
    }

    #[test]
    fn accepted_parameters_are_checked_in_canonical_order() {
        // Repeated sort before a bad page, whatever the request order.
        assert_eq!(checked("page[limit]=0&sort=a&sort=b", &LIST).unwrap_err(), "sort");
        assert_eq!(checked("page[limit]=0&page[offset]=-1", &LIST).unwrap_err(), "page[offset]");
        assert_eq!(
            checked("include=a&include=b&filter[status]=x&filter[status]=y", &LIST).unwrap_err(),
            "filter[status]"
        );
        assert_eq!(
            checked("filter[status]=x&filter[status]=y&filter[epic_id]=1&filter[epic_id]=2", &LIST).unwrap_err(),
            "filter[epic_id]"
        );
        assert_eq!(checked("include=a&include=b&page[limit]=x", &LIST).unwrap_err(), "include");
        assert_eq!(
            checked("opArg[verbose]=1&opArg[verbose]=2&opArg[limit]=1&opArg[limit]=1", &GET_OP).unwrap_err(),
            "opArg[limit]"
        );
    }

    #[test]
    fn page_values() {
        let limit = |v: &str| {
            let q = QueryParams::parse(Some(&format!("page[limit]={v}")), &LIST).unwrap();
            q.page_limit().map_err(|e| e.detail().to_owned())
        };
        let offset = |v: &str| QueryParams::parse(Some(&format!("page[offset]={v}")), &LIST).unwrap().page_offset();
        assert_eq!(limit("10"), Ok(Some(10)));
        assert_eq!(limit("020"), Ok(Some(20)));
        assert_eq!(limit("4294967295"), Ok(Some(u32::MAX)));
        assert_eq!(limit("0"), Err("page[limit] must be an integer between 1 and 4294967295".to_owned()));
        for bad in ["-1", "+5", "", "1.5", "ten", "4294967296", "%2B5", " 5", "1e3"] {
            assert!(limit(bad).is_err(), "page[limit]={bad}");
            assert!(offset(bad).is_err(), "page[offset]={bad}");
        }
        assert_eq!(offset("0").unwrap(), Some(0));
        assert_eq!(offset("000").unwrap(), Some(0));
        let err = offset("x").unwrap_err();
        assert_eq!(err.detail(), "page[offset] must be an integer between 0 and 4294967295");
        assert_eq!(err.source(), Some(&crate::ErrorSource::Parameter("page[offset]".to_owned())));
    }

    #[test]
    fn values_are_form_decoded() {
        let q = QueryParams::parse(Some("filter[status]=open+ready%2Fx&filter[epic_id]=%zz"), &LIST).unwrap();
        assert_eq!(q.filters().unwrap(), vec![("epic_id", "%zz"), ("status", "open ready/x")]);
        let q = QueryParams::parse(Some("include"), &LIST).unwrap();
        assert_eq!(q.include().unwrap(), Some(""));
        let q = QueryParams::parse(Some("sort=a=b"), &LIST).unwrap();
        assert_eq!(q.sort().unwrap(), Some("a=b"));
    }

    #[test]
    fn op_args_are_read_in_member_order() {
        let q = checked("opArg[verbose]=true&opArg[limit]=5", &GET_OP).unwrap();
        assert_eq!(q.op_args().unwrap(), vec![("limit", "5"), ("verbose", "true")]);
    }

    #[test]
    fn decoding() {
        assert_eq!(decode("a%5Bb%5d"), "a[b]");
        assert_eq!(decode("%"), "%");
        assert_eq!(decode("%4"), "%4");
        assert_eq!(decode("%4g"), "%4g");
        assert_eq!(decode("a+b"), "a b");
        assert_eq!(decode("%2B"), "+");
        assert_eq!(decode("%C3%A9"), "é");
        assert_eq!(decode("%FF"), "\u{FFFD}");
        assert_eq!(percent_decode("a+b%20c", false), b"a+b c");
        assert_eq!(percent_decode("%FF", false), [0xFF]);
    }
}
