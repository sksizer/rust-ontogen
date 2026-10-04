//! Response documents (§4, §5), and the payloads of event frames (§12).
//!
//! Member order is fixed by field order, never by a map, so a document
//! serializes to the same bytes every time (§4.1). `serde_json::Value` is
//! avoided for that reason: without `preserve_order` it sorts its keys.

use serde::{Serialize, Serializer, ser::SerializeMap};
use serde_json::value::RawValue;

use crate::links::CanonicalQuery;

/// The top-level `jsonapi` member, always `{"version":"1.1"}` (§4.1).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct JsonApiObject;

impl Serialize for JsonApiObject {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(1))?;
        map.serialize_entry("version", "1.1")?;
        map.end()
    }
}

/// Stands in for a member a document never carries, such as `meta` on a
/// plain resource document or `data` on a meta-only one. It has no values,
/// so such a member can only ever be absent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Absent {}

impl Serialize for Absent {
    fn serialize<S: Serializer>(&self, _serializer: S) -> Result<S::Ok, S::Error> {
        match *self {}
    }
}

/// A success document: `jsonapi`, `links`, `meta`, `data`, `included`, in
/// that order (§4.1). Error documents are [`crate::error::ErrorDocument`].
///
/// `D` is the primary data: a [`ResourceObject`], `Option<ResourceObject>`
/// (a to-one that may be `null`), a `Vec` of them, or a [`Linkage`]. `M` is
/// the `meta` member: [`PageMeta`] or [`ResultMeta`].
#[derive(Debug, Clone, Serialize)]
pub struct Document<D, M = Absent> {
    jsonapi: JsonApiObject,
    #[serde(skip_serializing_if = "Option::is_none")]
    links: Option<Links>,
    #[serde(skip_serializing_if = "Option::is_none")]
    meta: Option<M>,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<D>,
    #[serde(skip_serializing_if = "Option::is_none")]
    included: Option<Vec<AnyResource>>,
}

impl<D> Document<D, Absent> {
    /// A document with primary data and its top-level links (§4.1).
    pub fn new(data: D, links: Links) -> Self {
        Document { jsonapi: JsonApiObject, links: Some(links), meta: None, data: Some(data), included: None }
    }

    /// Adds the `meta` member, e.g. a paginated collection's [`PageMeta`].
    pub fn with_meta<M>(self, meta: M) -> Document<D, M> {
        Document {
            jsonapi: self.jsonapi,
            links: self.links,
            meta: Some(meta),
            data: self.data,
            included: self.included,
        }
    }
}

impl<A> Document<ResourceObject<A>, Absent> {
    /// The document of one resource (§8): its top-level `self` is the
    /// resource's own `links.self` followed by `query` (§4.3), as a `GET`
    /// with `include` writes it. A resource with no links, of a type whose
    /// module serves no `get_by_id` (§5.2), has no URL to repeat, so the
    /// document has no top-level links either (§4.1).
    pub fn resource(resource: ResourceObject<A>, query: &CanonicalQuery) -> Self {
        let links = resource.links().map(|links| Links::new(query.href(links.self_link())));
        Document { jsonapi: JsonApiObject, links, meta: None, data: Some(resource), included: None }
    }
}

impl<M> Document<Absent, M> {
    /// A meta-only document, the response of a custom op (§10.1). It has no
    /// primary data and therefore no links.
    pub fn meta_only(meta: M) -> Self {
        Document { jsonapi: JsonApiObject, links: None, meta: Some(meta), data: None, included: None }
    }
}

impl<D, M> Document<D, M> {
    /// Sets `included`. The spec requires the member whenever the request
    /// carried `include`, even when it is empty (§7.5).
    pub fn with_included(mut self, included: Vec<AnyResource>) -> Self {
        self.included = Some(included);
        self
    }

    /// The top-level links.
    pub fn links(&self) -> Option<&Links> {
        self.links.as_ref()
    }

    /// The `meta` member.
    pub fn meta(&self) -> Option<&M> {
        self.meta.as_ref()
    }

    /// The primary data.
    pub fn data(&self) -> Option<&D> {
        self.data.as_ref()
    }

    /// The `included` member.
    pub fn included(&self) -> Option<&[AnyResource]> {
        self.included.as_deref()
    }
}

/// `meta` of a paginated collection: the effective values after clamping,
/// in the order `total`, `limit`, `offset` (§7.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct PageMeta {
    /// The filter-aware count of the whole collection.
    pub total: u64,
    /// The effective page size.
    pub limit: u32,
    /// The effective offset.
    pub offset: u32,
}

