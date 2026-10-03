//! Reading request documents: the schema-independent rows of step 7 of
//! §13.2 (§8.2, §8.3, §9.2).
//!
//! The functions here check, in table order, every rule that does not need
//! the schema, and hand the rest of the document to generated code, which
//! checks attributes and relationships against the entity. Each function
//! stops at the first failure, because a response carries one error
//! (§13.1).

use std::{collections::HashSet, fmt::Display, hash::Hash};

use serde_json::{Map, Value};

use crate::error::{ErrorCode, ErrorObject};

/// The resource endpoint a document was sent to: its resource type and its
/// path, used in error details.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Endpoint<'a> {
    /// The resource type the endpoint holds (`tasks`).
    pub type_name: &'a str,
    /// The request path (`/api/tasks`, `/api/tasks/ship-the-emitter`).
    pub path: &'a str,
}

/// The parts of a create or update document's `data` that generated code
/// checks against the schema.
#[derive(Debug, Clone, PartialEq)]
pub struct ResourceData {
    /// `data.id`: on create the client id if one was sent; on update the
    /// URL id, which the body matched.
    pub id: Option<String>,
    /// `data.attributes`, when present.
    pub attributes: Option<Map<String, Value>>,
    relationships: Option<Value>,
}

impl ResourceData {
    /// `data.relationships`, when present.
    ///
    /// Its shape is checked here rather than when the document is read,
    /// because the attribute checks come first in step 7 (§8.2): generated
    /// code checks the attributes, then calls this.
    pub fn relationships(&self) -> Result<Option<&Map<String, Value>>, ErrorObject> {
        match &self.relationships {
            None => Ok(None),
            Some(Value::Object(map)) => Ok(Some(map)),
            Some(_) => Err(invalid_document("`data.relationships` must be an object", "/data/relationships")),
        }
    }
}

/// Reads a request body as a JSON object: the first row of every step-7
/// table. An empty body is not JSON.
pub fn parse_object(body: &[u8]) -> Result<Map<String, Value>, ErrorObject> {
    match serde_json::from_slice::<Value>(body) {
        Ok(Value::Object(map)) => Ok(map),
        Ok(_) => Err(ErrorObject::new(ErrorCode::InvalidDocument, "the request body must be a JSON object")),
        Err(err) => {
            Err(ErrorObject::new(ErrorCode::InvalidDocument, format!("the request body is not valid JSON: {err}")))
        }
    }
}

