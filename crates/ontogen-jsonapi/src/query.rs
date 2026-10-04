//! Query parameters (§3.4, §6, §7.3, §13.2 step 5).
//!
//! [`QueryParams::parse`] performs the first half of step 5: it rejects the
//! first name, in request order, that the route does not accept. The
//! accessors perform the second half for one parameter each (a repeat, or a
//! value that does not read), so a handler calls them in canonical order and
//! interleaves its own checks of `sort` and `include` values where they
//! belong. [`QueryParams::check`] runs every accessor in that order for
//! routes with no checks of their own.
//!
//! `filter[…]` values are read here, into the list's `*Query` struct
//! ([`QueryParams::filter`]) and its bare filter parameters
//! ([`QueryParams::filter_member`]); what a filter means is the handler's
//! business. The semantics of `sort` and `include` are not decided here.

mod form;

use std::collections::BTreeMap;

use serde::de::DeserializeOwned;

use crate::{
    error::{ErrorCode, ErrorObject},
    links::CanonicalQuery,
};

/// The query parameters a route accepts (§6). Anything else is
/// `400 invalid_query_parameter`.
///
/// A list's filter (§7.3) is its bare filter parameters, each a
/// `filter[{name}]` member listed in `filter`, and at most one `*Query`
/// struct, whose fields are members too:
///
/// ```
/// use ontogen_jsonapi::{QuerySpec, filter_fields};
///
/// #[derive(serde::Deserialize)]
/// struct ListTasksQuery {
///     status: Option<String>,
/// }
///
/// const SPEC: QuerySpec = QuerySpec {
///     filter: &["skill_id"],
///     filter_fields: Some(filter_fields::<ListTasksQuery>),
///     page: true,
///     ..QuerySpec::NONE
/// };
/// ```
#[derive(Debug, Clone, Copy)]
pub struct QuerySpec {
    /// Accepted bare `filter[…]` members, one per bare filter parameter.
    pub filter: &'static [&'static str],
    /// The fields of the `*Query` struct the route reads from the `filter`
    /// family, each an accepted `filter[…]` member: [`filter_fields`] of
    /// that struct. A route that takes no filter has neither these nor
    /// bare members.
    pub filter_fields: Option<fn() -> &'static [&'static str]>,
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
    pub const NONE: QuerySpec =
        QuerySpec { filter: &[], filter_fields: None, sort: false, include: false, page: false, op_args: &[] };

    fn accepts(&self, name: &Name<'_>) -> bool {
        match *name {
            Name::Family("sort") => self.sort,
            Name::Family("include") => self.include,
            Name::Member("filter", member) => {
                self.filter.contains(&member) || self.filter_fields.is_some_and(|fields| fields().contains(&member))
            }
            Name::Member("page", "offset" | "limit") => self.page,
            Name::Member("opArg", member) => self.op_args.contains(&member),
            _ => false,
        }
    }
}

