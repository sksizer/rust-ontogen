//! The relationship and related routes of a resource module (wire contract
//! §9): `{collection}/{id}/relationships/{rel}` (`GET`, `PATCH`, `POST`,
//! `DELETE`) and `{collection}/{id}/{rel}` (`GET`), one handler per method,
//! each matching the captured `{rel}` against the type's relationships.
//!
//! Field relationships are read from the parent entity and written through
//! its module's `update`; junction relationships call their `list_X`,
//! `add_Y` and `remove_Y`. Every handler runs the §13.2 steps in order:
//! the path, `{rel}` (step 4), the query (5), the route-level refusals (6),
//! the body (7), the parent and each linked resource (8), then the write (9).

use ontogen_core::ir::OpKind;
use ontogen_jsonapi::links::encode_path_segment;

use super::{
    Routes, SCOPE, app_error_path, await_str, axum_path, collection_expr, entity_type, err_map, is_scoped,
    linkage_expr, linked_lookup, relationship_routes, resource_names, returns_app_error, served_resource,
};
use crate::resource::{Arity, JunctionRelationship, Relationship, Resource};
use crate::servers::classify::classify_op;
use crate::servers::config::{Config, RoutePrefix};
use crate::servers::parse::{ApiFn, ApiModule};
use crate::servers::types::extract_input_type;

/// A resource module serving relationship routes, under one scope.
struct Served<'a> {
    m: &'a ApiModule,
    modules: &'a [ApiModule],
    config: &'a Config,
    resource: &'a Resource,
    /// The `get_by_id` every route reads the parent with.
    get: &'a ApiFn,
    /// The `update` field relationships are written through; without it they
    /// are read-only.
    update: Option<&'a ApiFn>,
    junctions: Vec<JunctionRelationship<'a>>,
    /// The route prefix, when the routes are the scoped ones.
    scope: Option<&'a RoutePrefix>,
}

impl<'a> Served<'a> {
    fn new(m: &'a ApiModule, modules: &'a [ApiModule], config: &'a Config) -> Option<Self> {
        let get = relationship_routes(m, config)?;
        let resource = config.resources.by_module(&m.name)?;
        let update =
            m.functions.iter().find(|f| classify_op(m, f) == OpKind::Update && served_resource(m, f, config).is_some());
        Some(Served {
            m,
            modules,
            config,
            resource,
            get,
            update,
            junctions: config
                .resources
                .junctions(m)
                .expect("`check_http_ops` refuses a module whose junction relationships do not build"),
            scope: config.route_prefix.as_ref().filter(|_| is_scoped(get, config)),
        })
    }

    /// The module and `get_by_id` of a relationship's target, which
    /// `check_http_ops` has checked exist.
    fn target(&self, target_module: &str) -> (&'a ApiModule, &'a ApiFn) {
        linked_lookup(self.modules, target_module).expect("a relationship's target module serves get_by_id")
    }

    /// The helper of `kind` for `f` of module `module`, as a call's callee
    /// and its leading arguments.
    fn call(&self, kind: Helper, module: &str, f: &ApiFn) -> String {
        let scoped = self.scope.is_some() && f.first_param_is_store;
        let scope = if scoped { format!("&{SCOPE}, ") } else { String::new() };
        format!("{}(&ontogen_state, {scope}", kind.name(module, scoped))
    }

    /// The collection a target's resources are linked under: under the
    /// prefix when this is the scoped handler and the target's `get_by_id`
    /// is scoped too.
    fn target_collection(&self, target_type: &str, target_get: &ApiFn) -> String {
        collection_expr(target_type, self.scope.filter(|_| target_get.first_param_is_store))
    }
}

/// The helpers the relationship handlers share, one per module, kind and
/// scope.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Helper {
    /// The parent resource, read as its `GET` reads it.
    Read,
    /// One relation field written through the module's `update`.
    WriteField,
    /// That each linked id names a resource.
    CheckIds,
    /// The resources ids name, the dangling ones skipped.
    Fetch,
}

impl Helper {
    fn name(self, module: &str, scoped: bool) -> String {
        let kind = match self {
            Helper::Read => "read",
            Helper::WriteField => "write_field",
            Helper::CheckIds => "check_ids",
            Helper::Fetch => "fetch",
        };
        let suffix = if scoped { "_scoped" } else { "" };
        format!("ontogen_{module}_{kind}{suffix}")
    }
}

