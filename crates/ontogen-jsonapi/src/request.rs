//! Reading request documents: the schema-independent rows of step 7 of
//! §13.2 (§8.2, §8.3, §9.2, §10.2).
//!
//! The functions here check, in table order, every rule that does not need
//! the schema, and hand the rest of the document to generated code, which
//! checks attributes and relationships against the entity, or a custom op's
//! arguments against its parameters. Each function stops at the first
//! failure, because a response carries one error (§13.1).

use std::{collections::HashSet, fmt::Display, hash::Hash};

use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use crate::{
    error::{ErrorCode, ErrorObject},
    path::LookupKey,
};

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

/// Reads a relationship endpoint's body (`PATCH`, `POST` or `DELETE` on
/// `/api/{type}/{id}/relationships/{rel}`, §9): a JSON object, returned whole.
///
/// The whole body is the relationship object, so the handler reads its
/// linkage with [`to_one`] or [`to_many_linked`] at pointer `""`: a missing
/// `data` is reported at `""`, and the identifiers at `/data` and
/// `/data/{i}`, as §9.2 places them.
pub fn parse_relationship(body: &[u8]) -> Result<Value, ErrorObject> {
    parse_object(body).map(Value::Object)
}

/// Reads a create document (`POST /api/{type}`, §8.2) up to the attribute
/// checks.
///
/// `check_id` validates a client `data.id` against the shared id-validity
/// rule (§8.2); its error becomes `400 invalid_document` at `/data/id`,
/// ahead of the `lid` and attribute rows as the table orders them.
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
/// checked against the id-validity rule. A `url_id` that does not decode
/// equals no body id, so it is a `409 id_mismatch` here and never reaches
/// the store.
pub fn parse_update(body: &[u8], endpoint: Endpoint<'_>, url_id: &LookupKey) -> Result<ResourceData, ErrorObject> {
    let mut data = read_data(body, endpoint, "updated")?;
    let id = match data.remove("id") {
        None => return Err(invalid_document("`data.id` is required to update a resource", "/data")),
        Some(Value::String(id)) => id,
        Some(_) => return Err(invalid_document("`data.id` must be a string", "/data/id")),
    };
    if url_id.as_str() != Some(id.as_str()) {
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
/// array); with `max_identifiers`, the array's length (`403
/// relationship_batch_unsupported`); every identifier's string `type` and
/// `id`; then every identifier's type against `target_type`. Pointers name
/// the request's array index.
///
/// A relationship `POST` or `DELETE` passes `Some(1)`, since each takes one
/// identifier (§9); a `PATCH` and a resource document pass `None`. The
/// length counts identifiers as sent, before duplicates collapse.
pub fn to_many(
    relationship: &Value,
    pointer: &str,
    target_type: &str,
    max_identifiers: Option<usize>,
) -> Result<Vec<String>, ErrorObject> {
    to_many_linked(relationship, pointer, target_type, max_identifiers)
        .map(|linked| linked.into_iter().map(|l| l.id).collect())
}

/// [`to_many`], keeping with each id the pointer of the identifier that
/// named it: its first occurrence, since duplicates collapse to it.
pub fn to_many_linked(
    relationship: &Value,
    pointer: &str,
    target_type: &str,
    max_identifiers: Option<usize>,
) -> Result<Vec<LinkedId>, ErrorObject> {
    let data_pointer = format!("{pointer}/data");
    let Value::Array(items) = linkage(relationship, pointer)? else {
        return Err(invalid_document("a to-many relationship's `data` must be an array", data_pointer));
    };
    if let Some(max) = max_identifiers.filter(|&max| items.len() > max) {
        return Err(ErrorObject::new(
            ErrorCode::RelationshipBatchUnsupported,
            format!("this request takes at most {max} identifier(s); {} were sent", items.len()),
        )
        .with_pointer(data_pointer));
    }
    let identifiers = items
        .iter()
        .enumerate()
        .map(|(i, item)| identifier(item, &format!("{data_pointer}/{i}")))
        .collect::<Result<Vec<_>, _>>()?;
    for (i, (type_name, _)) in identifiers.iter().enumerate() {
        check_type(type_name, target_type, &format!("{data_pointer}/{i}"))?;
    }
    let mut seen = HashSet::with_capacity(identifiers.len());
    Ok(identifiers
        .into_iter()
        .enumerate()
        .filter(|(_, (_, id))| seen.insert(*id))
        .map(|(i, (_, id))| LinkedId { id: id.to_owned(), pointer: format!("{data_pointer}/{i}") })
        .collect())
}

/// A linked id read from a request document, with the pointer of the
/// identifier that named it.
///
/// Before a create or update, the handler looks up each linked resource
/// (§13.2 step 8), and a missing one is reported at this pointer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkedId {
    /// The linked resource's id.
    pub id: String,
    /// The identifier's RFC 6901 pointer in the request body
    /// (`/data/relationships/tags/data/1`).
    pub pointer: String,
}

impl LinkedId {
    /// `404 related_resource_not_found` at this identifier: no resource of
    /// `target_type` has this id.
    pub fn not_found(&self, target_type: &str) -> ErrorObject {
        ErrorObject::new(ErrorCode::RelatedResourceNotFound, format!("`{target_type}` `{}` does not exist", self.id))
            .with_pointer(self.pointer.clone())
    }
}

/// Step 7, first member family: the first attribute name, in byte order,
/// that is not one of `declared` is `400 unknown_attribute` (§8.2).
///
/// `relationships` pairs each relationship's Rust field with its name
/// (`("epic_id", "epic")`). A client still sending the flat shape writes
/// one of those as an attribute, and the detail names the relationship to
/// use instead.
pub fn check_attribute_names(
    attributes: Option<&Map<String, Value>>,
    type_name: &str,
    declared: &[&str],
    relationships: &[(&str, &str)],
) -> Result<(), ErrorObject> {
    let Some(name) = first_unknown(attributes, declared) else { return Ok(()) };
    let mut detail = format!("`{name}` is not an attribute of `{type_name}`");
    if let Some((_, relationship)) = relationships.iter().find(|(field, rel)| *field == name || *rel == name) {
        detail.push_str(&format!("; it is the `{relationship}` relationship"));
    }
    Err(ErrorObject::new(ErrorCode::UnknownAttribute, detail).with_pointer(pointer("/data/attributes", name)))
}

/// Step 7, second member family, for one declared attribute: its value when
/// present.
///
/// The value is checked by deserializing it as `T`, the attribute's Rust
/// type, and one serde rejects is `400 invalid_attribute`, including `null`
/// for a type that is not an `Option`. An absent `required` attribute is
/// `400 missing_attribute`, pointing at `attributes`, or at `data` when the
/// document has none.
pub fn attribute<'a, T: DeserializeOwned>(
    attributes: Option<&'a Map<String, Value>>,
    name: &str,
    required: bool,
) -> Result<Option<&'a Value>, ErrorObject> {
    match attributes.and_then(|a| a.get(name)) {
        Some(value) => match T::deserialize(value) {
            Ok(_) => Ok(Some(value)),
            Err(err) => Err(ErrorObject::new(ErrorCode::InvalidAttribute, format!("`{name}` is invalid: {err}"))
                .with_pointer(pointer("/data/attributes", name))),
        },
        None if required => {
            let at = if attributes.is_some() { "/data/attributes" } else { "/data" };
            Err(ErrorObject::new(ErrorCode::MissingAttribute, format!("the attribute `{name}` is required"))
                .with_pointer(at))
        }
        None => Ok(None),
    }
}