/// Reads a create document (`POST /api/{type}`, §8.2) up to the attribute
/// checks.
///
/// `check_id` validates a client `data.id` against the id-validity rule;
/// its error becomes `400 invalid_document` at `/data/id`, ahead of the
/// `lid` and attribute rows as the table orders them. Generated code passes
/// the shared rule from `ontogen-core`.
pub fn parse_create<F, E>(body: &[u8], endpoint: Endpoint<'_>, check_id: F) -> Result<ResourceData, ErrorObject>
where
    F: FnOnce(&str) -> Result<(), E>,
    E: Display,
{
    let mut data = read_data(body, endpoint, "created")?;
    let id = match data.remove("id") {
        None => None,
        Some(Value::String(id)) => {
            check_id(&id).map_err(|e| invalid_document(format!("`{id}` is not a valid id: {e}"), "/data/id"))?;
            Some(id)
        }
        Some(_) => return Err(invalid_document("`data.id` must be a string", "/data/id")),
    };
    rest(data, id)
}

/// Reads an update document (`PATCH /api/{type}/{id}`, §8.3) up to the
/// attribute checks. The body id must equal `url_id` exactly; neither is
/// checked against the id-validity rule.
pub fn parse_update(body: &[u8], endpoint: Endpoint<'_>, url_id: &str) -> Result<ResourceData, ErrorObject> {
    let mut data = read_data(body, endpoint, "updated")?;
    let id = match data.remove("id") {
        None => return Err(invalid_document("`data.id` is required to update a resource", "/data")),
        Some(Value::String(id)) => id,
        Some(_) => return Err(invalid_document("`data.id` must be a string", "/data/id")),
    };
    if id != url_id {
        return Err(ErrorObject::new(
            ErrorCode::IdMismatch,
            format!("body id `{id}` does not match URL id `{url_id}`"),
        )
        .with_pointer("/data/id"));
    }
    rest(data, Some(id))
}

/// The rows shared by create and update, up to and including
/// `type_mismatch`.
fn read_data(body: &[u8], endpoint: Endpoint<'_>, verb: &str) -> Result<Map<String, Value>, ErrorObject> {
    let mut doc = parse_object(body)?;
    let data = match doc.remove("data") {
        None => return Err(invalid_document("the document has no `data` member", "")),
        Some(Value::Object(data)) => data,
        Some(_) => return Err(invalid_document("`data` must be a resource object", "/data")),
    };
    match data.get("type") {
        None => Err(invalid_document("`data` has no `type` member", "/data")),
        Some(Value::String(t)) if t == endpoint.type_name => Ok(data),
        Some(Value::String(t)) => Err(ErrorObject::new(
            ErrorCode::TypeMismatch,
            format!("`{t}` cannot be {verb} at {}, which holds `{}`", endpoint.path, endpoint.type_name),
        )
        .with_pointer("/data/type")),
        Some(_) => Err(invalid_document("`data.type` must be a string", "/data/type")),
    }
}

/// The rows after the id: `lid`, then the shape of `attributes`.
fn rest(mut data: Map<String, Value>, id: Option<String>) -> Result<ResourceData, ErrorObject> {
    if data.contains_key("lid") {
        return Err(invalid_document("`data.lid` is not supported", "/data/lid"));
    }
    let attributes = match data.remove("attributes") {
        None => None,
        Some(Value::Object(attributes)) => Some(attributes),
        Some(_) => return Err(invalid_document("`data.attributes` must be an object", "/data/attributes")),
    };
    Ok(ResourceData { id, attributes, relationships: data.remove("relationships") })
}

/// Reads a to-one relationship object at `pointer` (`/data/relationships/epic`
/// in a resource document, `""` for a relationship endpoint's whole body),
/// returning the linked id or `None` for `null`.
///
/// Checks, in order (§8.2): the object and its `data` member; the arity
/// (`null` or an identifier); the identifier's string `type` and `id`;
/// `null` when the relationship is not `nullable` (`403
/// relationship_required`); the identifier's type against `target_type`
/// (`409 type_mismatch`).
pub fn to_one(
    relationship: &Value,
    pointer: &str,
    target_type: &str,
    nullable: bool,
) -> Result<Option<String>, ErrorObject> {
    let data_pointer = format!("{pointer}/data");
    let identifier = match linkage(relationship, pointer)? {
        Value::Null => None,
        value @ Value::Object(_) => Some(identifier(value, &data_pointer)?),
        _ => {
            return Err(invalid_document(
                "a to-one relationship's `data` must be null or a resource identifier",
                data_pointer,
            ));
        }
    };
    match identifier {
        None if nullable => Ok(None),
        None => Err(ErrorObject::new(ErrorCode::RelationshipRequired, "this relationship cannot be emptied")
            .with_pointer(data_pointer)),
        Some((type_name, id)) => {
            check_type(type_name, target_type, &data_pointer)?;
            Ok(Some(id.to_owned()))
        }
    }
}

/// Reads a to-many relationship object at `pointer`, returning the linked
/// ids in request order with duplicates collapsed to their first occurrence
/// (§5.4).
///
/// Checks, in order: the object and its `data` member; the arity (an
/// array); every identifier's string `type` and `id`; then every
/// identifier's type against `target_type`. Pointers name the request's
/// array index.
pub fn to_many(relationship: &Value, pointer: &str, target_type: &str) -> Result<Vec<String>, ErrorObject> {
    let data_pointer = format!("{pointer}/data");
    let Value::Array(items) = linkage(relationship, pointer)? else {
        return Err(invalid_document("a to-many relationship's `data` must be an array", data_pointer));
    };
    let identifiers = items
        .iter()
        .enumerate()
        .map(|(i, item)| identifier(item, &format!("{data_pointer}/{i}")))
        .collect::<Result<Vec<_>, _>>()?;
    for (i, (type_name, _)) in identifiers.iter().enumerate() {
        check_type(type_name, target_type, &format!("{data_pointer}/{i}"))?;
    }
    Ok(collapse_duplicates(identifiers.into_iter().map(|(_, id)| id.to_owned()).collect()))
}

/// Keeps the first occurrence of each item, in order (§5.4).
pub fn collapse_duplicates<T: Eq + Hash>(items: Vec<T>) -> Vec<T> {
    let keep: Vec<bool> = {
        let mut seen = HashSet::with_capacity(items.len());
        items.iter().map(|item| seen.insert(item)).collect()
    };
    items.into_iter().zip(keep).filter_map(|(item, keep)| keep.then_some(item)).collect()
}

/// Appends `token` to an RFC 6901 pointer, escaping `~` and `/`, for a
/// pointer that names a client-chosen member such as an unknown attribute.
pub fn pointer(base: &str, token: &str) -> String {
    format!("{base}/{}", token.replace('~', "~0").replace('/', "~1"))
}

fn linkage<'a>(relationship: &'a Value, pointer: &str) -> Result<&'a Value, ErrorObject> {
    match relationship {
        Value::Object(rel) => {
            rel.get("data").ok_or_else(|| invalid_document("the relationship has no `data` member", pointer))
        }
        _ => Err(invalid_document("a relationship must be an object", pointer)),
    }
}

