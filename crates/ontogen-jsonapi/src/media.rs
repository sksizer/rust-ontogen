//! Media-type negotiation (§3.2).

use http::{HeaderMap, header};

use crate::{
    MEDIA_TYPE,
    error::{ErrorCode, ErrorObject},
};

/// Whether an `Accept` field value admits a JSON:API response (§3.2).
///
/// `None` is an absent header. Comparison of types, parameter names and `q`
/// is case-insensitive, and a range with `q=0` is not acceptable. A range
/// that does not parse, or whose `q` is malformed, admits nothing.
pub fn is_acceptable(accept: Option<&str>) -> bool {
    let Some(accept) = accept else { return true };
    let ranges: Vec<MediaType> = split_unquoted(accept, b',').into_iter().filter_map(MediaType::parse).collect();
    let mut jsonapi = ranges.iter().filter(|r| r.essence == MEDIA_TYPE).peekable();
    if jsonapi.peek().is_some() {
        // Wildcards do not rescue a request whose every JSON:API instance is
        // unusable: the spec requires 406 then.
        return jsonapi.any(|r| r.q_nonzero() && r.params.iter().all(|(name, _)| name == "profile" || name == "q"));
    }
    ranges.iter().any(|r| (r.essence == "*/*" || r.essence == "application/*") && r.q_nonzero())
}

/// Step 2 of §13.2: `406 not_acceptable` unless the request's `Accept`
/// admits a JSON:API response. Repeated `Accept` fields are one list.
pub fn check_accept(headers: &HeaderMap) -> Result<(), ErrorObject> {
    let values: Vec<String> =
        headers.get_all(header::ACCEPT).iter().map(|v| String::from_utf8_lossy(v.as_bytes()).into_owned()).collect();
    let accept = if values.is_empty() { None } else { Some(values.join(",")) };
    if is_acceptable(accept.as_deref()) {
        Ok(())
    } else {
        Err(ErrorObject::new(
            ErrorCode::NotAcceptable,
            format!("this server responds only with {MEDIA_TYPE}, with no parameters other than profile"),
        )
        .with_header("Accept"))
    }
}

/// Step 3 of §13.2: `415 unsupported_media_type` unless a request that
/// carries a body declares `application/vnd.api+json`, optionally with
/// `profile`. `content_type` is the header's value, `None` when absent.
///
/// A request without a body passes whatever it declares (§10.2).
pub fn check_content_type(content_type: Option<&str>, has_body: bool) -> Result<(), ErrorObject> {
    if !has_body {
        return Ok(());
    }
    let unsupported =
        |detail: String| Err(ErrorObject::new(ErrorCode::UnsupportedMediaType, detail).with_header("Content-Type"));
    let Some(raw) = content_type else {
        return unsupported(format!("a request body must be sent as {MEDIA_TYPE}; Content-Type is missing"));
    };
    let Some(media) = MediaType::parse(raw) else {
        return unsupported(format!("a request body must be sent as {MEDIA_TYPE}; `{raw}` is not a media type"));
    };
    if media.essence != MEDIA_TYPE {
        return unsupported(format!("a request body must be sent as {MEDIA_TYPE}, not {}", media.essence));
    }
    match media.params.iter().find(|(name, _)| name != "profile") {
        None => Ok(()),
        Some((name, _)) if name == "ext" => unsupported("this server supports no JSON:API extension".to_owned()),
        Some((name, _)) => unsupported(format!("{MEDIA_TYPE} does not take the parameter `{name}`")),
    }
}

/// [`check_content_type`] over request headers. A repeated `Content-Type`
/// field is ambiguous and is refused.
pub fn check_content_type_headers(headers: &HeaderMap, has_body: bool) -> Result<(), ErrorObject> {
    let mut values = headers.get_all(header::CONTENT_TYPE).iter();
    let first = values.next().map(|v| String::from_utf8_lossy(v.as_bytes()).into_owned());
    if has_body && values.next().is_some() {
        return Err(ErrorObject::new(ErrorCode::UnsupportedMediaType, "Content-Type is given more than once")
            .with_header("Content-Type"));
    }
    check_content_type(first.as_deref(), has_body)
}

/// A parsed media type or media range. `essence` (`type/subtype`) and
/// parameter names are lowercased; values are unquoted.
struct MediaType {
    essence: String,
    params: Vec<(String, String)>,
}