/// Step 7, third member family: the first relationship name, in byte
/// order, that is not one of `declared` is `400 unknown_relationship`
/// (§8.2).
pub fn check_relationship_names(
    relationships: Option<&Map<String, Value>>,
    type_name: &str,
    declared: &[&str],
) -> Result<(), ErrorObject> {
    match first_unknown(relationships, declared) {
        None => Ok(()),
        Some(name) => Err(ErrorObject::new(
            ErrorCode::UnknownRelationship,
            format!("`{name}` is not a relationship of `{type_name}`"),
        )
        .with_pointer(pointer("/data/relationships", name))),
    }
}

/// `400 missing_relationship`: a create document leaves out a to-one that
/// is not an `Option` (§8.2). The pointer names `relationships`, or `data`
/// when the document has none.
pub fn missing_relationship(name: &str, relationships: Option<&Map<String, Value>>) -> ErrorObject {
    let at = if relationships.is_some() { "/data/relationships" } else { "/data" };
    ErrorObject::new(ErrorCode::MissingRelationship, format!("the relationship `{name}` is required")).with_pointer(at)
}

/// Reads a custom op's request body, `{"meta":{"args":{…}}}` (§10.2), and
/// returns `meta.args` for [`check_op_arg_names`] and [`op_arg`].
///
/// An empty body is read as `{"meta":{"args":{}}}`. A missing `meta` is
/// `400 invalid_document` at `""`, and a missing `meta.args` at `/meta`,
/// unless `has_required` is false: an op with no required argument reads a
/// missing member as `{}`. Either member present but not an object is
/// `400 invalid_document` at itself. Other members are ignored.
pub fn op_args(body: &[u8], has_required: bool) -> Result<Map<String, Value>, ErrorObject> {
    if body.is_empty() {
        return Ok(Map::new());
    }
    let mut meta = match parse_object(body)?.remove("meta") {
        Some(Value::Object(meta)) => meta,
        Some(_) => return Err(invalid_document("`meta` must be an object", "/meta")),
        None if has_required => return Err(invalid_document("the document has no `meta` member", "")),
        None => return Ok(Map::new()),
    };
    match meta.remove("args") {
        Some(Value::Object(args)) => Ok(args),
        Some(_) => Err(invalid_document("`meta.args` must be an object", "/meta/args")),
        None if has_required => Err(invalid_document("`meta` has no `args` member", "/meta")),
        None => Ok(Map::new()),
    }
}