/// Emit every helper the relationship handlers of `modules` call, once each.
pub(super) fn emit_helpers(out: &mut String, modules: &[ApiModule], config: &Config) {
    let mut wanted: Vec<(Helper, &ApiModule, &ApiFn, bool)> = Vec::new();
    for m in modules {
        let Some(served) = Served::new(m, modules, config) else { continue };
        let mut uses = vec![(Helper::Read, m, served.get)];
        if let Some(update) = served.update.filter(|_| !served.resource.relationships.is_empty()) {
            uses.push((Helper::WriteField, m, update));
        }
        for rel in &served.resource.relationships {
            let (tm, tf) = served.target(&rel.target_module);
            uses.push((Helper::Fetch, tm, tf));
            if served.update.is_some() {
                uses.push((Helper::CheckIds, tm, tf));
            }
        }
        for j in &served.junctions {
            let (tm, tf) = served.target(&j.target_module);
            if j.add.is_some() {
                uses.push((Helper::CheckIds, tm, tf));
            }
            if !j.lists_entities {
                uses.push((Helper::Fetch, tm, tf));
            }
        }
        for (kind, module, f) in uses {
            let scoped = served.scope.is_some() && f.first_param_is_store;
            if !wanted.iter().any(|(k, wm, _, s)| *k == kind && wm.name == module.name && *s == scoped) {
                wanted.push((kind, module, f, scoped));
            }
        }
    }
    for (kind, m, f, scoped) in wanted {
        emit_helper(out, kind, m, f, config, scoped);
    }
}

/// Emit the helper of `kind` calling `f` of module `m`.
fn emit_helper(out: &mut String, kind: Helper, m: &ApiModule, f: &ApiFn, config: &Config, scoped: bool) {
    let resource = config.resources.by_module(&m.name).expect("a relationship helper serves a resource module");
    let type_name = &resource.resource_type;
    let state_type = &config.state_type;
    let svc = m.service_ident(f.surface);
    let aw = await_str(f.is_async);
    let map_err = err_map(f, config);
    let prefix = config.route_prefix.as_ref().filter(|_| scoped);
    // A `String` prefix is borrowed as `&str`, which clippy's `ptr_arg` asks
    // of a consumer's code.
    let scope_param = prefix
        .map(|p| {
            let ty = &p.params[0].rust_type;
            format!("{SCOPE}: &{}, ", if ty == "String" { "str" } else { ty.as_str() })
        })
        .unwrap_or_default();
    let (open, arg) = match (f.first_param_is_store, prefix) {
        (false, _) => (String::new(), "state"),
        (true, Some(prefix)) => (
            format!("    let store = state.{}({SCOPE}).map_err(ontogen_internal_error)?;\n", prefix.state_accessor),
            "&store",
        ),
        (true, None) => (
            format!("    let store = state.{}().await.map_err(ontogen_internal_error)?;\n", f.store_accessor),
            "&store",
        ),
    };
    let name = kind.name(&m.name, scoped);
    let entity_ty = entity_type(m).expect("a resource module names its entity");
    // The target's `{Entity}NotFound`, when its `get_by_id` fails with it:
    // the id names nothing.
    let not_found = config
        .error_map
        .as_ref()
        .and_then(|map| map.variants.iter().find(|v| v.name == format!("{}NotFound", resource.entity.name)))
        .filter(|_| returns_app_error(f, config))
        .map(|v| v.pattern(&app_error_path(config)));
    let fallback = if returns_app_error(f, config) { "ontogen_app_error" } else { "ontogen_internal_error" };
    match kind {
        Helper::Read => out.push_str(&format!(
            "/// The `{type_name}` resource `id` names, read as its `GET` reads it: the\n/// parent of a \
             relationship route.\nasync fn {name}(state: &{state_type}, {scope_param}id: &LookupKey) -> \
             Result<{entity_ty}, ErrorObject> {{\n{open}    {svc}::{}({arg}, {}(id)?){aw}{map_err}\n}}\n\n",
            f.name,
            resource_names(&m.name).key,
        )),
        Helper::WriteField => out.push_str(&format!(
            "/// Sets the relation field `field` of the `{type_name}` resource `id` to\n/// `value` through \
             `{svc}::{}`, as a resource `PATCH` naming only that\n/// relationship does.\nasync fn \
             {name}(\n    state: &{state_type},\n    {scope_param}id: &str,\n    field: &str,\n    value: \
             serde_json::Value,\n) -> Result<(), ErrorObject> {{\n    let input: {} = \
             from_fields(serde_json::Map::from_iter([(field.to_owned(), value)]))?;\n{open}    {svc}::{}({arg}, \
             id, input){aw}{map_err}?;\n    Ok(())\n}}\n\n",
            f.name,
            extract_input_type(&f.params[1].ty),
            f.name,
        )),
        Helper::CheckIds => {
            let check = match &not_found {
                Some(pattern) => format!(
                    "        match {svc}::{}({arg}, &linked.id){aw} {{\n            Ok(_) => {{}}\n            \
                     Err({pattern}) => return Err(linked.not_found(\"{type_name}\")),\n            Err(e) => return \
                     Err({fallback}(e)),\n        }}\n",
                    f.name
                ),
                None => format!("        {svc}::{}({arg}, &linked.id){aw}{map_err}?;\n", f.name),
            };
            out.push_str(&format!(
                "/// Checks, in order, that each of `ids` names a resource of type `{type_name}`.\nasync fn \
                 {name}(state: &{state_type}, {scope_param}ids: &[LinkedId]) -> Result<(), ErrorObject> \
                 {{\n{open}    for linked in ids {{\n{check}    }}\n    Ok(())\n}}\n\n"
            ));
        }
        Helper::Fetch => {
            let fetch = match &not_found {
                Some(pattern) => format!(
                    "        match {svc}::{}({arg}, id){aw} {{\n            Ok(entity) => found.push(entity),\n            \
                     Err({pattern}) => {{}}\n            Err(e) => return Err({fallback}(e)),\n        }}\n",
                    f.name
                ),
                None => format!("        found.push({svc}::{}({arg}, id){aw}{map_err}?);\n", f.name),
            };
            out.push_str(&format!(
                "/// The `{type_name}` resources `ids` name, in order, without the ids that\n/// name none.\nasync \
                 fn {name}(state: &{state_type}, {scope_param}ids: &[String]) -> Result<Vec<{entity_ty}>, \
                 ErrorObject> {{\n{open}    let mut found = Vec::with_capacity(ids.len());\n    for id in ids \
                 {{\n{fetch}    }}\n    Ok(found)\n}}\n\n"
            ));
        }
    }
}