/// The field names serde's derive declares for the struct `T`, renamed as
/// serde reports them: the `filter[…]` members a `*Query` struct reads
/// (§7.3). `&[]` when `T` does not deserialize as a plain struct (a map, an
/// enum, a newtype, a struct with a flattened field), so a route that reads
/// such a `T` accepts no member for it.
///
/// ```
/// #[derive(serde::Deserialize)]
/// struct ListTasksQuery {
///     status: Option<String>,
///     #[serde(rename = "epic")]
///     epic_id: Option<String>,
/// }
///
/// assert_eq!(ontogen_jsonapi::filter_fields::<ListTasksQuery>(), ["status", "epic"]);
/// ```
pub fn filter_fields<T: DeserializeOwned>() -> &'static [&'static str] {
    form::struct_fields::<T>()
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

    /// The list's `*Query` struct, read from the `filter[…]` members that
    /// are its fields ([`filter_fields`]); the other members are bare
    /// filters, read with [`filter_member`](Self::filter_member).
    ///
    /// Members are read in ascending byte order of name, each value as
    /// `axum::extract::Query` reads a field (see [`op_arg`](Self::op_arg)),
    /// and the first failure in that order is the error: a repeated member,
    /// or a value serde rejects. Then a required field with no member fails,
    /// the first in declaration order as serde reports it. Each is
    /// `400 invalid_query_parameter` with `source.parameter` the member
    /// (`filter[status]`); a failure serde does not tie to a member names
    /// `filter`.
    pub fn filter<T: DeserializeOwned>(&self) -> Result<T, ErrorObject> {
        let fields = filter_fields::<T>();
        let members = self
            .filter
            .iter()
            .filter(|(member, _)| fields.contains(&member.as_str()))
            .map(|(member, slot)| (member.as_str(), slot.value.as_str(), slot.repeated));
        form::read_struct(members).map_err(|err| match err {
            form::Error::Repeated(member) => repeated(&filter_name(&member)),
            form::Error::Member { member, message } => {
                let parameter = filter_name(&member);
                invalid(&parameter, format!("`{parameter}` is invalid: {message}"))
            }
            form::Error::MissingField(field) => required(&filter_name(field)),
            form::Error::Custom(message) => invalid("filter", format!("`filter` is invalid: {message}")),
        })
    }

    /// The bare filter `filter[{name}]` read as `T`, or `None` when absent.
    /// Call it for each bare filter in ascending byte order of name, after
    /// [`filter`](Self::filter).
    ///
    /// The value reads as with [`op_arg`](Self::op_arg). A repeated or
    /// malformed value is `400 invalid_query_parameter`, with
    /// `source.parameter` the name.
    pub fn filter_member<T: DeserializeOwned>(&self, name: &str) -> Result<Option<T>, ErrorObject> {
        member_value(self.filter.get(name), &filter_name(name))
    }

    /// As [`filter_member`](Self::filter_member), for a bare filter the list
    /// requires: absent is `400 invalid_query_parameter` too.
    pub fn required_filter_member<T: DeserializeOwned>(&self, name: &str) -> Result<T, ErrorObject> {
        let parameter = filter_name(name);
        member_value(self.filter.get(name), &parameter)?.ok_or_else(|| required(&parameter))
    }

    /// Every `filter[…]` member with its value as the request gave it
    /// (decoded), for the links to repeat (§4.3). Fails on the first
    /// repeated member in ascending byte order of name.
    pub fn link_query(&self) -> Result<CanonicalQuery, ErrorObject> {
        let mut query = CanonicalQuery::new();
        for (member, value) in self.filters()? {
            query.set_filter(member, value);
        }
        Ok(query)
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

    /// `opArg[{name}]` read as `T`, or `None` when absent (§10.2). Call it
    /// for each argument in ascending byte order of name, the order step 5
    /// checks them in.
    ///
    /// The value is read as `axum::extract::Query` (`serde_urlencoded`)
    /// reads a field: `true` is a `bool`, `5` a number, `high` a unit enum
    /// variant, anything a string, and an `Option` is `Some` whenever the
    /// parameter is present (`Some("")` for an empty string). A repeated or
    /// malformed value is `400 invalid_query_parameter`, with
    /// `source.parameter` the name.
    pub fn op_arg<T: DeserializeOwned>(&self, name: &str) -> Result<Option<T>, ErrorObject> {
        member_value(self.op_args.get(name), &format!("opArg[{name}]"))
    }

    /// `opArg[{name}]` read as a page value, or `None` when absent: the
    /// digits `page[offset]` takes (§7.2), `0` included. A paginated list
    /// that is not served as a resource takes its page as `opArg[limit]` and
    /// `opArg[offset]`, and reads them as strictly as a resource list reads
    /// `page[…]`, which [`op_arg`](Self::op_arg) would not (`%2B5` is `5` to
    /// it). Anything else is `400 invalid_query_parameter`, with
    /// `source.parameter` the name.
    pub fn page_op_arg(&self, name: &str) -> Result<Option<u32>, ErrorObject> {
        page(self.op_args.get(name), &format!("opArg[{name}]"), 0)
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

fn member_value<T: DeserializeOwned>(slot: Option<&Slot>, parameter: &str) -> Result<Option<T>, ErrorObject> {
    let Some(value) = single(slot, parameter)? else { return Ok(None) };
    T::deserialize(form::Value(value))
        .map(Some)
        .map_err(|err| invalid(parameter, format!("`{parameter}` is invalid: {err}")))
}

fn filter_name(member: &str) -> String {
    format!("filter[{member}]")
}

fn invalid(name: &str, detail: String) -> ErrorObject {
    ErrorObject::new(ErrorCode::InvalidQueryParameter, detail).with_parameter(name)
}

fn repeated(name: &str) -> ErrorObject {
    invalid(name, format!("`{name}` is given more than once"))
}

fn required(name: &str) -> ErrorObject {
    invalid(name, format!("`{name}` is required"))
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
        QuerySpec { filter: &["status", "epic_id"], sort: true, include: true, page: true, ..QuerySpec::NONE };
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

    fn op_arg_failure<T: serde::de::DeserializeOwned + std::fmt::Debug>(q: &QueryParams, name: &str) -> String {
        let err = q.op_arg::<T>(name).unwrap_err();
        assert_eq!((err.status(), err.code()), (StatusCode::BAD_REQUEST, "invalid_query_parameter"));
        match err.source() {
            Some(crate::ErrorSource::Parameter(p)) => p.clone(),
            other => panic!("expected a parameter source, got {other:?}"),
        }
    }

    #[test]
    fn op_arg_values_read_as_a_form_field() {
        #[derive(Debug, PartialEq, serde::Deserialize)]
        #[serde(rename_all = "snake_case")]
        enum Kind {
            Workout,
        }
        const SPEC: QuerySpec =
            QuerySpec { op_args: &["verbose", "limit", "name", "kind", "empty"], ..QuerySpec::NONE };
        let q = QueryParams::parse(
            Some("opArg%5Bverbose%5D=true&opArg[limit]=5&opArg[name]=a+b%26c&opArg[kind]=workout&opArg[empty]="),
            &SPEC,
        )
        .unwrap();
        assert_eq!(q.op_arg::<bool>("verbose").unwrap(), Some(true));
        assert_eq!(q.op_arg::<u32>("limit").unwrap(), Some(5));
        assert_eq!(q.op_arg::<String>("limit").unwrap(), Some("5".to_owned()));
        assert_eq!(q.op_arg::<String>("name").unwrap(), Some("a b&c".to_owned()));
        assert_eq!(q.op_arg::<Kind>("kind").unwrap(), Some(Kind::Workout));
        assert_eq!(q.op_arg::<String>("empty").unwrap(), Some(String::new()));
        // Absent is `None`; the name was accepted by the route.
        assert_eq!(QueryParams::parse(Some(""), &SPEC).unwrap().op_arg::<bool>("verbose").unwrap(), None);
    }

    #[test]
    fn a_malformed_or_repeated_op_arg_names_its_parameter() {
        let q = QueryParams::parse(Some("opArg[verbose]=yes&opArg[limit]=-1&opArg[limit]=2"), &GET_OP).unwrap();
        assert_eq!(op_arg_failure::<bool>(&q, "verbose"), "opArg[verbose]");
        assert_eq!(op_arg_failure::<u32>(&q, "limit"), "opArg[limit]");
        let q = QueryParams::parse(Some("opArg[limit]=-1"), &GET_OP).unwrap();
        assert_eq!(op_arg_failure::<u32>(&q, "limit"), "opArg[limit]");
        let q = QueryParams::parse(Some("opArg[limit]="), &GET_OP).unwrap();
        assert_eq!(op_arg_failure::<u32>(&q, "limit"), "opArg[limit]");
    }

    #[test]
    fn page_op_args_read_as_page_values() {
        const SPEC: QuerySpec = QuerySpec { op_args: &["limit", "offset"], ..QuerySpec::NONE };
        let read =
            |v: &str| QueryParams::parse(Some(&format!("opArg[limit]={v}")), &SPEC).unwrap().page_op_arg("limit");
        assert_eq!(read("5").unwrap(), Some(5));
        assert_eq!(read("020").unwrap(), Some(20));
        assert_eq!(read("0").unwrap(), Some(0));
        assert_eq!(read("4294967295").unwrap(), Some(u32::MAX));
        assert_eq!(QueryParams::parse(None, &SPEC).unwrap().page_op_arg("limit").unwrap(), None);
        for bad in ["+5", "%2B5", "-1", "", "1.5", " 5", "ten", "4294967296", "1e3"] {
            let err = read(bad).unwrap_err();
            assert_eq!((err.status(), err.code()), (StatusCode::BAD_REQUEST, "invalid_query_parameter"), "{bad}");
            assert_eq!(err.source(), Some(&crate::ErrorSource::Parameter("opArg[limit]".to_owned())), "{bad}");
            assert_eq!(err.detail(), "opArg[limit] must be an integer between 0 and 4294967295");
        }
        let q = QueryParams::parse(Some("opArg[offset]=1&opArg[offset]=2"), &SPEC).unwrap();
        assert_eq!(
            q.page_op_arg("offset").unwrap_err().source(),
            Some(&crate::ErrorSource::Parameter("opArg[offset]".to_owned()))
        );
    }

    mod filter {
        use std::collections::HashMap;

        use http::StatusCode;
        use serde::Deserialize;

        use super::super::*;
        use crate::{ErrorSource, links::pagination_links};

        #[derive(Debug, Default, PartialEq, Deserialize)]
        struct ListTasksQuery {
            status: Option<String>,
            epic_id: Option<String>,
        }

        #[derive(Debug, PartialEq, Deserialize)]
        #[serde(rename_all = "snake_case")]
        enum Priority {
            High,
            Low,
        }

        #[derive(Debug, PartialEq, Deserialize)]
        struct Slug(String);

        #[derive(Debug, PartialEq, Deserialize)]
        struct Typed {
            done: Option<bool>,
            min: Option<u32>,
            score: Option<f64>,
            priority: Option<Priority>,
            initial: Option<char>,
            big: Option<u128>,
            slug: Option<Slug>,
            owner: String,
            limit: u8,
        }

        const TASKS: QuerySpec = QuerySpec {
            filter: &["skill_id"],
            filter_fields: Some(filter_fields::<ListTasksQuery>),
            sort: true,
            include: true,
            page: true,
            ..QuerySpec::NONE
        };
        const TYPED: QuerySpec = QuerySpec { filter_fields: Some(filter_fields::<Typed>), ..QuerySpec::NONE };

        fn parse(raw: &str, spec: &QuerySpec) -> QueryParams {
            QueryParams::parse(Some(raw), spec).unwrap()
        }

        fn parameter(err: &ErrorObject) -> String {
            assert_eq!((err.status(), err.code()), (StatusCode::BAD_REQUEST, "invalid_query_parameter"));
            match err.source() {
                Some(ErrorSource::Parameter(p)) => p.clone(),
                other => panic!("expected a parameter source, got {other:?}"),
            }
        }

        fn rejected(raw: &str, spec: &QuerySpec) -> String {
            parameter(&QueryParams::parse(Some(raw), spec).unwrap_err())
        }

        fn filter_failure<T: DeserializeOwned + std::fmt::Debug>(raw: &str, spec: &QuerySpec) -> (String, String) {
            let err = parse(raw, spec).filter::<T>().unwrap_err();
            (parameter(&err), err.detail().to_owned())
        }

        #[test]
        fn struct_fields_are_the_names_serde_declares() {
            #[derive(Deserialize)]
            #[allow(dead_code)]
            struct Renamed {
                #[serde(rename = "epic")]
                epic_id: String,
                done: Option<bool>,
            }
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            #[allow(dead_code)]
            struct Camel {
                epic_id: Option<String>,
                skill_id: String,
            }
            assert_eq!(filter_fields::<ListTasksQuery>(), ["status", "epic_id"]);
            assert_eq!(filter_fields::<Renamed>(), ["epic", "done"]);
            assert_eq!(filter_fields::<Camel>(), ["epicId", "skillId"]);
        }

        #[test]
        fn a_type_that_is_not_a_struct_has_no_fields() {
            #[derive(Deserialize)]
            #[allow(dead_code)]
            struct Newtype(ListTasksQuery);
            #[derive(Deserialize)]
            #[allow(dead_code)]
            struct Flattened {
                #[serde(flatten)]
                inner: ListTasksQuery,
            }
            assert!(filter_fields::<HashMap<String, String>>().is_empty());
            assert!(filter_fields::<Priority>().is_empty());
            assert!(filter_fields::<Newtype>().is_empty());
            assert!(filter_fields::<Flattened>().is_empty());
            assert!(filter_fields::<String>().is_empty());
            assert!(filter_fields::<Option<ListTasksQuery>>().is_empty());
        }

        #[test]
        fn struct_members_and_bare_members_are_accepted() {
            let q = parse("filter[status]=open&filter[epic_id]=e&filter[skill_id]=s&page[limit]=5", &TASKS);
            assert_eq!(q.filters().unwrap(), vec![("epic_id", "e"), ("skill_id", "s"), ("status", "open")]);
            assert_eq!(rejected("filter[bogus]=1", &TASKS), "filter[bogus]");
            // Only bare members, or only struct members.
            assert_eq!(rejected("filter[status]=1", &QuerySpec { filter_fields: None, ..TASKS }), "filter[status]");
            assert_eq!(rejected("filter[skill_id]=1", &QuerySpec { filter: &[], ..TASKS }), "filter[skill_id]");
        }

        #[test]
        fn an_unknown_member_is_rejected_in_request_order() {
            assert_eq!(rejected("filter[bogus]=1&nope=2", &TASKS), "filter[bogus]");
            assert_eq!(rejected("nope=2&filter[bogus]=1", &TASKS), "nope");
            assert_eq!(rejected("filter[status]=a&filter[zzz]=1&filter[aaa]=2", &TASKS), "filter[zzz]");
            // A rejected name wins over a repeat or a bad value seen first.
            assert_eq!(rejected("filter[status]=a&filter[status]=b&filter[x]=1", &TASKS), "filter[x]");
        }

        #[test]
        fn nested_array_and_dotted_members_are_rejected() {
            assert_eq!(rejected("filter[status][]=a", &TASKS), "filter[status][]");
            assert_eq!(rejected("filter[status][x]=a", &TASKS), "filter[status][x]");
            assert_eq!(rejected("filter[status.x]=a", &TASKS), "filter[status.x]");
            assert_eq!(rejected("filter%5Bstatus%5D%5B%5D=a", &TASKS), "filter[status][]");
        }

        #[test]
        fn any_filter_is_rejected_on_a_route_that_takes_none() {
            let unfiltered = QuerySpec { sort: true, include: true, page: true, ..QuerySpec::NONE };
            assert_eq!(rejected("filter[status]=a", &unfiltered), "filter[status]");
            assert_eq!(rejected("filter[status]=a", &QuerySpec::NONE), "filter[status]");
            // A struct with no fields accepts nothing.
            let empty = QuerySpec { filter_fields: Some(filter_fields::<HashMap<String, String>>), ..QuerySpec::NONE };
            assert_eq!(rejected("filter[status]=a", &empty), "filter[status]");
        }

        #[test]
        fn the_struct_reads_its_members() {
            let q = parse("filter%5Bstatus%5D=closed%2Fdone&filter[epic_id]=a+b&filter[skill_id]=s", &TASKS);
            assert_eq!(
                q.filter::<ListTasksQuery>().unwrap(),
                ListTasksQuery { status: Some("closed/done".to_owned()), epic_id: Some("a b".to_owned()) }
            );
            assert_eq!(parse("", &TASKS).filter::<ListTasksQuery>().unwrap(), ListTasksQuery::default());
            assert_eq!(
                parse("filter[status]=", &TASKS).filter::<ListTasksQuery>().unwrap(),
                ListTasksQuery { status: Some(String::new()), epic_id: None }
            );
        }

        #[test]
        fn struct_values_read_as_a_form_field() {
            let q = parse(
                "filter[done]=true&filter[min]=5&filter[score]=1.5&filter[priority]=high&filter[initial]=x\
                 &filter[big]=340282366920938463463374607431768211455&filter[slug]=a%2Fb&filter[owner]=5&filter[limit]=7",
                &TYPED,
            );
            assert_eq!(
                q.filter::<Typed>().unwrap(),
                Typed {
                    done: Some(true),
                    min: Some(5),
                    score: Some(1.5),
                    priority: Some(Priority::High),
                    initial: Some('x'),
                    big: Some(u128::MAX),
                    slug: Some(Slug("a/b".to_owned())),
                    owner: "5".to_owned(),
                    limit: 7,
                }
            );
            let q = parse("filter[owner]=&filter[limit]=0", &TYPED);
            let typed = q.filter::<Typed>().unwrap();
            assert_eq!((typed.owner.as_str(), typed.limit, typed.done, typed.priority), ("", 0, None, None));
        }

        #[test]
        fn a_value_serde_rejects_names_its_member() {
            for (raw, member) in [
                ("filter[done]=yes", "filter[done]"),
                ("filter[min]=-1", "filter[min]"),
                ("filter[min]=", "filter[min]"),
                ("filter[score]=x", "filter[score]"),
                ("filter[priority]=medium", "filter[priority]"),
                ("filter[initial]=xy", "filter[initial]"),
                ("filter[limit]=256", "filter[limit]"),
            ] {
                let (parameter, _) = filter_failure::<Typed>(&format!("{raw}&filter[owner]=o&filter[limit]=1"), &TYPED);
                assert_eq!(parameter, member, "{raw}");
            }
            let (parameter, detail) =
                filter_failure::<Typed>("filter[done]=yes&filter[owner]=o&filter[limit]=1", &TYPED);
            assert_eq!(parameter, "filter[done]");
            assert_eq!(detail, "`filter[done]` is invalid: provided string was not `true` or `false`");
            let (_, detail) =
                filter_failure::<Typed>("filter[priority]=medium&filter[owner]=o&filter[limit]=1", &TYPED);
            assert_eq!(detail, "`filter[priority]` is invalid: unknown variant `medium`, expected `high` or `low`");
        }

        #[test]
        fn a_missing_required_field_names_its_member() {
            let (parameter, detail) = filter_failure::<Typed>("filter[limit]=1", &TYPED);
            assert_eq!((parameter.as_str(), detail.as_str()), ("filter[owner]", "`filter[owner]` is required"));
            // The first missing field in declaration order, as serde reports it.
            assert_eq!(filter_failure::<Typed>("", &TYPED).0, "filter[owner]");
            assert_eq!(filter_failure::<Typed>("filter[owner]=o", &TYPED).0, "filter[limit]");
        }

        #[test]
        fn struct_failures_come_in_byte_order_of_member_then_missing_fields() {
            // Two bad values: the first by byte order, not by request order.
            assert_eq!(
                filter_failure::<Typed>("filter[min]=x&filter[done]=x&filter[owner]=o", &TYPED).0,
                "filter[done]"
            );
            // A bad value before a repeat, and a repeat before a bad value.
            assert_eq!(filter_failure::<Typed>("filter[min]=1&filter[min]=2&filter[done]=x", &TYPED).0, "filter[done]");
            let (parameter, detail) = filter_failure::<Typed>("filter[min]=x&filter[done]=1&filter[done]=2", &TYPED);
            assert_eq!(
                (parameter.as_str(), detail.as_str()),
                ("filter[done]", "`filter[done]` is given more than once")
            );
            // A repeat is caught across spellings of the name.
            assert_eq!(filter_failure::<Typed>("filter[min]=1&filter%5Bmin%5D=1", &TYPED).0, "filter[min]");
            // Any member failure before a missing field.
            assert_eq!(filter_failure::<Typed>("filter[score]=x", &TYPED).0, "filter[score]");
        }

        #[test]
        fn the_struct_skips_bare_members() {
            // A repeated or unreadable bare member is not the struct's concern.
            let q = parse("filter[skill_id]=a&filter[skill_id]=b&filter[status]=s", &TASKS);
            assert_eq!(
                q.filter::<ListTasksQuery>().unwrap(),
                ListTasksQuery { status: Some("s".to_owned()), epic_id: None }
            );
        }

        #[test]
        fn a_failure_serde_ties_to_no_member_names_the_family() {
            #[derive(Deserialize)]
            struct Raw {
                min: Option<u32>,
                max: Option<u32>,
            }
            #[derive(Debug, Deserialize)]
            #[serde(try_from = "Raw")]
            #[allow(dead_code)]
            struct Range(Option<u32>, Option<u32>);
            impl TryFrom<Raw> for Range {
                type Error = &'static str;
                fn try_from(raw: Raw) -> Result<Self, Self::Error> {
                    match (raw.min, raw.max) {
                        (Some(min), Some(max)) if min > max => Err("min is above max"),
                        (min, max) => Ok(Range(min, max)),
                    }
                }
            }
            const RANGE: QuerySpec = QuerySpec { filter_fields: Some(filter_fields::<Range>), ..QuerySpec::NONE };
            assert!(parse("filter[min]=1&filter[max]=2", &RANGE).filter::<Range>().is_ok());
            let (parameter, detail) = filter_failure::<Range>("filter[min]=3&filter[max]=2", &RANGE);
            assert_eq!((parameter.as_str(), detail.as_str()), ("filter", "`filter` is invalid: min is above max"));
        }

        #[test]
        fn bare_members_read_as_a_form_field() {
            const SPEC: QuerySpec = QuerySpec { filter: &["skill_id", "done", "min", "empty"], ..QuerySpec::NONE };
            let q = parse("filter[skill_id]=a+b%2Fc&filter[done]=true&filter[min]=5&filter[empty]=", &SPEC);
            assert_eq!(q.filter_member::<String>("skill_id").unwrap(), Some("a b/c".to_owned()));
            assert_eq!(q.filter_member::<bool>("done").unwrap(), Some(true));
            assert_eq!(q.filter_member::<u32>("min").unwrap(), Some(5));
            assert_eq!(q.filter_member::<String>("empty").unwrap(), Some(String::new()));
            assert_eq!(q.required_filter_member::<u32>("min").unwrap(), 5);
            let q = parse("", &SPEC);
            assert_eq!(q.filter_member::<String>("skill_id").unwrap(), None);
            let err = q.required_filter_member::<String>("skill_id").unwrap_err();
            assert_eq!(
                (parameter(&err).as_str(), err.detail()),
                ("filter[skill_id]", "`filter[skill_id]` is required")
            );
        }

        #[test]
        fn a_bad_or_repeated_bare_member_names_itself() {
            const SPEC: QuerySpec = QuerySpec { filter: &["done", "min"], ..QuerySpec::NONE };
            let q = parse("filter[done]=yes&filter[min]=1&filter[min]=2", &SPEC);
            let err = q.filter_member::<bool>("done").unwrap_err();
            assert_eq!(parameter(&err), "filter[done]");
            assert_eq!(err.detail(), "`filter[done]` is invalid: provided string was not `true` or `false`");
            let err = q.required_filter_member::<u32>("min").unwrap_err();
            assert_eq!(
                (parameter(&err).as_str(), err.detail()),
                ("filter[min]", "`filter[min]` is given more than once")
            );
            assert_eq!(parameter(&q.filter_member::<u32>("min").unwrap_err()), "filter[min]");
        }

        #[test]
        fn link_query_carries_every_member_as_sent() {
            let q = parse("filter[status]=closed/done&filter[epic_id]=markdown-backend", &TASKS);
            let mut link_query = q.link_query().unwrap();
            assert_eq!(
                link_query.set_page(0, 20).href("/api/tasks"),
                "/api/tasks?filter%5Bepic_id%5D=markdown-backend&filter%5Bstatus%5D=closed%2Fdone\
                 &page%5Boffset%5D=0&page%5Blimit%5D=20"
            );
            let links = pagination_links("/api/tasks", &q.link_query().unwrap(), 0, 20, 1);
            let expected = "/api/tasks?filter%5Bepic_id%5D=markdown-backend&filter%5Bstatus%5D=closed%2Fdone\
                            &page%5Boffset%5D=0&page%5Blimit%5D=20";
            assert_eq!(links.self_link(), expected);
            let pagination = links.pagination().unwrap();
            assert_eq!((pagination.first.as_str(), pagination.last.as_str()), (expected, expected));
            // Bare members too, values decoded as the request gave them.
            let q = parse("filter[skill_id]=a+b&filter%5Bstatus%5D=", &TASKS);
            assert_eq!(q.link_query().unwrap().query_string(), "filter%5Bskill_id%5D=a%20b&filter%5Bstatus%5D=");
            assert_eq!(parse("", &TASKS).link_query().unwrap().href("/api/tasks"), "/api/tasks");
        }

        #[test]
        fn link_query_fails_on_the_first_repeat_in_byte_order() {
            let q = parse("filter[status]=a&filter[status]=b&filter[epic_id]=1&filter[epic_id]=2", &TASKS);
            assert_eq!(parameter(&q.link_query().unwrap_err()), "filter[epic_id]");
        }
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