/// Step 7 for `meta.args`, unknown names first: the first member, in byte
/// order, that is not one of `declared` is `400 invalid_document` at
/// `/meta/args/{name}`.
pub fn check_op_arg_names(args: &Map<String, Value>, declared: &[&str]) -> Result<(), ErrorObject> {
    match first_unknown(Some(args), declared) {
        None => Ok(()),
        Some(name) => {
            Err(invalid_document(format!("`{name}` is not an argument of this operation"), pointer("/meta/args", name)))
        }
    }
}

/// Step 7 for `meta.args`, one declared argument, read as `T`, the
/// argument's owned Rust type. Call it for each argument in declaration
/// order.
///
/// A value serde rejects is `400 invalid_document` at `/meta/args/{name}`.
/// An absent `required` argument is `400 invalid_document` at `/meta/args`.
/// An absent optional argument reads as `null`, so an `Option` is `None`.
pub fn op_arg<T: DeserializeOwned>(args: &Map<String, Value>, name: &str, required: bool) -> Result<T, ErrorObject> {
    let missing = || invalid_document(format!("the argument `{name}` is required"), "/meta/args");
    match args.get(name) {
        Some(value) => T::deserialize(value)
            .map_err(|err| invalid_document(format!("`{name}` is invalid: {err}"), pointer("/meta/args", name))),
        None if required => Err(missing()),
        None => T::deserialize(&Value::Null).map_err(|_| missing()),
    }
}

/// The first member name, in byte order, outside `declared`. Sorting here
/// keeps the order independent of how `serde_json` orders a map.
fn first_unknown<'a>(members: Option<&'a Map<String, Value>>, declared: &[&str]) -> Option<&'a str> {
    let mut names: Vec<&str> = members?.keys().map(String::as_str).collect();
    names.sort_unstable();
    names.into_iter().find(|name| !declared.contains(name))
}