fn identifier<'a>(value: &'a Value, pointer: &str) -> Result<(&'a str, &'a str), ErrorObject> {
    let fields = value.as_object().and_then(|o| Some((o.get("type")?.as_str()?, o.get("id")?.as_str()?)));
    fields.ok_or_else(|| invalid_document("a resource identifier needs a string `type` and a string `id`", pointer))
}

fn check_type(type_name: &str, target_type: &str, pointer: &str) -> Result<(), ErrorObject> {
    if type_name == target_type {
        Ok(())
    } else {
        Err(ErrorObject::new(
            ErrorCode::TypeMismatch,
            format!("`{type_name}` cannot be linked here; expected `{target_type}`"),
        )
        .with_pointer(pointer))
    }
}

fn invalid_document(detail: impl Into<String>, pointer: impl Into<String>) -> ErrorObject {
    ErrorObject::new(ErrorCode::InvalidDocument, detail).with_pointer(pointer)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const TASKS: Endpoint<'static> = Endpoint { type_name: "tasks", path: "/api/tasks" };
    const TASK: Endpoint<'static> = Endpoint { type_name: "tasks", path: "/api/tasks/ship-the-emitter" };

    fn any_id(_: &str) -> Result<(), &'static str> {
        Ok(())
    }

    fn no_index(id: &str) -> Result<(), &'static str> {
        if id.eq_ignore_ascii_case("index") { Err("`index` is reserved") } else { Ok(()) }
    }

    /// (status, code, pointer) of a failure.
    fn failure(result: Result<ResourceData, ErrorObject>) -> (u16, String, Option<String>) {
        let err = result.unwrap_err();
        let pointer = match err.source() {
            None => None,
            Some(crate::ErrorSource::Pointer(p)) => Some(p.clone()),
            Some(other) => panic!("unexpected source {other:?}"),
        };
        (err.status().as_u16(), err.code().to_owned(), pointer)
    }

    fn create(body: &str) -> (u16, String, Option<String>) {
        failure(parse_create(body.as_bytes(), TASKS, no_index))
    }

    fn update(body: &str) -> (u16, String, Option<String>) {
        failure(parse_update(body.as_bytes(), TASK, "ship-the-emitter"))
    }

    fn bad(pointer: &str) -> (u16, String, Option<String>) {
        (400, "invalid_document".to_owned(), Some(pointer.to_owned()))
    }

    #[test]
    fn create_rows_in_table_order() {
        let no_source = (400, "invalid_document".to_owned(), None);
        assert_eq!(create(""), no_source);
        assert_eq!(create("{"), no_source);
        assert_eq!(create("[]"), no_source);
        assert_eq!(create(r#""data""#), no_source);
        assert_eq!(create("{}"), bad(""));
        assert_eq!(create(r#"{"data":[]}"#), bad("/data"));
        assert_eq!(create(r#"{"data":null}"#), bad("/data"));
        assert_eq!(create(r#"{"data":{}}"#), bad("/data"));
        assert_eq!(create(r#"{"data":{"type":1}}"#), bad("/data/type"));
        assert_eq!(
            create(r#"{"data":{"type":"epics"}}"#),
            (409, "type_mismatch".to_owned(), Some("/data/type".to_owned()))
        );
        assert_eq!(create(r#"{"data":{"type":"tasks","id":7}}"#), bad("/data/id"));
        assert_eq!(create(r#"{"data":{"type":"tasks","id":null}}"#), bad("/data/id"));
        assert_eq!(create(r#"{"data":{"type":"tasks","id":"Index"}}"#), bad("/data/id"));
        assert_eq!(create(r#"{"data":{"type":"tasks","lid":"x"}}"#), bad("/data/lid"));
        assert_eq!(create(r#"{"data":{"type":"tasks","attributes":[]}}"#), bad("/data/attributes"));
        assert_eq!(create(r#"{"data":{"type":"tasks","attributes":null}}"#), bad("/data/attributes"));
    }

    #[test]
    fn deep_nesting_is_an_invalid_document_not_a_stack_overflow() {
        let body = format!(r#"{{"data":{}{}}}"#, "[".repeat(100_000), "]".repeat(100_000));
        assert_eq!(create(&body), (400, "invalid_document".to_owned(), None));
    }

    #[test]
    fn earlier_rows_win_over_later_ones() {
        // An invalid id beats a lid; a type mismatch beats both.
        assert_eq!(create(r#"{"data":{"type":"tasks","id":"index","lid":"x"}}"#), bad("/data/id"));
        assert_eq!(create(r#"{"data":{"type":"epics","id":5,"lid":"x"}}"#).0, 409);
        assert_eq!(create(r#"{"data":{"type":"tasks","lid":"x","attributes":1}}"#), bad("/data/lid"));
    }

    #[test]
    fn relationships_shape_is_checked_on_demand() {
        let data = parse_create(br#"{"data":{"type":"tasks","attributes":{"x":1},"relationships":[]}}"#, TASKS, any_id)
            .unwrap();
        assert_eq!(data.attributes, Some(json!({"x": 1}).as_object().unwrap().clone()));
        assert_eq!(failure(data.relationships().map(|_| data.clone())), bad("/data/relationships"));
        let data = parse_create(br#"{"data":{"type":"tasks"}}"#, TASKS, any_id).unwrap();
        assert_eq!(data.relationships().unwrap(), None);
        assert_eq!(data.attributes, None);
        assert_eq!(data.id, None);
    }

    #[test]
    fn create_returns_the_client_id_and_ignores_other_members() {
        let data = parse_create(
            br#"{"jsonapi":{"version":"1.1"},"meta":{},"data":{"type":"tasks","id":"review-q3","meta":{},"links":{},"relationships":{"epic":{"data":null}}}}"#,
            TASKS,
            any_id,
        )
        .unwrap();
        assert_eq!(data.id.as_deref(), Some("review-q3"));
        assert!(data.relationships().unwrap().unwrap().contains_key("epic"));
    }

    #[test]
    fn type_mismatch_document_matches_the_contract_example() {
        let err = parse_create(br#"{"data":{"type":"epics"}}"#, TASKS, any_id).unwrap_err();
        assert_eq!(
            serde_json::to_string(&err.document()).unwrap(),
            concat!(
                r#"{"jsonapi":{"version":"1.1"},"errors":[{"status":"409","code":"type_mismatch","title":"Conflict","#,
                r#""detail":"`epics` cannot be created at /api/tasks, which holds `tasks`","source":{"pointer":"/data/type"}}]}"#
            )
        );
    }

    #[test]
    fn update_rows_in_table_order() {
        assert_eq!(update("{}"), bad(""));
        assert_eq!(update(r#"{"data":{"type":"epics","id":"other"}}"#).0, 409);
        assert_eq!(update(r#"{"data":{"type":"tasks"}}"#), bad("/data"));
        assert_eq!(update(r#"{"data":{"type":"tasks","id":1}}"#), bad("/data/id"));
        assert_eq!(
            update(r#"{"data":{"type":"tasks","id":""}}"#),
            (409, "id_mismatch".to_owned(), Some("/data/id".to_owned()))
        );
        assert_eq!(update(r#"{"data":{"type":"tasks","id":"Ship-the-emitter"}}"#).1, "id_mismatch");
        assert_eq!(update(r#"{"data":{"type":"tasks","id":"ship-the-emitter","lid":"x"}}"#), bad("/data/lid"));
        assert_eq!(
            update(r#"{"data":{"type":"tasks","id":"ship-the-emitter","attributes":"x"}}"#),
            bad("/data/attributes")
        );
    }

    #[test]
    fn update_takes_the_url_id_and_does_not_validate_it() {
        let data = parse_update(
            br#"{"data":{"type":"tasks","id":"index"}}"#,
            Endpoint { path: "/api/tasks/index", ..TASK },
            "index",
        )
        .unwrap();
        assert_eq!(data.id.as_deref(), Some("index"));
    }

    #[test]
    fn id_mismatch_document_matches_the_contract_example() {
        let err = parse_update(br#"{"data":{"type":"tasks","id":"other"}}"#, TASK, "ship-the-emitter").unwrap_err();
        assert_eq!(
            serde_json::to_string(&err.document()).unwrap(),
            concat!(
                r#"{"jsonapi":{"version":"1.1"},"errors":[{"status":"409","code":"id_mismatch","title":"Conflict","#,
                r#""detail":"body id `other` does not match URL id `ship-the-emitter`","source":{"pointer":"/data/id"}}]}"#
            )
        );
    }

    fn rel_failure<T: std::fmt::Debug>(result: Result<T, ErrorObject>) -> (u16, String, String) {
        let err = result.unwrap_err();
        match err.source() {
            Some(crate::ErrorSource::Pointer(p)) => (err.status().as_u16(), err.code().to_owned(), p.clone()),
            other => panic!("unexpected source {other:?}"),
        }
    }

    const EPIC: &str = "/data/relationships/epic";
    const TAGS: &str = "/data/relationships/tags";

    #[test]
    fn to_one_linkage() {
        let read = |v: Value, nullable| to_one(&v, EPIC, "epics", nullable);
        assert_eq!(
            read(json!({"data": {"type": "epics", "id": "markdown-backend"}}), true).unwrap().as_deref(),
            Some("markdown-backend")
        );
        assert_eq!(read(json!({"data": null}), true).unwrap(), None);
        let p = |s: &str| format!("{EPIC}{s}");
        let bad = |s: &str| (400, "invalid_document".to_owned(), p(s));
        assert_eq!(rel_failure(read(json!("epics"), true)), bad(""));
        assert_eq!(rel_failure(read(json!({}), true)), bad(""));
        assert_eq!(rel_failure(read(json!({"links": {}}), true)), bad(""));
        assert_eq!(rel_failure(read(json!({"data": []}), true)), bad("/data"));
        assert_eq!(rel_failure(read(json!({"data": "x"}), true)), bad("/data"));
        assert_eq!(rel_failure(read(json!({"data": {"type": "epics"}}), true)), bad("/data"));
        assert_eq!(rel_failure(read(json!({"data": {"type": "epics", "id": 1}}), true)), bad("/data"));
        assert_eq!(
            rel_failure(read(json!({"data": null}), false)),
            (403, "relationship_required".to_owned(), p("/data"))
        );
        assert_eq!(
            rel_failure(read(json!({"data": {"type": "tags", "id": "x"}}), true)),
            (409, "type_mismatch".to_owned(), p("/data"))
        );
    }

    #[test]
    fn to_many_linkage() {
        let read = |v: Value| to_many(&v, TAGS, "tags");
        assert_eq!(read(json!({"data": []})).unwrap(), Vec::<String>::new());
        assert_eq!(
            read(
                json!({"data": [{"type": "tags", "id": "b"}, {"type": "tags", "id": "a"}, {"type": "tags", "id": "b"}]})
            )
            .unwrap(),
            vec!["b", "a"]
        );
        let p = |s: &str| format!("{TAGS}{s}");
        let bad = |s: &str| (400, "invalid_document".to_owned(), p(s));
        assert_eq!(rel_failure(read(json!([]))), bad(""));
        assert_eq!(rel_failure(read(json!({"data": null}))), bad("/data"));
        assert_eq!(rel_failure(read(json!({"data": {"type": "tags", "id": "a"}}))), bad("/data"));
        assert_eq!(rel_failure(read(json!({"data": [{"type": "tags", "id": "a"}, {"id": "b"}]}))), bad("/data/1"));
        assert_eq!(
            rel_failure(read(json!({"data": [{"type": "tags", "id": "a"}, {"type": "epics", "id": "b"}]}))),
            (409, "type_mismatch".to_owned(), p("/data/1"))
        );
        // Every identifier's shape is checked before any identifier's type.
        assert_eq!(
            rel_failure(read(json!({"data": [{"type": "epics", "id": "a"}, {"type": "tags"}]}))),
            bad("/data/1")
        );
    }

    #[test]
    fn relationship_endpoint_bodies_use_the_empty_pointer() {
        // §9.2: the whole body is the relationship object.
        assert_eq!(rel_failure(to_many(&json!({}), "", "tags")), (400, "invalid_document".to_owned(), String::new()));
        assert_eq!(rel_failure(to_many(&json!({"data": {}}), "", "tags")).2, "/data");
        assert_eq!(rel_failure(to_one(&json!({"data": [{"type": "epics", "id": "x"}]}), "", "epics", true)).2, "/data");
        assert_eq!(
            rel_failure(to_one(&json!({"data": null}), "", "epics", false)),
            (403, "relationship_required".to_owned(), "/data".to_owned())
        );
        assert_eq!(rel_failure(to_many(&json!({"data": [{"type": "x", "id": "1"}]}), "", "tags")).2, "/data/0");
    }

    #[test]
    fn duplicates_collapse_to_their_first_occurrence() {
        assert_eq!(collapse_duplicates(vec![3, 1, 3, 2, 1]), vec![3, 1, 2]);
        assert_eq!(collapse_duplicates(Vec::<u8>::new()), Vec::<u8>::new());
    }

    #[test]
    fn pointer_tokens_are_escaped() {
        assert_eq!(pointer("/data/attributes", "a/b~c"), "/data/attributes/a~1b~0c");
        assert_eq!(pointer("", "data"), "/data");
    }
}