impl MediaType {
    fn parse(s: &str) -> Option<MediaType> {
        let mut parts = split_unquoted(s, b';').into_iter();
        let essence = parts.next()?.trim().to_ascii_lowercase();
        let (kind, subtype) = essence.split_once('/')?;
        if !is_token(kind) || !is_token(subtype) {
            return None;
        }
        // RFC 9110 §5.6.6 allows an empty slot between semicolons
        // (`application/vnd.api+json;`); it is not a parameter.
        let params = parts
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .map(|p| match p.split_once('=') {
                Some((name, value)) => (name.trim().to_ascii_lowercase(), unquote(value.trim())),
                None => (p.to_ascii_lowercase(), String::new()),
            })
            .collect();
        Some(MediaType { essence, params })
    }

    /// `q` defaults to 1. A malformed `q` counts as zero, so a range the
    /// client did not state clearly never makes a request acceptable.
    fn q_nonzero(&self) -> bool {
        match self.params.iter().find(|(name, _)| name == "q") {
            None => true,
            Some((_, value)) => parse_q(value).is_some_and(|q| q > 0),
        }
    }
}

/// An RFC 9110 qvalue in thousandths: `0[.ddd]` or `1[.000]`.
fn parse_q(value: &str) -> Option<u16> {
    let (int, frac) = match value.split_once('.') {
        Some((int, frac)) => (int, frac),
        None => (value, ""),
    };
    if frac.len() > 3 || !frac.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let thousandths = format!("{frac:0<3}").parse::<u16>().ok()?;
    match int {
        "0" => Some(thousandths),
        "1" if thousandths == 0 => Some(1000),
        _ => None,
    }
}

fn is_token(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
}

/// Splits on `delim` outside double-quoted strings, honouring backslash
/// escapes inside them, so a quoted `ext` or `profile` URI list cannot split
/// a range.
fn split_unquoted(s: &str, delim: u8) -> Vec<&str> {
    let mut parts = Vec::new();
    let (mut start, mut quoted, mut escaped) = (0, false, false);
    for (i, b) in s.bytes().enumerate() {
        if escaped {
            escaped = false;
        } else if quoted && b == b'\\' {
            escaped = true;
        } else if b == b'"' {
            quoted = !quoted;
        } else if !quoted && b == delim {
            parts.push(&s[start..i]);
            start = i + 1;
        }
    }
    parts.push(&s[start..]);
    parts
}