/// One of the type's relationships.
enum Rel<'r, 'a> {
    Field(&'r Relationship),
    Junction(&'r JunctionRelationship<'a>),
}

impl Rel<'_, '_> {
    fn name(&self) -> &str {
        match self {
            Rel::Field(rel) => &rel.name,
            Rel::Junction(j) => &j.name,
        }
    }
}

/// Emit the relationship and related handlers of resource module `m`, under
/// the scope of its `get_by_id`, and register their routes.
pub(super) fn emit(out: &mut String, routes: &mut Routes, m: &ApiModule, modules: &[ApiModule], config: &Config) {
    let Some(served) = Served::new(m, modules, config) else { return };
    let rels: Vec<Rel<'_, '_>> = served
        .resource
        .relationships
        .iter()
        .map(Rel::Field)
        .chain(served.junctions.iter().map(Rel::Junction))
        .collect();
    let handler = |kind: &str| handler_name(m, kind, served.scope.is_some());

    out.push_str(&get_handler(&served, &rels, &handler("relationship_get"), false));
    for method in [Method::Patch, Method::Post, Method::Delete] {
        out.push_str(&write_handler(&served, &rels, &handler(method.handler()), method));
    }
    out.push_str(&get_handler(&served, &rels, &handler("related_get"), true));

    for (method, path, handler) in route_table(m, config) {
        routes.add(&path, method, &handler);
    }
}

/// The relationship and related routes of resource module `m`, each as its
/// routing method, path and handler, in registration order: under the
/// prefix when its `get_by_id` is scoped, and none when it serves no
/// relationship routes. The generator registers exactly these and the
/// server metadata reports them, so the two cannot disagree.
pub(in crate::servers) fn route_table(m: &ApiModule, config: &Config) -> Vec<(&'static str, String, String)> {
    let Some(get) = relationship_routes(m, config) else { return Vec::new() };
    let scope = config.route_prefix.as_ref().filter(|_| is_scoped(get, config));
    let handler = |kind: &str| handler_name(m, kind, scope.is_some());
    let url = config.naming.url_for_module(m);
    let base = match scope {
        None => format!("/api/{url}"),
        Some(prefix) => format!("/api/{}/{url}", axum_path(&prefix.segments)),
    };
    let relationship = format!("{base}/{{id}}/relationships/{{rel}}");
    let mut table = vec![("get", relationship.clone(), handler("relationship_get"))];
    for method in [Method::Patch, Method::Post, Method::Delete] {
        table.push((method.routing(), relationship.clone(), handler(method.handler())));
    }
    table.push(("get", format!("{base}/{{id}}/{{rel}}"), handler("related_get")));
    table
}

/// The name of `m`'s relationship handler of `kind`, `_scoped` for the
/// scoped one.
fn handler_name(m: &ApiModule, kind: &str, scoped: bool) -> String {
    let suffix = if scoped { "_scoped" } else { "" };
    format!("ontogen_{}_{kind}{suffix}", m.name)
}

/// The path extractor's pattern and type: the prefix parameter as
/// [`SCOPE`] when scoped, then `id` and `rel`.
fn path_extract(served: &Served<'_>) -> (String, String) {
    match served.scope {
        None => ("(id, rel)".to_string(), "(LookupKey, LookupKey)".to_string()),
        Some(prefix) => {
            (format!("({SCOPE}, id, rel)"), format!("({}, LookupKey, LookupKey)", prefix.params[0].rust_type))
        }
    }
}

/// The relationship (`related` false) or related (`related` true) `GET`
/// handler: linkage, or the related resources (§9.1, §9.3).
fn get_handler(served: &Served<'_>, rels: &[Rel<'_, '_>], name: &str, related: bool) -> String {
    let state_type = &served.config.state_type;
    let type_name = &served.resource.resource_type;
    let (pattern, ty) = path_extract(served);
    let collection = collection_expr(&served.config.naming.url_for_module(served.m), served.scope);
    let mut out = format!(
        "async fn {name}(\n    State(ontogen_state): State<Arc<{state_type}>>,\n    _: AcceptGuard,\n    \
         Path({pattern}): Path<{ty}>,\n    RawQuery(ontogen_raw_query): RawQuery,\n) -> Result<Response, \
         ErrorObject> {{\n    let ontogen_collection = {collection};\n    match rel.as_str() {{\n"
    );
    for rel in rels {
        let arm = match (rel, related) {
            (Rel::Field(rel), false) => field_linkage(served, rel),
            (Rel::Field(rel), true) => field_related(served, rel),
            (Rel::Junction(j), _) => junction_get(served, j, related),
        };
        out.push_str(&format!("        Some(\"{}\") => {{\n{arm}        }}\n", rel.name()));
    }
    out.push_str(&format!("        _ => Err(relationship_not_found(\"{type_name}\", &rel)),\n    }}\n}}\n\n"));
    out
}

/// The lines every `GET` arm starts with: the query (§13.2 step 5) and the
/// parent (step 8). A paginated junction reads its page.
fn get_prelude(served: &Served<'_>, page: Option<(u32, u32)>) -> String {
    let query = match page {
        None => "            QueryParams::parse(ontogen_raw_query.as_deref(), &QuerySpec::NONE)?;\n".to_string(),
        Some((default_limit, max_limit)) => format!(
            "            let ontogen_query =\n                QueryParams::parse(ontogen_raw_query.as_deref(), \
             &QuerySpec {{ page: true, ..QuerySpec::NONE }})?;\n            let (ontogen_offset, ontogen_limit) = \
             page(&ontogen_query, {default_limit}, {max_limit})?;\n"
        ),
    };
    format!(
        "{query}            let ontogen_entity = {}&id).await?;\n",
        served.call(Helper::Read, &served.m.name, served.get)
    )
}

/// The parent's id, as the entity holds it.
fn parent_id(served: &Served<'_>) -> String {
    format!("ontogen_entity.{}", served.resource.id_field)
}

/// `GET …/relationships/{rel}` of a field relationship: its linkage, read
/// from the parent.
fn field_linkage(served: &Served<'_>, rel: &Relationship) -> String {
    let enc = encode_path_segment(&rel.name);
    format!(
        "{}            let ontogen_base = format!(\"{{ontogen_collection}}/{{}}\", \
         encode_path_segment(&{}));\n            let ontogen_links = \
         Links::new(format!(\"{{ontogen_base}}/relationships/{enc}\"))\n                \
         .with_related(format!(\"{{ontogen_base}}/{enc}\"));\n            let ontogen_data = {};\n            \
         Ok(response::ok(&Document::new(ontogen_data, ontogen_links)))\n",
        get_prelude(served, None),
        parent_id(served),
        linkage_expr(rel, "ontogen_entity", "ontogen_member"),
    )
}

/// `GET …/{rel}` of a field relationship: the resources its linkage names,
/// in order. A dangling id is skipped, and a to-one naming nothing is
/// `null`.
fn field_related(served: &Served<'_>, rel: &Relationship) -> String {
    let (tm, tf) = served.target(&rel.target_module);
    let field = format!("ontogen_entity.{}", rel.field);
    let ids = match rel.arity {
        Arity::ToOne { nullable: true } => format!("{field}.as_slice()"),
        Arity::ToOne { nullable: false } => format!("std::slice::from_ref(&{field})"),
        Arity::ToMany => format!("&{field}"),
    };
    let as_resource = resource_names(&tm.name).resource;
    let collection = served.target_collection(&rel.target_type, tf);
    let data = if rel.is_to_many() {
        format!(
            "let ontogen_data: Vec<_> =\n                ontogen_related.iter().map(|ontogen_member| \
             {as_resource}(ontogen_member, {collection})).collect();"
        )
    } else {
        format!(
            "let ontogen_data =\n                ontogen_related.first().map(|ontogen_member| \
             {as_resource}(ontogen_member, {collection}));"
        )
    };
    format!(
        "{}            let ontogen_related = {}{ids}).await?;\n            {data}\n            let ontogen_self = \
         format!(\"{{ontogen_collection}}/{{}}/{}\", encode_path_segment(&{}));\n            \
         Ok(response::ok(&Document::new(ontogen_data, Links::new(ontogen_self))))\n",
        get_prelude(served, None),
        served.call(Helper::Fetch, &tm.name, tf),
        encode_path_segment(&rel.name),
        parent_id(served),
    )
}

/// The lines opening the stores `fns` take, each once, in a handler arm
/// indented by `indent`, and each fn's leading argument (`None` for a
/// stateless fn).
fn open_stores(served: &Served<'_>, fns: &[&ApiFn], indent: &str) -> (String, Vec<Option<String>>) {
    let mut opens: Vec<String> = Vec::new();
    let args = fns
        .iter()
        .map(|f| {
            if f.is_stateless {
                return None;
            }
            if !f.first_param_is_store {
                return Some("&ontogen_state".to_string());
            }
            let (binding, open) = match served.scope {
                Some(prefix) => (
                    "ontogen_store".to_string(),
                    format!("ontogen_state.{}(&{SCOPE}).map_err(ontogen_internal_error)?", prefix.state_accessor),
                ),
                None => {
                    let accessor = &f.store_accessor;
                    let binding = if accessor == "store" {
                        "ontogen_store".to_string()
                    } else {
                        format!("ontogen_{accessor}_store")
                    };
                    (binding, format!("ontogen_state.{accessor}().await.map_err(ontogen_internal_error)?"))
                }
            };
            let line = format!("{indent}let {binding} = {open};\n");
            if !opens.contains(&line) {
                opens.push(line);
            }
            Some(format!("&{binding}"))
        })
        .collect();
    (opens.concat(), args)
}

/// A call of junction op `f` with `args` after its state or store.
fn junction_call(served: &Served<'_>, f: &ApiFn, first: Option<&String>, args: &[String]) -> String {
    let svc = served.m.service_ident(f.surface);
    let all: Vec<&str> = first.into_iter().map(String::as_str).chain(args.iter().map(String::as_str)).collect();
    format!("{svc}::{}({}){}{}?", f.name, all.join(", "), await_str(f.is_async), err_map(f, served.config))
}

/// `expr`, a `String` place, passed as parameter `i` of `f`: borrowed for
/// `&str` and `&String`, cloned for `String`.
fn pass(f: &ApiFn, i: usize, expr: &str) -> String {
    match &f.params[i].ty_ast {
        syn::Type::Reference(_) => format!("&{expr}"),
        _ => format!("{expr}.clone()"),
    }
}

/// The page of a junction relationship read by a paginated `list_X`: its
/// module's default and maximum limit.
fn junction_page(served: &Served<'_>, j: &JunctionRelationship<'_>) -> Option<(u32, u32)> {
    served
        .config
        .pagination_for(&served.m.name, j.list.surface)
        .filter(|_| j.list.return_type.starts_with("Vec<"))
        .map(|pg| (pg.default_limit, pg.max_limit))
}

/// `GET …/relationships/{rel}` (`related` false) or `GET …/{rel}` (`related`
/// true) of a junction relationship: its members, as `list_X` returns them,
/// paged in memory when the module paginates (§9.1).
fn junction_get(served: &Served<'_>, j: &JunctionRelationship<'_>, related: bool) -> String {
    let page = junction_page(served, j);
    let (opens, args) = open_stores(served, &[j.list], "            ");
    let list = junction_call(served, j.list, args[0].as_ref(), &[pass(j.list, 0, &parent_id(served))]);
    let enc = encode_path_segment(&j.name);
    let (tm, tf) = served.target(&j.target_module);
    let target = served.config.resources.by_module(&tm.name).expect("a junction targets a resource");
    let mut out = format!("{}{opens}            let ontogen_members = {list};\n", get_prelude(served, page));
    let members = if page.is_some() {
        out.push_str("            let ontogen_total = ontogen_members.len() as u64;\n");
        "ontogen_members.iter().skip(ontogen_offset as usize).take(ontogen_limit as usize)"
    } else {
        "ontogen_members.iter()"
    };
    let data = if related {
        let as_resource = resource_names(&tm.name).resource;
        let collection = served.target_collection(&j.target_type, tf);
        let to_resource = format!(".map(|ontogen_member| {as_resource}(ontogen_member, {collection})).collect()");
        if j.lists_entities {
            format!("            let ontogen_data: Vec<_> = {members}{to_resource};\n")
        } else {
            format!(
                "            let ontogen_ids: Vec<String> = {members}.cloned().collect();\n            let \
                 ontogen_related = {}&ontogen_ids).await?;\n            let ontogen_data: Vec<_> = \
                 ontogen_related.iter(){to_resource};\n",
                served.call(Helper::Fetch, &tm.name, tf)
            )
        }
    } else {
        let id = if j.lists_entities {
            format!("ontogen_member.{}.as_str()", target.id_field)
        } else {
            "ontogen_member.as_str()".to_string()
        };
        format!(
            "            let ontogen_data = Linkage::ToMany(\n                {members}.map(|ontogen_member| \
             ResourceIdentifier::new(\"{}\", {id})).collect(),\n            );\n",
            j.target_type
        )
    };
    out.push_str(&data);
    let parent = parent_id(served);
    let links = match page {
        Some(_) => {
            "pagination_links(&ontogen_self, &CanonicalQuery::new(), ontogen_offset, ontogen_limit, \
                    ontogen_total)"
        }
        None => "Links::new(ontogen_self)",
    };
    // A relationship document carries the spec's `related` link beside
    // `self`, so a client holding the linkage reaches the resources it names.
    let (self_line, links) = if related {
        (
            format!(
                "let ontogen_self = format!(\"{{ontogen_collection}}/{{}}/{enc}\", encode_path_segment(&{parent}));"
            ),
            links.to_string(),
        )
    } else {
        (
            format!(
                "let ontogen_base = format!(\"{{ontogen_collection}}/{{}}\", encode_path_segment(&{parent}));\n            \
                 let ontogen_self = format!(\"{{ontogen_base}}/relationships/{enc}\");"
            ),
            format!("{links}.with_related(format!(\"{{ontogen_base}}/{enc}\"))"),
        )
    };
    let document = match page {
        Some(_) => {
            "Document::new(ontogen_data, ontogen_links)\n                .with_meta(PageMeta { total: \
                    ontogen_total, limit: ontogen_limit, offset: ontogen_offset })"
        }
        None => "Document::new(ontogen_data, ontogen_links)",
    };
    out.push_str(&format!(
        "            {self_line}\n            let ontogen_links = {links};\n            Ok(response::ok(&{document}))\n"
    ));
    out
}

/// A write method of the relationship route.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Method {
    Patch,
    Post,
    Delete,
}