/// Keeps the first occurrence of each item, in order (§5.4).
pub(crate) fn collapse_duplicates<T: Eq + Hash>(items: Vec<T>) -> Vec<T> {
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
        failure(parse_update(body.as_bytes(), TASK, &LookupKey::from("ship-the-emitter")))
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
    fn an_undecodable_url_id_matches_no_body_id() {
        let undecodable = LookupKey::undecodable("%FF");
        for body_id in ["%FF", "\u{FF}", "\u{FFFD}"] {
            let body = format!(r#"{{"data":{{"type":"tasks","id":"{body_id}"}}}}"#);
            let err = parse_update(body.as_bytes(), TASK, &undecodable).unwrap_err();
            assert_eq!(err.code(), "id_mismatch", "{body_id}");
        }
    }

    #[test]
    fn update_takes_the_url_id_and_does_not_validate_it() {
        let data = parse_update(
            br#"{"data":{"type":"tasks","id":"index"}}"#,
            Endpoint { path: "/api/tasks/index", ..TASK },
            &LookupKey::from("index"),
        )
        .unwrap();
        assert_eq!(data.id.as_deref(), Some("index"));
    }

    #[test]
    fn id_mismatch_document_matches_the_contract_example() {
        let err =
            parse_update(br#"{"data":{"type":"tasks","id":"other"}}"#, TASK, &LookupKey::from("ship-the-emitter"))
                .unwrap_err();
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
        let read = |v: Value| to_many(&v, TAGS, "tags", None);
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
        assert_eq!(
            rel_failure(to_many(&json!({}), "", "tags", None)),
            (400, "invalid_document".to_owned(), String::new())
        );
        assert_eq!(rel_failure(to_many(&json!({"data": {}}), "", "tags", None)).2, "/data");
        assert_eq!(rel_failure(to_one(&json!({"data": [{"type": "epics", "id": "x"}]}), "", "epics", true)).2, "/data");
        assert_eq!(
            rel_failure(to_one(&json!({"data": null}), "", "epics", false)),
            (403, "relationship_required".to_owned(), "/data".to_owned())
        );
        assert_eq!(rel_failure(to_many(&json!({"data": [{"type": "x", "id": "1"}]}), "", "tags", None)).2, "/data/0");
    }

    #[test]
    fn a_relationship_body_is_read_whole_for_its_linkage() {
        let no_source = |body: &str| {
            let err = parse_relationship(body.as_bytes()).unwrap_err();
            (err.status().as_u16(), err.code().to_owned(), err.source().cloned())
        };
        for body in ["", "{", "[]", "null", r#""data""#] {
            assert_eq!(no_source(body), (400, "invalid_document".to_owned(), None), "{body:?}");
        }

        let doc = parse_relationship(br#"{"data":{"type":"epics","id":"markdown-backend"},"meta":{}}"#).unwrap();
        assert_eq!(to_one(&doc, "", "epics", true).unwrap().as_deref(), Some("markdown-backend"));
        let doc = parse_relationship(br#"{"data":[{"type":"tags","id":"a"},{"type":"tags","id":"b"}]}"#).unwrap();
        let pointers: Vec<String> =
            to_many_linked(&doc, "", "tags", None).unwrap().into_iter().map(|l| l.pointer).collect();
        assert_eq!(pointers, ["/data/0", "/data/1"]);

        let doc = parse_relationship(b"{}").unwrap();
        assert_eq!(
            rel_failure(to_many_linked(&doc, "", "tags", Some(1))),
            (400, "invalid_document".to_owned(), String::new())
        );
        let doc = parse_relationship(br#"{"data":[{"type":"tags","id":"a"},{"type":"tags","id":"b"}]}"#).unwrap();
        assert_eq!(
            rel_failure(to_many_linked(&doc, "", "tags", Some(1))),
            (403, "relationship_batch_unsupported".to_owned(), "/data".to_owned())
        );
    }

    #[test]
    fn a_relationship_post_or_delete_takes_one_identifier() {
        // §9.2: after the arity row, before the identifier rows.
        let read = |v: Value| to_many(&v, "", "tags", Some(1));
        let batch = (403, "relationship_batch_unsupported".to_owned(), "/data".to_owned());
        assert_eq!(read(json!({"data": []})).unwrap(), Vec::<String>::new());
        assert_eq!(read(json!({"data": [{"type": "tags", "id": "a"}]})).unwrap(), vec!["a"]);
        assert_eq!(
            rel_failure(read(json!({"data": [{"type": "tags", "id": "a"}, {"type": "tags", "id": "b"}]}))),
            batch
        );
        // Duplicates count as sent.
        assert_eq!(
            rel_failure(read(json!({"data": [{"type": "tags", "id": "a"}, {"type": "tags", "id": "a"}]}))),
            batch
        );
        // The arity row comes first...
        assert_eq!(
            rel_failure(read(json!({"data": {"type": "tags", "id": "a"}}))),
            (400, "invalid_document".to_owned(), "/data".to_owned())
        );
        // ...and the identifier and type rows after.
        assert_eq!(rel_failure(read(json!({"data": [{"id": "a"}, {"type": "tags"}]}))), batch);
        assert_eq!(
            rel_failure(read(json!({"data": [{"type": "epics", "id": "a"}, {"type": "tags", "id": "b"}]}))),
            batch
        );
        assert_eq!(
            rel_failure(read(json!({"data": [{"id": "a"}]}))),
            (400, "invalid_document".to_owned(), "/data/0".to_owned())
        );
    }

    #[test]
    fn duplicates_collapse_to_their_first_occurrence() {
        assert_eq!(collapse_duplicates(vec![3, 1, 3, 2, 1]), vec![3, 1, 2]);
        assert_eq!(collapse_duplicates(Vec::<u8>::new()), Vec::<u8>::new());
    }

    #[test]
    fn to_many_linked_points_at_each_first_occurrence() {
        let linked = to_many_linked(
            &json!({"data": [{"type": "tags", "id": "b"}, {"type": "tags", "id": "a"}, {"type": "tags", "id": "b"}]}),
            TAGS,
            "tags",
            None,
        )
        .unwrap();
        let pairs: Vec<(&str, &str)> = linked.iter().map(|l| (l.id.as_str(), l.pointer.as_str())).collect();
        assert_eq!(pairs, vec![("b", "/data/relationships/tags/data/0"), ("a", "/data/relationships/tags/data/1")]);
        let missing = linked[1].not_found("tags");
        assert_eq!(
            serde_json::to_value(&missing).unwrap(),
            json!({
                "status": "404",
                "code": "related_resource_not_found",
                "title": "Not Found",
                "detail": "`tags` `a` does not exist",
                "source": { "pointer": "/data/relationships/tags/data/1" }
            })
        );
    }

    fn attrs(value: Value) -> Map<String, Value> {
        value.as_object().unwrap().clone()
    }

    #[test]
    fn unknown_attributes_are_reported_in_byte_order() {
        let declared = ["title", "status"];
        let relationships = [("epic_id", "epic"), ("tags", "tags")];
        let check = |v: Value| check_attribute_names(Some(&attrs(v)), "tasks", &declared, &relationships);
        assert!(check(json!({"title": "x", "status": "y"})).is_ok());
        assert!(check_attribute_names(None, "tasks", &declared, &relationships).is_ok());
        assert_eq!(
            rel_failure(check(json!({"zeta": 1, "beta": 2, "title": "x"}))),
            (400, "unknown_attribute".to_owned(), "/data/attributes/beta".to_owned())
        );
        let err = check(json!({"epic_id": "markdown-backend"})).unwrap_err();
        assert_eq!(err.detail(), "`epic_id` is not an attribute of `tasks`; it is the `epic` relationship");
        let err = check(json!({"tags": []})).unwrap_err();
        assert_eq!(err.detail(), "`tags` is not an attribute of `tasks`; it is the `tags` relationship");
        assert_eq!(rel_failure(check(json!({"a/b": 1}))).2, "/data/attributes/a~1b");
    }

    #[test]
    fn a_declared_attribute_is_checked_against_its_type() {
        let a = attrs(json!({"title": "x", "count": "seven", "notes": null, "done": null}));
        assert_eq!(attribute::<String>(Some(&a), "title", true).unwrap(), Some(&json!("x")));
        assert_eq!(attribute::<Option<String>>(Some(&a), "notes", false).unwrap(), Some(&Value::Null));
        assert_eq!(
            rel_failure(attribute::<i32>(Some(&a), "count", false)),
            (400, "invalid_attribute".to_owned(), "/data/attributes/count".to_owned())
        );
        // `null` is a value, and only an `Option` takes it.
        assert_eq!(rel_failure(attribute::<bool>(Some(&a), "done", false)).1, "invalid_attribute");
        assert_eq!(attribute::<String>(Some(&a), "status", false).unwrap(), None);
        assert_eq!(
            rel_failure(attribute::<String>(Some(&a), "status", true)),
            (400, "missing_attribute".to_owned(), "/data/attributes".to_owned())
        );
        assert_eq!(
            rel_failure(attribute::<String>(None, "status", true)),
            (400, "missing_attribute".to_owned(), "/data".to_owned())
        );
    }

    #[test]
    fn unknown_and_missing_relationships() {
        let rels = attrs(json!({"tags": {"data": []}, "owner": {}, "author": {}}));
        assert!(check_relationship_names(Some(&rels), "tasks", &["tags", "owner", "author"]).is_ok());
        assert_eq!(
            rel_failure(check_relationship_names(Some(&rels), "tasks", &["tags"])),
            (400, "unknown_relationship".to_owned(), "/data/relationships/author".to_owned())
        );
        let missing = missing_relationship("workout", Some(&rels));
        assert_eq!((missing.status().as_u16(), missing.code()), (400, "missing_relationship"));
        assert_eq!(missing.source(), Some(&crate::ErrorSource::Pointer("/data/relationships".to_owned())));
        assert_eq!(
            missing_relationship("workout", None).source(),
            Some(&crate::ErrorSource::Pointer("/data".to_owned()))
        );
    }

    #[test]
    fn pointer_tokens_are_escaped() {
        assert_eq!(pointer("/data/attributes", "a/b~c"), "/data/attributes/a~1b~0c");
        assert_eq!(pointer("", "data"), "/data");
    }

    /// (status, code, pointer) of a failure with an optional pointer.
    fn op_failure<T: std::fmt::Debug>(result: Result<T, ErrorObject>) -> (u16, String, Option<String>) {
        let err = result.unwrap_err();
        let pointer = match err.source() {
            None => None,
            Some(crate::ErrorSource::Pointer(p)) => Some(p.clone()),
            Some(other) => panic!("unexpected source {other:?}"),
        };
        (err.status().as_u16(), err.code().to_owned(), pointer)
    }

    fn args_of(body: &str, has_required: bool) -> Result<Map<String, Value>, ErrorObject> {
        op_args(body.as_bytes(), has_required)
    }

    #[test]
    fn op_args_rows() {
        // No body is `{"meta":{"args":{}}}`, required arguments or not.
        assert_eq!(args_of("", true).unwrap(), Map::new());
        assert_eq!(args_of("", false).unwrap(), Map::new());

        let no_source = (400, "invalid_document".to_owned(), None);
        for body in ["{", "[]", "null", r#""meta""#, " "] {
            assert_eq!(op_failure(args_of(body, true)), no_source, "{body}");
            assert_eq!(op_failure(args_of(body, false)), no_source, "{body}");
        }

        // A missing member names the nearest one that exists, unless the op
        // has no required argument.
        assert_eq!(op_failure(args_of("{}", true)), bad(""));
        assert_eq!(op_failure(args_of(r#"{"meta":{}}"#, true)), bad("/meta"));
        assert_eq!(args_of("{}", false).unwrap(), Map::new());
        assert_eq!(args_of(r#"{"meta":{}}"#, false).unwrap(), Map::new());

        // A member that is not an object is itself the error, either way.
        for has_required in [true, false] {
            assert_eq!(op_failure(args_of(r#"{"meta":[]}"#, has_required)), bad("/meta"));
            assert_eq!(op_failure(args_of(r#"{"meta":null}"#, has_required)), bad("/meta"));
            assert_eq!(op_failure(args_of(r#"{"meta":{"args":1}}"#, has_required)), bad("/meta/args"));
            assert_eq!(op_failure(args_of(r#"{"meta":{"args":null}}"#, has_required)), bad("/meta/args"));
        }

        // Other members are ignored.
        let args = args_of(r#"{"data":1,"meta":{"other":2,"args":{"input":{"a":1}}},"links":{}}"#, true).unwrap();
        assert_eq!(Value::Object(args), json!({ "input": { "a": 1 } }));
    }

    #[test]
    fn unknown_op_args_are_reported_in_byte_order_and_escaped() {
        let args = attrs(json!({ "zeta": 1, "b/c": 2, "input": 3, "a~": 4 }));
        assert!(check_op_arg_names(&args, &["zeta", "b/c", "input", "a~"]).is_ok());
        assert_eq!(op_failure(check_op_arg_names(&args, &["input"])), bad("/meta/args/a~0"));
        assert_eq!(op_failure(check_op_arg_names(&args, &["input", "a~"])), bad("/meta/args/b~1c"));
        assert_eq!(op_failure(check_op_arg_names(&args, &["input", "a~", "b/c"])), bad("/meta/args/zeta"));
        assert!(check_op_arg_names(&Map::new(), &[]).is_ok());
    }

    #[test]
    fn a_declared_op_arg_is_read_as_its_type() {
        #[derive(Debug, PartialEq, serde::Deserialize)]
        struct StartInput {
            template_id: String,
        }
        let args = attrs(json!({ "input": { "template_id": "push-day" }, "count": "x", "note": null }));

        let input: StartInput = op_arg(&args, "input", true).unwrap();
        assert_eq!(input, StartInput { template_id: "push-day".to_owned() });

        // A missing required argument points at `args`; a rejected value at
        // its member.
        assert_eq!(op_failure(op_arg::<u32>(&args, "limit", true)), bad("/meta/args"));
        assert_eq!(op_failure(op_arg::<u32>(&args, "count", true)), bad("/meta/args/count"));
        assert_eq!(op_failure(op_arg::<Option<u32>>(&args, "count", false)), bad("/meta/args/count"));

        // Absent or `null` is `None` for an `Option`.
        assert_eq!(op_arg::<Option<String>>(&args, "absent", false).unwrap(), None);
        assert_eq!(op_arg::<Option<String>>(&args, "note", false).unwrap(), None);
        // `null` for a required argument is a value serde rejects.
        assert_eq!(op_failure(op_arg::<String>(&args, "note", true)), bad("/meta/args/note"));
    }
}