/// `meta` of a custom-op response, `{"result": …}` (§10.1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResultMeta<T> {
    /// The op's `Ok` value, serialized as the op's own type.
    pub result: T,
}

/// A links object: `self`, then `related`, then the four pagination links.
///
/// Every links object ontogen emits has `self`. On a paginated document all
/// four pagination links are present, `null` when unavailable (§7.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Links {
    self_link: String,
    related: Option<String>,
    pagination: Option<PaginationLinks>,
}

/// The pagination links of a paginated document (§7.2). Build them with
/// [`crate::links::pagination_links`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaginationLinks {
    /// The first page; always available.
    pub first: String,
    /// The previous page, or `None` (written `null`) on the first page.
    pub prev: Option<String>,
    /// The next page, or `None` (written `null`) on the last page.
    pub next: Option<String>,
    /// The last page; always available.
    pub last: String,
}

impl Links {
    /// A links object with only `self`.
    pub fn new(self_link: impl Into<String>) -> Self {
        Links { self_link: self_link.into(), related: None, pagination: None }
    }

    /// Adds `related`, as on a relationship object (§5.4).
    pub fn with_related(mut self, related: impl Into<String>) -> Self {
        self.related = Some(related.into());
        self
    }

    /// Adds `first`, `prev`, `next` and `last`.
    pub fn with_pagination(mut self, pagination: PaginationLinks) -> Self {
        self.pagination = Some(pagination);
        self
    }

    /// The `self` link.
    pub fn self_link(&self) -> &str {
        &self.self_link
    }

    /// The `related` link.
    pub fn related(&self) -> Option<&str> {
        self.related.as_deref()
    }

    /// The pagination links.
    pub fn pagination(&self) -> Option<&PaginationLinks> {
        self.pagination.as_ref()
    }
}

impl Serialize for Links {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("self", &self.self_link)?;
        if let Some(related) = &self.related {
            map.serialize_entry("related", related)?;
        }
        if let Some(page) = &self.pagination {
            map.serialize_entry("first", &page.first)?;
            map.serialize_entry("prev", &page.prev)?;
            map.serialize_entry("next", &page.next)?;
            map.serialize_entry("last", &page.last)?;
        }
        map.end()
    }
}

/// A resource identifier object, `{"type": …, "id": …}`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
pub struct ResourceIdentifier {
    /// The resource type, the module's `url_plural` (§5.2).
    #[serde(rename = "type")]
    pub type_name: String,
    /// The resource id.
    pub id: String,
}

impl ResourceIdentifier {
    /// An identifier for `id` of type `type_name`.
    pub fn new(type_name: impl Into<String>, id: impl Into<String>) -> Self {
        ResourceIdentifier { type_name: type_name.into(), id: id.into() }
    }
}

/// Resource linkage: the `data` of a relationship object (§5.4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum Linkage {
    /// A to-one relationship; `None` is written `null`.
    ToOne(Option<ResourceIdentifier>),
    /// A to-many relationship, in linkage order.
    ToMany(Vec<ResourceIdentifier>),
}

/// A relationship object: `links`, then `data` (§5.4).
///
/// The constructors require at least one member, because the spec forbids
/// an empty relationship object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Relationship {
    #[serde(skip_serializing_if = "Option::is_none")]
    links: Option<Links>,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Linkage>,
}

impl Relationship {
    /// A relationship with linkage and no links.
    pub fn from_data(data: Linkage) -> Self {
        Relationship { links: None, data: Some(data) }
    }

    /// A relationship with links and no linkage, as a junction-op
    /// relationship is written (§9.1).
    pub fn from_links(links: Links) -> Self {
        Relationship { links: Some(links), data: None }
    }

    /// A relationship with both members.
    pub fn new(links: Links, data: Linkage) -> Self {
        Relationship { links: Some(links), data: Some(data) }
    }

    /// The links.
    pub fn links(&self) -> Option<&Links> {
        self.links.as_ref()
    }

    /// The linkage.
    pub fn data(&self) -> Option<&Linkage> {
        self.data.as_ref()
    }
}

/// A resource object: `type`, `id`, `attributes`, `relationships`, `links`
/// (§5.2).
///
/// `A` is the attributes object. Generated code passes a struct whose fields
/// are the entity's attributes in declaration order, so the member order
/// comes from the struct.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResourceObject<A> {
    #[serde(rename = "type")]
    type_name: String,
    id: String,
    attributes: A,
    #[serde(skip_serializing_if = "Relationships::is_empty")]
    relationships: Relationships,
    #[serde(skip_serializing_if = "Option::is_none")]
    links: Option<Links>,
}