impl Method {
    fn handler(self) -> &'static str {
        match self {
            Method::Patch => "relationship_patch",
            Method::Post => "relationship_post",
            Method::Delete => "relationship_delete",
        }
    }

    fn routing(self) -> &'static str {
        match self {
            Method::Patch => "patch",
            Method::Post => "post",
            Method::Delete => "delete",
        }
    }

    fn http(self) -> &'static str {
        match self {
            Method::Patch => "PATCH",
            Method::Post => "POST",
            Method::Delete => "DELETE",
        }
    }
}

/// The `PATCH`, `POST` or `DELETE` handler of the relationship route (§9.1,
/// §9.2): each arm answers its query, refuses a write the relationship
/// does not support, reads the body, checks the parent and the linked
/// resources, then writes.
fn write_handler(served: &Served<'_>, rels: &[Rel<'_, '_>], name: &str, method: Method) -> String {
    let state_type = &served.config.state_type;
    let type_name = &served.resource.resource_type;
    let (pattern, ty) = path_extract(served);
    let arms: Vec<(&str, Option<String>)> = rels
        .iter()
        .map(|rel| {
            let arm = match rel {
                Rel::Field(rel) => field_write(served, rel, method),
                Rel::Junction(j) => junction_write(served, j, method),
            };
            (rel.name(), arm)
        })
        .collect();
    // A handler whose every arm refuses reads neither the state nor the
    // body, but still extracts the body for its media type (§13.2 step 3).
    let writes = arms.iter().any(|(_, arm)| arm.is_some());
    let (state, body, pattern) = if writes {
        (format!("    State(ontogen_state): State<Arc<{state_type}>>,\n"), "ontogen_body", pattern)
    } else {
        let unused = if served.scope.is_some() { "(_, _, rel)" } else { "(_, rel)" };
        (String::new(), "_", unused.to_string())
    };
    let mut out = format!(
        "async fn {name}(\n{state}    _: AcceptGuard,\n    ontogen_path: Result<Path<{ty}>, ErrorObject>,\n    \
         ontogen_query: Result<Query<NoParams>, ErrorObject>,\n    {body}: Body,\n) -> Result<Response, \
         ErrorObject> {{\n    let Path({pattern}) = ontogen_path?;\n    match rel.as_str() {{\n"
    );
    for (rel, arm) in arms {
        let arm = arm.unwrap_or_else(|| {
            format!(
                "            Err(relationship_update_unsupported(\"{type_name}\", \"{rel}\", \"{}\"))\n",
                method.http()
            )
        });
        out.push_str(&format!("        Some(\"{rel}\") => {{\n            ontogen_query?;\n{arm}        }}\n"));
    }
    out.push_str(&format!("        _ => Err(relationship_not_found(\"{type_name}\", &rel)),\n    }}\n}}\n\n"));
    out
}

