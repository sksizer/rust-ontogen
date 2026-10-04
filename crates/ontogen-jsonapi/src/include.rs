//! Compound documents: reading `include` and gathering `included` (§7.5).
//!
//! Which relationships a route can include depends on the schema and the
//! route's scope, so generated code names them; this module holds the rules
//! that do not: how the value is split and checked, and how `included` is
//! ordered and kept free of duplicates.

use std::collections::HashSet;

use serde::Serialize;

use crate::{
    document::{AnyResource, ResourceObject},
    error::{ErrorCode, ErrorObject},
};

/// The paths of an `include` value, each one of `includable`, duplicates
/// collapsed to their first occurrence (§4.3 rule 4). An empty value is no
/// path.
///
/// Fails with `400 invalid_include_path` on the first item, in request
/// order, that is empty, dotted (nested inclusion is not supported), a
/// relationship in `excluded`, or no relationship of `type_name` at all.
pub(crate) fn parse_paths<'s>(
    value: &str,
    type_name: &str,
    includable: &[&'s str],
    excluded: &[&str],
) -> Result<Vec<&'s str>, ErrorObject> {
    if value.is_empty() {
        return Ok(Vec::new());
    }
    let mut paths: Vec<&'s str> = Vec::new();
    for item in value.split(',') {
        let refused = if item.is_empty() {
            "`include` has an empty item".to_owned()
        } else if item.contains('.') {
            format!("`{item}` is a nested path, which cannot be included")
        } else if let Some(&path) = includable.iter().find(|&&path| path == item) {
            if !paths.contains(&path) {
                paths.push(path);
            }
            continue;
        } else if excluded.contains(&item) {
            format!("`{item}` is a relationship of `{type_name}` that cannot be included")
        } else {
            format!("`{item}` is not a relationship of `{type_name}`")
        };
        let can = if includable.is_empty() {
            format!("`{type_name}` has no relationship that can be included")
        } else {
            format!("it can include: {}", includable.join(", "))
        };
        return Err(
            ErrorObject::new(ErrorCode::InvalidIncludePath, format!("{refused}; {can}")).with_parameter("include")
        );
    }
    Ok(paths)
}

/// The `included` member of a compound document, gathered one included
/// path at a time (§7.5).
///
/// It holds each `(type, id)` once, in the order resources are pushed, and
/// never one of the primary data's own resources. Generated code asks it
/// which linked ids are new before fetching them, so a resource reached
/// through two paths, or linked twice, is fetched once.
#[derive(Debug, Default)]
pub struct Included {
    seen: HashSet<(String, String)>,
    resources: Vec<AnyResource>,
}

impl Included {
    /// An empty `included` for primary data of type `type_name` with the
    /// ids `primary`, which are never added.
    pub fn new<'a>(type_name: &str, primary: impl IntoIterator<Item = &'a str>) -> Self {
        let mut included = Included::default();
        included.new_ids(type_name, primary);
        included
    }

    /// The ids of `ids` not seen yet as resources of type `type_name`, in
    /// order and each once. They count as seen from now on, whether or not
    /// a resource is then found for them: a dangling id is fetched once.
    pub fn new_ids<'a>(&mut self, type_name: &str, ids: impl IntoIterator<Item = &'a str>) -> Vec<String> {
        ids.into_iter()
            .filter(|id| self.seen.insert((type_name.to_owned(), (*id).to_owned())))
            .map(str::to_owned)
            .collect()
    }

    /// Appends `resource`. Its attributes are serialized now, which fails
    /// only for a consumer type's own `Serialize`: `500 internal_error`.
    pub fn push<A: Serialize>(&mut self, resource: ResourceObject<A>) -> Result<(), ErrorObject> {
        let resource = resource
            .erase()
            .map_err(|err| ErrorObject::internal(format!("failed to serialize an included resource: {err}")))?;
        self.resources.push(resource);
        Ok(())
    }

    /// The resources, in the order they were pushed.
    pub fn finish(self) -> Vec<AnyResource> {
        self.resources
    }
}

#[cfg(test)]
mod tests {
    use http::StatusCode;

    use super::*;
    use crate::ErrorSource;

    const TASK: &[&str] = &["epic", "tags", "parent", "subtasks"];

    fn refused(value: &str, includable: &[&str], excluded: &[&str]) -> String {
        let err = parse_paths(value, "tasks", includable, excluded).unwrap_err();
        assert_eq!((err.status(), err.code()), (StatusCode::BAD_REQUEST, "invalid_include_path"));
        assert_eq!(err.source(), Some(&ErrorSource::Parameter("include".to_owned())));
        err.detail().to_owned()
    }