fn unquote(value: &str) -> String {
    let Some(inner) = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) else {
        return value.to_owned();
    };
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(next) = chars.next() {
                out.push(next);
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use http::{HeaderValue, StatusCode};

    use super::*;

    #[test]
    fn absent_accept_is_acceptable() {
        assert!(is_acceptable(None));
    }

    #[test]
    fn jsonapi_instances_decide_when_present() {
        assert!(is_acceptable(Some("application/vnd.api+json")));
        assert!(is_acceptable(Some(r#"application/vnd.api+json; profile="https://example.com/p""#)));
        assert!(is_acceptable(Some("application/vnd.api+json;q=0.5")));
        assert!(is_acceptable(Some(r#"application/vnd.api+json; profile="a b"; q=1"#)));
        // One usable instance among unusable ones is enough.
        assert!(is_acceptable(Some("application/vnd.api+json; charset=utf-8, application/vnd.api+json")));
        assert!(is_acceptable(Some("text/html, application/vnd.api+json")));
    }

    #[test]
    fn wildcards_do_not_rescue_unusable_jsonapi_instances() {
        // The three §3.2 examples.
        assert!(!is_acceptable(Some("application/vnd.api+json; charset=utf-8, */*")));
        assert!(!is_acceptable(Some(r#"application/vnd.api+json; ext="https://jsonapi.org/ext/atomic", */*"#)));
        assert!(!is_acceptable(Some("application/vnd.api+json;q=0, */*")));
        assert!(!is_acceptable(Some("application/vnd.api+json; ext=foo, application/*")));
    }

    #[test]
    fn comparison_is_case_insensitive() {
        assert!(is_acceptable(Some("Application/VND.API+JSON")));
        assert!(is_acceptable(Some(r#"application/vnd.api+json; PROFILE="x"; Q=0.9"#)));
        assert!(!is_acceptable(Some("application/vnd.api+json; Q=0, */*")));
        assert!(!is_acceptable(Some("APPLICATION/VND.API+JSON; CHARSET=utf-8")));
    }

    #[test]
    fn wildcards_satisfy_when_no_jsonapi_instance_is_listed() {
        assert!(is_acceptable(Some("*/*")));
        assert!(is_acceptable(Some("application/*")));
        assert!(is_acceptable(Some("text/html, */*;q=0.1")));
        assert!(is_acceptable(Some("APPLICATION/*")));
        assert!(!is_acceptable(Some("*/*;q=0")));
        assert!(!is_acceptable(Some("application/*;q=0.000")));
        assert!(!is_acceptable(Some("text/*")));
        assert!(!is_acceptable(Some("application/json")));
        assert!(!is_acceptable(Some("text/html")));
        assert!(!is_acceptable(Some("")));
    }

    #[test]
    fn quoted_commas_do_not_split_ranges() {
        assert!(!is_acceptable(Some(r#"application/vnd.api+json; ext="https://a, https://b", */*"#)));
        assert!(is_acceptable(Some(r#"application/vnd.api+json; profile="https://a, https://b""#)));
    }

    #[test]
    fn q_values() {
        assert_eq!(parse_q("0"), Some(0));
        assert_eq!(parse_q("0.000"), Some(0));
        assert_eq!(parse_q("0.001"), Some(1));
        assert_eq!(parse_q("0.5"), Some(500));
        assert_eq!(parse_q("1"), Some(1000));
        assert_eq!(parse_q("1.000"), Some(1000));
        assert_eq!(parse_q("1.5"), None);
        assert_eq!(parse_q("0.0001"), None);
        assert_eq!(parse_q("abc"), None);
        assert_eq!(parse_q(""), None);
        // A malformed q admits nothing.
        assert!(!is_acceptable(Some("*/*;q=high")));
    }

    #[test]
    fn check_accept_joins_repeated_fields_and_reports_the_header() {
        let mut headers = HeaderMap::new();
        assert!(check_accept(&headers).is_ok());
        headers.append(header::ACCEPT, HeaderValue::from_static("text/html"));
        let err = check_accept(&headers).unwrap_err();
        assert_eq!(err.status(), StatusCode::NOT_ACCEPTABLE);
        assert_eq!(err.code(), "not_acceptable");
        assert_eq!(err.source(), Some(&crate::ErrorSource::Header("Accept")));
        headers.append(header::ACCEPT, HeaderValue::from_static("application/vnd.api+json"));
        assert!(check_accept(&headers).is_ok());
    }

    fn content_type_status(ct: Option<&str>) -> Option<u16> {
        check_content_type(ct, true).err().map(|e| {
            assert_eq!(e.code(), "unsupported_media_type");
            assert_eq!(e.source(), Some(&crate::ErrorSource::Header("Content-Type")));
            e.status().as_u16()
        })
    }

    #[test]
    fn content_type_table() {
        // The §3.2 table, row by row.
        assert_eq!(content_type_status(Some("application/vnd.api+json")), None);
        assert_eq!(content_type_status(Some(r#"application/vnd.api+json; profile="https://example.com/p""#)), None);
        assert_eq!(
            content_type_status(Some(r#"application/vnd.api+json; ext="https://jsonapi.org/ext/atomic""#)),
            Some(415)
        );
        assert_eq!(content_type_status(Some("application/vnd.api+json; charset=utf-8")), Some(415));
        assert_eq!(content_type_status(Some("application/json")), Some(415));
        assert_eq!(content_type_status(None), Some(415));
        // And around it.
        assert_eq!(content_type_status(Some("Application/Vnd.Api+Json")), None);
        assert_eq!(content_type_status(Some("application/vnd.api+json; PROFILE=x")), None);
        assert_eq!(content_type_status(Some("application/vnd.api+json; profile=x; ext=y")), Some(415));
        assert_eq!(content_type_status(Some("application/vnd.api+json;q=1")), Some(415));
        assert_eq!(content_type_status(Some("text/plain")), Some(415));
        assert_eq!(content_type_status(Some("garbage")), Some(415));
        assert_eq!(content_type_status(Some("")), Some(415));
    }

    #[test]
    fn empty_parameter_slots_are_not_parameters() {
        assert_eq!(content_type_status(Some("application/vnd.api+json;")), None);
        assert_eq!(content_type_status(Some("application/vnd.api+json ; ; profile=x;")), None);
        assert_eq!(content_type_status(Some("application/vnd.api+json;; charset=utf-8")), Some(415));
        assert!(is_acceptable(Some("application/vnd.api+json;, */*")));
        assert!(is_acceptable(Some("application/vnd.api+json;;q=0.5")));
        assert!(!is_acceptable(Some("application/vnd.api+json;;q=0, */*")));
    }

    #[test]
    fn content_type_is_not_checked_without_a_body() {
        assert!(check_content_type(None, false).is_ok());
        assert!(check_content_type(Some("application/json"), false).is_ok());
    }

    #[test]
    fn repeated_content_type_is_refused() {
        let mut headers = HeaderMap::new();
        headers.append(header::CONTENT_TYPE, HeaderValue::from_static(MEDIA_TYPE));
        assert!(check_content_type_headers(&headers, true).is_ok());
        headers.append(header::CONTENT_TYPE, HeaderValue::from_static(MEDIA_TYPE));
        assert!(check_content_type_headers(&headers, true).is_err());
        assert!(check_content_type_headers(&headers, false).is_ok());
    }
}