/// The body's linkage (§13.2 step 7) and the parent (step 8), the lines
/// every writing arm starts with.
fn write_prelude(served: &Served<'_>, linkage: &str) -> String {
    format!(
        "            let ontogen_bytes = ontogen_body.into_bytes()?;\n            let ontogen_linked = \
         {linkage};\n            let ontogen_entity = {}&id).await?;\n",
        served.call(Helper::Read, &served.m.name, served.get)
    )
}

/// The identifiers of a `POST` or `DELETE` body: at most one (§9).
fn one_identifier(target_type: &str) -> String {
    format!("request::to_many_linked(&request::parse_relationship(&ontogen_bytes)?, \"\", \"{target_type}\", Some(1))?")
}

/// A write arm of field relationship `rel`, or `None` when `method` is
/// refused (§9's table): `POST` and `DELETE` on a to-one, and every write
/// when the module serves no `update`.
fn field_write(served: &Served<'_>, rel: &Relationship, method: Method) -> Option<String> {
    let update = served.update?;
    let (tm, tf) = served.target(&rel.target_module);
    let target = &rel.target_type;
    let field = crate::resource::member_name(&rel.field);
    let parent = parent_id(served);
    let write = |value: &str| {
        format!("{}&{parent}, \"{field}\", {value}).await?", served.call(Helper::WriteField, &served.m.name, update))
    };
    let check = served.call(Helper::CheckIds, &tm.name, tf);
    let body = match (method, rel.arity) {
        (Method::Patch, Arity::ToOne { nullable }) => format!(
            "{}            {check}ontogen_linked.as_slice()).await?;\n            {};\n",
            write_prelude(
                served,
                &format!(
                    "request::to_one(&request::parse_relationship(&ontogen_bytes)?, \"\", \"{target}\", {nullable})?\n                \
                     .map(|ontogen_member| LinkedId {{ id: ontogen_member, pointer: \"/data\".to_owned() }})"
                )
            ),
            write("ontogen_linked.map(|ontogen_member| ontogen_member.id).into()"),
        ),
        (Method::Patch, Arity::ToMany) => format!(
            "{}            {check}&ontogen_linked).await?;\n            {};\n",
            write_prelude(
                served,
                &format!(
                    "request::to_many_linked(&request::parse_relationship(&ontogen_bytes)?, \"\", \"{target}\", None)?"
                )
            ),
            write("ontogen_linked.into_iter().map(|ontogen_member| ontogen_member.id).collect()"),
        ),
        (_, Arity::ToOne { .. }) => return None,
        (Method::Post, Arity::ToMany) => format!(
            "{}            {check}&ontogen_linked).await?;\n            if let Some(ontogen_ids) = \
             ontogen_added(&ontogen_entity.{}, &ontogen_linked) {{\n                {};\n            }}\n",
            write_prelude(served, &one_identifier(target)),
            rel.field,
            write("ontogen_ids.into()"),
        ),
        (Method::Delete, Arity::ToMany) => format!(
            "{}            if let Some(ontogen_ids) = ontogen_removed(&ontogen_entity.{}, &ontogen_linked) {{\n                \
             {};\n            }}\n",
            write_prelude(served, &one_identifier(target)),
            rel.field,
            write("ontogen_ids.into()"),
        ),
    };
    Some(format!("{body}            Ok(response::no_content())\n"))
}