    #[test]
    fn paths_are_kept_in_request_order_without_duplicates() {
        assert_eq!(parse_paths("tags,epic", "tasks", TASK, &[]).unwrap(), ["tags", "epic"]);
        assert_eq!(parse_paths("epic,epic", "tasks", TASK, &[]).unwrap(), ["epic"]);
        assert_eq!(parse_paths("subtasks,parent,subtasks", "tasks", TASK, &[]).unwrap(), ["subtasks", "parent"]);
    }

    #[test]
    fn an_empty_value_includes_nothing() {
        assert!(parse_paths("", "tasks", TASK, &[]).unwrap().is_empty());
        assert!(parse_paths("", "tags", &[], &[]).unwrap().is_empty());
    }

    #[test]
    fn each_refusal_says_why_and_what_can_be_included() {
        let can = "it can include: epic, tags, parent, subtasks";
        assert_eq!(refused("owner", TASK, &["labels"]), format!("`owner` is not a relationship of `tasks`; {can}"));
        assert_eq!(
            refused("labels", TASK, &["labels"]),
            format!("`labels` is a relationship of `tasks` that cannot be included; {can}")
        );
        assert_eq!(
            refused("epic.tasks", TASK, &[]),
            format!("`epic.tasks` is a nested path, which cannot be included; {can}")
        );
        assert_eq!(refused("epic,", TASK, &[]), format!("`include` has an empty item; {can}"));
        assert_eq!(refused(",", TASK, &[]), format!("`include` has an empty item; {can}"));
        assert_eq!(
            refused("epic", &[], &["epic"]),
            "`epic` is a relationship of `tasks` that cannot be included; `tasks` has no relationship that can be \
             included"
        );
    }

    #[test]
    fn the_first_bad_item_in_request_order_is_reported() {
        assert!(refused("epic,owner,epic.tasks", TASK, &[]).starts_with("`owner`"));
        assert!(refused("epic.tasks,owner", TASK, &[]).starts_with("`epic.tasks`"));
        assert!(refused("epic,,owner", TASK, &[]).starts_with("`include` has an empty item"));
        // Names match exactly: no trimming, no case folding.
        assert!(refused("Epic", TASK, &[]).starts_with("`Epic`"));
        assert!(refused("epic, tags", TASK, &[]).starts_with("` tags`"));
    }

    #[derive(Serialize)]
    struct Title {
        title: &'static str,
    }

    fn resource(type_name: &str, id: &str) -> ResourceObject<Title> {
        ResourceObject::new(type_name, id, Title { title: "t" }, format!("/api/{type_name}/{id}"))
    }

    fn identifiers(included: Included) -> Vec<(String, String)> {
        included.finish().iter().map(|r| (r.type_name().to_owned(), r.id().to_owned())).collect()
    }

    #[test]
    fn primary_resources_are_never_new() {
        let mut included = Included::new("tasks", ["a", "b"]);
        assert_eq!(included.new_ids("tasks", ["b", "c", "a", "d"]), ["c", "d"]);
        // The same id under another type is another resource.
        assert_eq!(included.new_ids("epics", ["a"]), ["a"]);
    }

    #[test]
    fn an_id_is_new_once_across_paths_and_within_one() {
        let mut included = Included::new("tasks", ["a"]);
        assert_eq!(included.new_ids("tasks", ["p", "p", "q"]), ["p", "q"]);
        assert_eq!(included.new_ids("tasks", ["q", "r", "p"]), ["r"]);
        // A dangling id is never pushed, yet stays seen.
        assert!(included.new_ids("tasks", ["r"]).is_empty());
    }

    #[test]
    fn resources_keep_their_push_order() {
        let mut included = Included::new("tasks", ["a"]);
        for id in included.new_ids("epics", ["e2", "e1"]) {
            included.push(resource("epics", &id)).unwrap();
        }
        for id in included.new_ids("tags", ["t1"]) {
            included.push(resource("tags", &id)).unwrap();
        }
        assert_eq!(
            identifiers(included),
            [("epics", "e2"), ("epics", "e1"), ("tags", "t1")].map(|(t, i)| (t.to_owned(), i.to_owned()))
        );
    }

    #[test]
    fn a_resource_that_fails_to_serialize_is_a_500() {
        struct Broken;
        impl Serialize for Broken {
            fn serialize<S: serde::Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
                Err(serde::ser::Error::custom("broken"))
            }
        }
        let mut included = Included::default();
        let err = included.push(ResourceObject::new("tasks", "a", Broken, "/api/tasks/a")).unwrap_err();
        assert_eq!((err.status(), err.code()), (StatusCode::INTERNAL_SERVER_ERROR, "internal_error"));
        assert!(identifiers(included).is_empty());
    }
}