/// A resource object whose attributes are already serialized, so resources
/// of different types fit in one `included` array.
pub type AnyResource = ResourceObject<Box<RawValue>>;

impl<A> ResourceObject<A> {
    /// A resource with no relationships. `self_link` is its `links.self`,
    /// which equals the `Location` header on create (§5.2).
    pub fn new(
        type_name: impl Into<String>,
        id: impl Into<String>,
        attributes: A,
        self_link: impl Into<String>,
    ) -> Self {
        let mut resource = Self::without_links(type_name, id, attributes);
        resource.links = Some(Links::new(self_link));
        resource
    }

    /// A resource with no relationships and no `links` member: one of a
    /// type whose module serves no `get_by_id`, since a server must serve
    /// every link it emits (§5.2).
    pub fn without_links(type_name: impl Into<String>, id: impl Into<String>, attributes: A) -> Self {
        ResourceObject {
            type_name: type_name.into(),
            id: id.into(),
            attributes,
            relationships: Relationships::default(),
            links: None,
        }
    }

    /// Appends a relationship. Relationships serialize in the order they are
    /// added, which generated code makes the declaration order. The member
    /// is omitted while there are none (§5.2).
    pub fn with_relationship(mut self, name: impl Into<String>, relationship: Relationship) -> Self {
        self.relationships.0.push((name.into(), relationship));
        self
    }

    /// The resource type.
    pub fn type_name(&self) -> &str {
        &self.type_name
    }

    /// The resource id.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The attributes.
    pub fn attributes(&self) -> &A {
        &self.attributes
    }

    /// The relationships, in the order they serialize.
    pub fn relationships(&self) -> impl Iterator<Item = (&str, &Relationship)> {
        self.relationships.0.iter().map(|(name, rel)| (name.as_str(), rel))
    }

    /// The resource's links, `None` when it has none.
    pub fn links(&self) -> Option<&Links> {
        self.links.as_ref()
    }

    /// The identifier of this resource.
    pub fn identifier(&self) -> ResourceIdentifier {
        ResourceIdentifier::new(self.type_name.clone(), self.id.clone())
    }
}

impl<A: Serialize> ResourceObject<A> {
    /// Serializes the attributes now, so the resource can join an
    /// `included` array beside resources of other types.
    pub fn erase(self) -> Result<AnyResource, serde_json::Error> {
        let attributes = serde_json::value::to_raw_value(&self.attributes)?;
        Ok(ResourceObject {
            type_name: self.type_name,
            id: self.id,
            attributes,
            relationships: self.relationships,
            links: self.links,
        })
    }
}

impl<A> ResourceObject<A> {
    /// This resource as an event frame carries it (§12): no `links`, its
    /// relationships with `data` only, and a relationship with no `data` (a
    /// junction op's) left out. A frame is not tied to a request URL.
    pub fn into_unlinked(self) -> UnlinkedResource<A> {
        let relationships = self
            .relationships
            .0
            .into_iter()
            .filter_map(|(name, rel)| rel.data.map(|data| (name, Relationship::from_data(data))))
            .collect();
        UnlinkedResource {
            type_name: self.type_name,
            id: self.id,
            attributes: self.attributes,
            relationships: Relationships(relationships),
        }
    }
}

/// A resource object with no links: `type`, `id`, `attributes`,
/// `relationships`, in that order, `relationships` omitted when there are
/// none (§12). Build it with [`ResourceObject::into_unlinked`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UnlinkedResource<A> {
    #[serde(rename = "type")]
    type_name: String,
    id: String,
    attributes: A,
    #[serde(skip_serializing_if = "Relationships::is_empty")]
    relationships: Relationships,
}

impl<A> UnlinkedResource<A> {
    /// The resource type.
    pub fn type_name(&self) -> &str {
        &self.type_name
    }

    /// The resource id.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The attributes.
    pub fn attributes(&self) -> &A {
        &self.attributes
    }

    /// The relationships, in the order they serialize, each with `data`
    /// only.
    pub fn relationships(&self) -> impl Iterator<Item = (&str, &Relationship)> {
        self.relationships.0.iter().map(|(name, rel)| (name.as_str(), rel))
    }
}