/// A write arm of junction relationship `j`, or `None` when `method` is
/// refused: `PATCH` always, `POST` without `add_Y`, `DELETE` without
/// `remove_Y`. A member is added or removed only when the membership read
/// says it must be, so a repeated `POST` or the `DELETE` of a non-member
/// succeeds without calling user code (§9.1).
fn junction_write(served: &Served<'_>, j: &JunctionRelationship<'_>, method: Method) -> Option<String> {
    let op = match method {
        Method::Patch => return None,
        Method::Post => j.add?,
        Method::Delete => j.remove?,
    };
    let (tm, tf) = served.target(&j.target_module);
    let indent = "                ";
    let (opens, args) = open_stores(served, &[j.list, op], indent);
    let parent = parent_id(served);
    let list = junction_call(served, j.list, args[0].as_ref(), &[pass(j.list, 0, &parent)]);
    let write = junction_call(served, op, args[1].as_ref(), &[pass(op, 0, &parent), pass(op, 1, "ontogen_child.id")]);
    let member = if j.lists_entities {
        let target = served.config.resources.by_module(&tm.name).expect("a junction targets a resource");
        format!("ontogen_members.iter().any(|ontogen_member| ontogen_member.{} == ontogen_child.id)", target.id_field)
    } else {
        "ontogen_members.contains(&ontogen_child.id)".to_string()
    };
    let (check, condition) = match method {
        Method::Post => (
            format!("            {}&ontogen_linked).await?;\n", served.call(Helper::CheckIds, &tm.name, tf)),
            format!("!{member}"),
        ),
        _ => (String::new(), member),
    };
    Some(format!(
        "{}{check}            if let Some(ontogen_child) = ontogen_linked.first() {{\n{opens}{indent}let ontogen_members \
         = {list};\n{indent}if {condition} {{\n{indent}    {write};\n{indent}}}\n            }}\n            \
         Ok(response::no_content())\n",
        write_prelude(served, &one_identifier(&j.target_type)),
    ))
}