/// The `data:` of an event frame whose item is not an entity:
/// `{"meta":{"result":…}}`, the `meta` a custom op responds with (§10.1),
/// without the document around it (§12).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResultFrame<T> {
    meta: ResultMeta<T>,
}

impl<T> ResultFrame<T> {
    /// The frame of `result`.
    pub fn new(result: T) -> Self {
        ResultFrame { meta: ResultMeta { result } }
    }

    /// The item.
    pub fn result(&self) -> &T {
        &self.meta.result
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Relationships(Vec<(String, Relationship)>);

impl Relationships {
    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl Serialize for Relationships {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (name, rel) in &self.0 {
            map.serialize_entry(name, rel)?;
        }
        map.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Serialize)]
    struct TaskAttributes {
        title: &'static str,
        status: &'static str,
        created: &'static str,
        body: &'static str,
    }

    #[derive(Serialize)]
    struct EpicAttributes {
        title: &'static str,
        status: &'static str,
        body: &'static str,
    }

    const TASK_BODY: &str = "## Goal\n\nEmit markdown CRUD matching the golden spec. ^summary\n\n## Outcome\n\nMatched on the first conformance run.\n";

    fn task() -> ResourceObject<TaskAttributes> {
        ResourceObject::new(
            "tasks",
            "ship-the-emitter",
            TaskAttributes { title: "Ship the emitter", status: "closed/done", created: "2026-06-06", body: TASK_BODY },
            "/api/tasks/ship-the-emitter",
        )
        .with_relationship(
            "epic",
            Relationship::new(
                Links::new("/api/tasks/ship-the-emitter/relationships/epic")
                    .with_related("/api/tasks/ship-the-emitter/epic"),
                Linkage::ToOne(Some(ResourceIdentifier::new("epics", "markdown-backend"))),
            ),
        )
        .with_relationship(
            "tags",
            Relationship::new(
                Links::new("/api/tasks/ship-the-emitter/relationships/tags")
                    .with_related("/api/tasks/ship-the-emitter/tags"),
                Linkage::ToMany(vec![ResourceIdentifier::new("tags", "codegen")]),
            ),
        )
    }

    fn epic() -> ResourceObject<EpicAttributes> {
        ResourceObject::new(
            "epics",
            "markdown-backend",
            EpicAttributes {
                title: "Markdown backend",
                status: "in-progress",
                body: "Implement ADR 0001 as a stacked-PR campaign.\n",
            },
            "/api/epics/markdown-backend",
        )
    }

    // §5.5, compacted.
    const TASK_JSON: &str = concat!(
        r#"{"type":"tasks","id":"ship-the-emitter","#,
        r#""attributes":{"title":"Ship the emitter","status":"closed/done","created":"2026-06-06","#,
        r###""body":"## Goal\n\nEmit markdown CRUD matching the golden spec. ^summary\n\n## Outcome\n\nMatched on the first conformance run.\n"},"###,
        r#""relationships":{"#,
        r#""epic":{"links":{"self":"/api/tasks/ship-the-emitter/relationships/epic","related":"/api/tasks/ship-the-emitter/epic"},"data":{"type":"epics","id":"markdown-backend"}},"#,
        r#""tags":{"links":{"self":"/api/tasks/ship-the-emitter/relationships/tags","related":"/api/tasks/ship-the-emitter/tags"},"data":[{"type":"tags","id":"codegen"}]}"#,
        r#"},"links":{"self":"/api/tasks/ship-the-emitter"}}"#,
    );

    const EPIC_JSON: &str = concat!(
        r#"{"type":"epics","id":"markdown-backend","#,
        r#""attributes":{"title":"Markdown backend","status":"in-progress","body":"Implement ADR 0001 as a stacked-PR campaign.\n"},"#,
        r#""links":{"self":"/api/epics/markdown-backend"}}"#,
    );

    #[test]
    fn resource_object_matches_the_contract_example_byte_for_byte() {
        assert_eq!(serde_json::to_string(&task()).unwrap(), TASK_JSON);
    }

    #[test]
    fn resource_without_relationships_omits_the_member() {
        assert_eq!(serde_json::to_string(&epic()).unwrap(), EPIC_JSON);
    }

    #[test]
    fn single_resource_document_orders_jsonapi_links_data() {
        let doc = Document::new(task(), Links::new("/api/tasks/ship-the-emitter"));
        let expected = format!(
            r#"{{"jsonapi":{{"version":"1.1"}},"links":{{"self":"/api/tasks/ship-the-emitter"}},"data":{TASK_JSON}}}"#
        );
        assert_eq!(serde_json::to_string(&doc).unwrap(), expected);
    }

    #[test]
    fn a_resource_without_links_omits_the_member() {
        let resource = ResourceObject::without_links(
            "epics",
            "markdown-backend",
            EpicAttributes { title: "Markdown backend", status: "in-progress", body: "" },
        )
        .with_relationship("owner", Relationship::from_data(Linkage::ToOne(None)));
        assert_eq!(resource.links(), None);
        assert_eq!(
            serde_json::to_string(&resource).unwrap(),
            concat!(
                r#"{"type":"epics","id":"markdown-backend","#,
                r#""attributes":{"title":"Markdown backend","status":"in-progress","body":""},"#,
                r#""relationships":{"owner":{"data":null}}}"#,
            )
        );
    }

    #[test]
    fn a_resource_document_repeats_the_resource_link_with_the_query() {
        let doc = Document::resource(task(), &CanonicalQuery::new());
        let expected = format!(
            r#"{{"jsonapi":{{"version":"1.1"}},"links":{{"self":"/api/tasks/ship-the-emitter"}},"data":{TASK_JSON}}}"#
        );
        assert_eq!(serde_json::to_string(&doc).unwrap(), expected);
        let mut query = CanonicalQuery::new();
        query.set_include(["epic"]);
        let doc = Document::resource(task(), &query);
        assert_eq!(doc.links().map(Links::self_link), Some("/api/tasks/ship-the-emitter?include=epic"));
        assert_eq!(doc.data().and_then(|r| r.links()).map(Links::self_link), Some("/api/tasks/ship-the-emitter"));
    }

    #[test]
    fn a_resource_without_links_has_a_document_without_links() {
        let resource =
            ResourceObject::without_links("tags", "codegen", EpicAttributes { title: "", status: "", body: "" });
        let doc = Document::resource(resource, &CanonicalQuery::new());
        assert_eq!(
            serde_json::to_string(&doc).unwrap(),
            r#"{"jsonapi":{"version":"1.1"},"data":{"type":"tags","id":"codegen","attributes":{"title":"","status":"","body":""}}}"#
        );
    }

    #[test]
    fn included_follows_data_and_keeps_attribute_order() {
        let doc = Document::new(task(), Links::new("/api/tasks/ship-the-emitter?include=epic"))
            .with_included(vec![epic().erase().unwrap()]);
        let expected = format!(
            r#"{{"jsonapi":{{"version":"1.1"}},"links":{{"self":"/api/tasks/ship-the-emitter?include=epic"}},"data":{TASK_JSON},"included":[{EPIC_JSON}]}}"#
        );
        assert_eq!(serde_json::to_string(&doc).unwrap(), expected);
    }

    #[test]
    fn empty_included_is_written() {
        let doc = Document::new(Vec::<AnyResource>::new(), Links::new("/api/tasks?include=")).with_included(vec![]);
        assert_eq!(
            serde_json::to_string(&doc).unwrap(),
            r#"{"jsonapi":{"version":"1.1"},"links":{"self":"/api/tasks?include="},"data":[],"included":[]}"#
        );
    }

    #[test]
    fn empty_to_one_primary_data_is_null() {
        let doc = Document::new(None::<ResourceObject<EpicAttributes>>, Links::new("/api/tasks/x/epic"));
        assert_eq!(
            serde_json::to_string(&doc).unwrap(),
            r#"{"jsonapi":{"version":"1.1"},"links":{"self":"/api/tasks/x/epic"},"data":null}"#
        );
    }

    #[test]
    fn paginated_document_orders_links_meta_data_and_writes_null_links() {
        let links = Links::new("/api/tasks?page%5Boffset%5D=0&page%5Blimit%5D=20").with_pagination(PaginationLinks {
            first: "/api/tasks?page%5Boffset%5D=0&page%5Blimit%5D=20".into(),
            prev: None,
            next: None,
            last: "/api/tasks?page%5Boffset%5D=0&page%5Blimit%5D=20".into(),
        });
        let doc =
            Document::new(Vec::<AnyResource>::new(), links).with_meta(PageMeta { total: 0, limit: 20, offset: 0 });
        assert_eq!(
            serde_json::to_string(&doc).unwrap(),
            concat!(
                r#"{"jsonapi":{"version":"1.1"},"links":{"#,
                r#""self":"/api/tasks?page%5Boffset%5D=0&page%5Blimit%5D=20","#,
                r#""first":"/api/tasks?page%5Boffset%5D=0&page%5Blimit%5D=20","prev":null,"next":null,"#,
                r#""last":"/api/tasks?page%5Boffset%5D=0&page%5Blimit%5D=20"},"#,
                r#""meta":{"total":0,"limit":20,"offset":0},"data":[]}"#
            )
        );
    }

    #[test]
    fn relationship_document_carries_self_and_related() {
        let doc = Document::new(
            Linkage::ToOne(Some(ResourceIdentifier::new("epics", "markdown-backend"))),
            Links::new("/api/tasks/ship-the-emitter/relationships/epic")
                .with_related("/api/tasks/ship-the-emitter/epic"),
        );
        assert_eq!(
            serde_json::to_string(&doc).unwrap(),
            concat!(
                r#"{"jsonapi":{"version":"1.1"},"links":{"self":"/api/tasks/ship-the-emitter/relationships/epic","#,
                r#""related":"/api/tasks/ship-the-emitter/epic"},"data":{"type":"epics","id":"markdown-backend"}}"#
            )
        );
    }

    #[test]
    fn meta_only_document_has_no_links_or_data() {
        #[derive(Serialize)]
        struct Stats {
            total_count: u32,
            total_duration_minutes: u32,
        }
        let doc = Document::meta_only(ResultMeta { result: Stats { total_count: 12, total_duration_minutes: 540 } });
        assert_eq!(
            serde_json::to_string(&doc).unwrap(),
            r#"{"jsonapi":{"version":"1.1"},"meta":{"result":{"total_count":12,"total_duration_minutes":540}}}"#
        );
    }

    #[test]
    fn relationship_with_links_only_or_data_only() {
        let links_only = Relationship::from_links(Links::new("/a/relationships/r").with_related("/a/r"));
        assert_eq!(
            serde_json::to_string(&links_only).unwrap(),
            r#"{"links":{"self":"/a/relationships/r","related":"/a/r"}}"#
        );
        let data_only = Relationship::from_data(Linkage::ToOne(None));
        assert_eq!(serde_json::to_string(&data_only).unwrap(), r#"{"data":null}"#);
        let empty_many = Relationship::from_data(Linkage::ToMany(vec![]));
        assert_eq!(serde_json::to_string(&empty_many).unwrap(), r#"{"data":[]}"#);
    }

    #[test]
    fn an_unlinked_resource_leaves_out_every_link_and_data_less_relationships() {
        let frame = task()
            .with_relationship(
                "subtasks",
                Relationship::from_links(
                    Links::new("/api/tasks/ship-the-emitter/relationships/subtasks")
                        .with_related("/api/tasks/ship-the-emitter/subtasks"),
                ),
            )
            .into_unlinked();
        assert_eq!(
            serde_json::to_string(&frame).unwrap(),
            concat!(
                r#"{"type":"tasks","id":"ship-the-emitter","#,
                r#""attributes":{"title":"Ship the emitter","status":"closed/done","created":"2026-06-06","#,
                r###""body":"## Goal\n\nEmit markdown CRUD matching the golden spec. ^summary\n\n## Outcome\n\nMatched on the first conformance run.\n"},"###,
                r#""relationships":{"epic":{"data":{"type":"epics","id":"markdown-backend"}},"#,
                r#""tags":{"data":[{"type":"tags","id":"codegen"}]}}}"#,
            )
        );
        assert_eq!(frame.relationships().map(|(name, _)| name).collect::<Vec<_>>(), ["epic", "tags"]);
    }

    #[test]
    fn an_unlinked_resource_without_relationships_omits_the_member() {
        assert_eq!(
            serde_json::to_string(&epic().into_unlinked()).unwrap(),
            concat!(
                r#"{"type":"epics","id":"markdown-backend","#,
                r#""attributes":{"title":"Markdown backend","status":"in-progress","body":"Implement ADR 0001 as a stacked-PR campaign.\n"}}"#,
            )
        );
    }

    #[test]
    fn a_result_frame_is_meta_only_without_a_document() {
        #[derive(Serialize)]
        struct Activity {
            seq: u64,
            kind: &'static str,
            id: &'static str,
        }
        let frame = ResultFrame::new(Activity { seq: 4, kind: "workout", id: "w1" });
        assert_eq!(
            serde_json::to_string(&frame).unwrap(),
            r#"{"meta":{"result":{"seq":4,"kind":"workout","id":"w1"}}}"#
        );
    }
}
