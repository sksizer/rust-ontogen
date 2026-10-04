//! The JSON:API half shared by both TypeScript HTTP clients (wire contract
//! §14): the request helpers, `JsonApiError`, one flatten/unflatten pair per
//! resource, emitted from the same relationship table the server uses, and
//! the call each op makes.
//!
//! JSON:API is applied and removed inside the generated client, so callers
//! keep the flat entity shapes the IPC transport also uses.

use ontogen_core::ir::OpKind;
use ontogen_core::naming::to_snake_case;

use crate::clients::config::Config;
use crate::clients::generators::{command_name, ts_params_in_declaration_order};
use crate::resource::{Arity, Resource, member_name};
use crate::servers::classify::classify_op;
use crate::servers::parse::{ApiFn, ApiModule, EventFn, Param};
use crate::servers::types::{extract_input_type, rust_type_to_ts, snake_to_camel, strip_ref};

/// The exported JSON:API types and the error every HTTP call throws (§14.4).
///
/// The error's fields are assigned in the constructor rather than declared as
/// parameter properties so the file also compiles under
/// `erasableSyntaxOnly` and Node's type stripping.
pub(crate) const JSON_API_TYPES: &str = "\
// ── JSON:API ──

export interface JsonApiResourceIdentifier {
  type: string;
  id: string;
}

export interface JsonApiRelationship {
  data?: JsonApiResourceIdentifier | JsonApiResourceIdentifier[] | null;
  links?: Record<string, string | null>;
}

export interface JsonApiResource {
  type: string;
  id: string;
  attributes?: Record<string, unknown>;
  relationships?: Record<string, JsonApiRelationship>;
  links?: Record<string, string | null>;
}

export interface JsonApiErrorObject {
  id?: string;
  status?: string;
  code?: string;
  title?: string;
  detail?: string;
  source?: { pointer?: string; parameter?: string; header?: string };
  meta?: Record<string, unknown>;
}

/** A non-2xx response. `errors` is empty when the body was not a JSON:API error document. */
export class JsonApiError extends Error {
  override readonly name = 'JsonApiError';
  readonly status: number;
  readonly errors: JsonApiErrorObject[];

  constructor(status: number, errors: JsonApiErrorObject[], message: string) {
    super(message);
    this.status = status;
    this.errors = errors;
  }
}

";

/// `BASE` and the fetch helpers. Every request asks for JSON:API, and every
/// request with a body declares it.
pub(crate) fn http_helpers() -> String {
    format!("{HTTP_HELPERS}{TO_QUERY_STRING}")
}

/// `callOp` serves every op that is not served as a resource: its arguments
/// go out as `meta.args` and its result comes back as `meta.result` (§10).
const HTTP_HELPERS: &str = "\
// ── HTTP Helpers ──

const BASE = '/api';
const JSON_API_MEDIA_TYPE = 'application/vnd.api+json';

async function httpRequest(method: string, path: string, body?: unknown): Promise<Response> {
  const headers: Record<string, string> = { Accept: JSON_API_MEDIA_TYPE };
  if (body != null) headers['Content-Type'] = JSON_API_MEDIA_TYPE;
  const res = await fetch(`${BASE}${path}`, {
    method,
    headers,
    body: body != null ? JSON.stringify(body) : null,
  });
  if (!res.ok) throw await toJsonApiError(res);
  return res;
}

async function toJsonApiError(res: Response): Promise<JsonApiError> {
  const body: unknown = await res.json().catch(() => null);
  const errors =
    typeof body === 'object' && body !== null && Array.isArray((body as { errors?: unknown }).errors)
      ? (body as { errors: JsonApiErrorObject[] }).errors
      : [];
  const first = errors[0];
  return new JsonApiError(res.status, errors, first?.detail ?? first?.title ?? res.statusText);
}

async function httpGet<T>(path: string): Promise<T> {
  const res = await httpRequest('GET', path);
  return res.json();
}

async function httpPost<T>(path: string, body?: unknown): Promise<T> {
  const res = await httpRequest('POST', path, body);
  if (res.status === 204) return null as T;
  return res.json();
}

async function httpPatch<T>(path: string, body: unknown): Promise<T> {
  const res = await httpRequest('PATCH', path, body);
  return res.json();
}

async function httpDelete(path: string): Promise<void> {
  await httpRequest('DELETE', path);
}

/**
 * Calls an op that is not served as a resource. `args` travel as the body's
 * `meta.args`, keyed by the op's parameter names; without them no body is
 * sent. Resolves to the reply's `meta.result`, or `null` for a 204.
 */
async function callOp<T>(method: string, path: string, args?: Record<string, unknown>): Promise<T> {
  const res = await httpRequest(method, path, args === undefined ? undefined : { meta: { args } });
  if (res.status === 204) return null as T;
  const doc = (await res.json()) as { meta: { result: T } };
  return doc.meta.result;
}

";

/// Builds `?a=1&b=x` from the defined values. An object value is a JSON:API
/// parameter family (§14.2): `{ filter: query, page: { offset: 0 } }` gives
/// `filter%5Bstatus%5D=…&page%5Boffset%5D=0`. A `null` or `undefined` member,
/// and a family that is itself `null` or `undefined` (an omitted `query`), is
/// skipped. An array value repeats its key; filters take none (§7.3).
const TO_QUERY_STRING: &str = "\
function toQueryString(params: Record<string, unknown>): string {
  const parts: string[] = [];
  const push = (key: string, value: unknown) => {
    if (value == null) return;
    for (const v of Array.isArray(value) ? value : [value]) {
      parts.push(`${key}=${encodeURIComponent(String(v))}`);
    }
  };
  for (const [key, value] of Object.entries(params)) {
    if (typeof value === 'object' && value !== null && !Array.isArray(value)) {
      for (const [member, v] of Object.entries(value)) {
        push(`${encodeURIComponent(key)}%5B${encodeURIComponent(member)}%5D`, v);
      }
    } else {
      push(encodeURIComponent(key), value);
    }
  }
  return parts.length > 0 ? `?${parts.join('&')}` : '';
}

";

/// The page a paginated list returns, emitted when any surface paginates.
pub(crate) const PAGINATED_RESULT: &str = "\
export interface PaginatedResult<T> {
  items: T[];
  total: number;
  limit: number;
  offset: number;
}

";

/// The document shapes and the table-driven `unflattenResource`, emitted once
/// when any resource has a generated CRUD method.
pub(crate) const RESOURCE_HELPERS: &str = "\
// ── JSON:API Resources ──

interface JsonApiResourceDef {
  type: string;
  idField: string;
  /** Keyed by relationship name. */
  relationships: Record<string, { field: string; type: string; many: boolean }>;
}

interface JsonApiResourceDocument {
  data: JsonApiResource;
}

interface JsonApiCollectionDocument {
  data: JsonApiResource[];
}

interface JsonApiPageDocument {
  data: JsonApiResource[];
  meta: { total: number; limit: number; offset: number };
}

interface JsonApiWriteDocument {
  data: {
    type: string;
    id?: string;
    attributes: Record<string, unknown>;
    relationships?: Record<string, JsonApiRelationship>;
  };
}

function toOneId(rel: JsonApiRelationship | undefined): string | null {
  const data = rel?.data;
  return data != null && !Array.isArray(data) ? data.id : null;
}

function toManyIds(rel: JsonApiRelationship | undefined): string[] {
  const data = rel?.data;
  return Array.isArray(data) ? data.map((i) => i.id) : [];
}

/**
 * A flat create or update input as a resource document. `undefined` keys are
 * left out, which an update reads as unchanged; `null` is kept and clears.
 * A to-many `null` is left out too: `{ data: null }` is not valid linkage for
 * it, and the flat update input reads `null` there as unchanged.
 */
function unflattenResource(def: JsonApiResourceDef, input: object, id?: string): JsonApiWriteDocument {
  let resourceId = id;
  const attributes: Record<string, unknown> = {};
  const relationships: Record<string, JsonApiRelationship> = {};
  const relByField = new Map(
    Object.entries(def.relationships).map(([name, rel]) => [rel.field, { name, ...rel }] as const),
  );
  for (const [key, value] of Object.entries(input)) {
    if (value === undefined) continue;
    if (key === def.idField) {
      // An empty create id asks the server to derive one.
      if (id === undefined && typeof value === 'string' && value !== '') resourceId = value;
      continue;
    }
    const rel = relByField.get(key);
    if (rel === undefined) {
      attributes[key] = value;
    } else if (rel.many) {
      if (Array.isArray(value)) {
        relationships[rel.name] = { data: value.map((v) => ({ type: rel.type, id: String(v) })) };
      }
    } else {
      relationships[rel.name] = { data: value === null ? null : { type: rel.type, id: String(value) } };
    }
  }
  return {
    data: {
      type: def.type,
      ...(resourceId !== undefined ? { id: resourceId } : {}),
      attributes,
      ...(Object.keys(relationships).length > 0 ? { relationships } : {}),
    },
  };
}

";

/// `flattenTask`.
pub(crate) fn flatten_fn(resource: &Resource) -> String {
    format!("flatten{}", resource.entity.name)
}

/// `unflattenTask`.
pub(crate) fn unflatten_fn(resource: &Resource) -> String {
    format!("unflatten{}", resource.entity.name)
}

/// One resource's relationship table, `flattenX` and `unflattenX` (§14.3).
pub(crate) fn resource_codec(resource: &Resource) -> String {
    let entity = &resource.entity.name;
    let table = format!("{}_RESOURCE", to_snake_case(entity).to_uppercase());
    let id_field = member_name(&resource.id_field);

    let mut out = format!(
        "const {table}: JsonApiResourceDef = {{\n  type: '{}',\n  idField: '{id_field}',\n  relationships: {{",
        resource.resource_type
    );
    if resource.relationships.is_empty() {
        out.push_str("},\n};\n\n");
    } else {
        out.push('\n');
        for rel in &resource.relationships {
            out.push_str(&format!(
                "    {}: {{ field: '{}', type: '{}', many: {} }},\n",
                rel.name,
                member_name(&rel.field),
                rel.target_type,
                rel.is_to_many()
            ));
        }
        out.push_str("  },\n};\n\n");
    }

    out.push_str(&format!(
        "function {}(r: JsonApiResource): {entity} {{\n  return {{\n    {id_field}: r.id,\n    ...r.attributes,\n",
        flatten_fn(resource)
    ));
    for rel in &resource.relationships {
        let read = match rel.arity {
            Arity::ToOne { .. } => "toOneId",
            Arity::ToMany => "toManyIds",
        };
        out.push_str(&format!("    {}: {read}(r.relationships?.['{}']),\n", member_name(&rel.field), rel.name));
    }
    out.push_str(&format!("  }} as {entity};\n}}\n\n"));

    out.push_str(&format!(
        "function {}(input: object, id?: string): JsonApiWriteDocument {{\n  return unflattenResource({table}, \
         input, id);\n}}\n\n",
        unflatten_fn(resource)
    ));
    out
}

/// True when `f` is emitted at all: it has a command name the config does
/// not skip.
fn is_emitted(module: &str, f: &ApiFn, config: &Config) -> bool {
    let cmd = command_name(module, f, config);
    !cmd.is_empty() && !config.ts_skip_commands.contains(&cmd)
}

/// How an op is served over HTTP, which decides the call that reaches it.
pub(crate) enum Served<'a> {
    /// As its resource (§5–§8), a filtered `list` included (§7.3).
    Resource(&'a Resource),
    /// As a custom op (§10): arguments in `meta.args`, the result in
    /// `meta.result`. Custom ops, junction ops, and CRUD ops with no
    /// resource behind them (§10.4), a filtered `list` included.
    Op,
}

/// How `f` of `module` is served, by the predicate the server's routes
/// follow ([`ResourceModel::serving`]).
///
/// [`ResourceModel::serving`]: crate::resource::ResourceModel::serving
pub(crate) fn served<'a>(module: &ApiModule, f: &ApiFn, config: &'a Config) -> Served<'a> {
    match config.resources.serving(&module.name, f) {
        Some(resource) => Served::Resource(resource),
        None => Served::Op,
    }
}

/// The resource an event op's frames carry, or `None` when they carry its
/// item as `meta.result` (§12).
pub(crate) fn event_resource<'a>(ev: &EventFn, config: &'a Config) -> Option<&'a Resource> {
    config.resources.by_item_type(&ev.item_type_ast)
}

/// Every resource that needs a flatten/unflatten pair, in module order: each
/// with at least one emitted method served as it and, with `events`, each an
/// event op's frames carry.
pub(crate) fn served_resources<'a>(modules: &[ApiModule], config: &'a Config, events: bool) -> Vec<&'a Resource> {
    let mut resources: Vec<&Resource> = modules
        .iter()
        .filter_map(|m| {
            m.functions.iter().filter(|f| is_emitted(&m.name, f, config)).find_map(|f| match served(m, f, config) {
                Served::Resource(r) => Some(r),
                Served::Op => None,
            })
        })
        .collect();
    if events {
        for r in modules.iter().flat_map(|m| &m.events).filter_map(|ev| event_resource(ev, config)) {
            if !resources.iter().any(|known| known.module == r.module) {
                resources.push(r);
            }
        }
    }
    resources
}

/// The expression that turns event frame `frame` into the flat item the
/// handlers receive: an entity's resource object is flattened, any other
/// item is the frame's `meta.result`.
pub(crate) fn decode_event_frame(ev: &EventFn, config: &Config, frame: &str) -> String {
    match event_resource(ev, config) {
        Some(r) => format!("{}({frame} as JsonApiResource)", flatten_fn(r)),
        None => format!("metaResult<{}>({frame})", rust_type_to_ts(&ev.item_type)),
    }
}

/// Reads a non-resource event frame (§12), emitted with the SSE helpers.
pub(crate) const META_RESULT: &str = "\
/** An event frame whose item is not a resource carries it as `meta.result`. */
function metaResult<T>(frame: unknown): T {
  return (frame as { meta: { result: T } }).meta.result;
}

";

/// A method of either HTTP client.
pub(crate) struct Method {
    /// Its parameters, ahead of any route-prefix parameter.
    pub params: Vec<String>,
    pub return_type: String,
    /// Its statements, in order. A statement spanning lines indents its
    /// continuation lines relative to its first.
    pub body: Vec<String>,
}

/// The method `f` of module `m` gets in either HTTP client, whose
/// parameters and return type the `Transport` interface declares, or `None`
/// for an event op. `scope` names the route-prefix parameter (`projectId`) of a
/// client that calls scoped routes through `scopedPath`, and is `None` for
/// one that calls only unscoped routes.
pub(crate) fn method(m: &ApiModule, f: &ApiFn, config: &Config, scope: Option<&str>) -> Option<Method> {
    let scope = scope_of(f, scope);
    let base = config.naming.url_for_module(m);
    let fetch = |p: &str| fetch(p, scope);
    let ret = if f.return_type == "()" { "null".to_string() } else { rust_type_to_ts(&f.return_type) };
    let input_type = |i: usize| rust_type_to_ts(&extract_input_type(&f.params[i].ty));
    let id_path = format!("/{base}/${{encodeURIComponent(id)}}");
    let op = classify_op(f);
    let resource = match served(m, f, config) {
        Served::Resource(r) => r,
        Served::Op if op == OpKind::List => return Some(list_method(m, f, config, scope)),
        Served::Op => {
            let (params, return_type) = match op {
                OpKind::EventStream => return None,
                OpKind::GetById => (vec!["id: string".to_string()], ret),
                OpKind::Create => (vec![format!("input: {}", input_type(0))], ret),
                OpKind::Update => (vec!["id: string".to_string(), format!("input: {}", input_type(1))], ret),
                OpKind::Delete => (vec!["id: string".to_string()], "null".to_string()),
                OpKind::JunctionList { .. } if is_paginated(m, f, config) => {
                    let mut params = ts_params_in_declaration_order(f);
                    params.push("limit?: number".to_string());
                    params.push("offset?: number".to_string());
                    (params, paginated_result(&ret))
                }
                OpKind::JunctionAdd { .. } | OpKind::JunctionRemove { .. } => {
                    (ts_params_in_declaration_order(f), "null".to_string())
                }
                OpKind::List | OpKind::JunctionList { .. } | OpKind::CustomGet | OpKind::CustomPost => {
                    (ts_params_in_declaration_order(f), ret)
                }
            };
            let body = op_body(m, f, config, &return_type, scope);
            return Some(Method { params, return_type, body });
        }
    };

    let method = match op {
        OpKind::List => list_method(m, f, config, scope),
        OpKind::GetById => Method {
            params: vec!["id: string".to_string()],
            return_type: ret,
            body: vec![
                format!("const {{ data }} = await httpGet<JsonApiResourceDocument>({});", fetch(&id_path)),
                format!("return {}(data);", flatten_fn(resource)),
            ],
        },
        OpKind::Create => Method {
            params: vec![format!("input: {}", input_type(0))],
            return_type: ret,
            body: vec![
                format!(
                    "const {{ data }} = await httpPost<JsonApiResourceDocument>({}, {}(input));",
                    fetch(&format!("/{base}")),
                    unflatten_fn(resource)
                ),
                format!("return {}(data);", flatten_fn(resource)),
            ],
        },
        OpKind::Update => Method {
            params: vec!["id: string".to_string(), format!("input: {}", input_type(1))],
            return_type: ret,
            body: vec![
                format!(
                    "const {{ data }} = await httpPatch<JsonApiResourceDocument>(\n  {},\n  {}(input, id),\n);",
                    fetch(&id_path),
                    unflatten_fn(resource)
                ),
                format!("return {}(data);", flatten_fn(resource)),
            ],
        },
        OpKind::Delete => Method {
            params: vec!["id: string".to_string()],
            return_type: "null".to_string(),
            body: vec![format!("await httpDelete({});", fetch(&id_path)), "return null;".to_string()],
        },
        _ => unreachable!("a resource serves CRUD ops only"),
    };
    Some(method)
}

/// The scope `f`'s calls take from a client's `scope`: only a store-scoped
/// op is served under the route prefix; any other keeps its unscoped route.
pub(crate) fn scope_of<'a>(f: &ApiFn, scope: Option<&'a str>) -> Option<&'a str> {
    scope.filter(|_| f.first_param_is_store)
}

/// The path expression a call fetches: `path` as a template literal when it
/// interpolates, else as a string, through `scopedPath` for a client with a
/// `scope`.
fn fetch(path: &str, scope: Option<&str>) -> String {
    let literal = if path.contains("${") { format!("`{path}`") } else { format!("'{path}'") };
    match scope {
        Some(prefix_arg) => format!("scopedPath({prefix_arg}, {literal})"),
        None => literal,
    }
}

/// The call that reaches an op served as a custom op (§10), as the server's
/// `op_shape` routes it: the HTTP method, the path (template-literal text
/// under `/api`), a list's `filter` family ([`filter_family`]), the `opArg`
/// members and the `meta.args` members, each member a
/// `(Rust name, TS expression)` pair.
#[derive(PartialEq, Eq)]
struct OpRoute<'a> {
    method: &'static str,
    path: String,
    filter: Option<String>,
    op_args: Vec<(&'a str, String)>,
    args: Vec<(&'a str, String)>,
}

/// The route `f` of `m` is called at, `scoped` when it is served under the
/// route prefix (§11.1). There a junction op is an action-style custom op:
/// its list a GET taking every argument in the path, its add and remove
/// POSTs taking both in `meta.args`. Unscoped, it keeps its nested route.
fn op_route<'a>(m: &ApiModule, f: &'a ApiFn, config: &Config, scoped: bool) -> OpRoute<'a> {
    let classified = classify_op(f);
    let op = match &classified {
        OpKind::JunctionList { .. } if scoped => OpKind::CustomGet,
        OpKind::JunctionAdd { .. } | OpKind::JunctionRemove { .. } if scoped => OpKind::CustomPost,
        op => op.clone(),
    };
    let base = format!("/{}", config.naming.url_for_module(m));
    let value = |p: &Param| if p.is_input() { "input".to_string() } else { snake_to_camel(&p.name) };
    let segment = |p: &Param| path_segment(&value(p));
    let (method, mut path, named) = match &op {
        OpKind::CustomGet | OpKind::CustomPost => {
            let action = config.naming.derive_action(&m.name, &f.name);
            let path = if action.is_empty() { base } else { format!("{base}/{action}") };
            (if op == OpKind::CustomGet { "GET" } else { "POST" }, path, 0)
        }
        OpKind::List => ("GET", base, 0),
        OpKind::GetById => ("GET", format!("{base}{}", path_segment("id")), 1),
        OpKind::Create => ("POST", base, 0),
        OpKind::Update => ("PATCH", format!("{base}{}", path_segment("id")), 1),
        OpKind::Delete => ("DELETE", format!("{base}{}", path_segment("id")), 1),
        OpKind::JunctionList { child_segment } => {
            ("GET", format!("{base}{}/{child_segment}", segment(&f.params[0])), 1)
        }
        OpKind::JunctionAdd { child_segment } => {
            ("POST", format!("{base}{}/{child_segment}", segment(&f.params[0])), 1)
        }
        OpKind::JunctionRemove { child_segment } => {
            ("DELETE", format!("{base}{}/{child_segment}{}", segment(&f.params[0]), segment(&f.params[1])), 2)
        }
        OpKind::EventStream => unreachable!("event fns are not ApiFns"),
    };
    let rest = &f.params[named.min(f.params.len())..];
    let page = || {
        if is_paginated(m, f, config) {
            vec![("limit", "limit".to_string()), ("offset", "offset".to_string())]
        } else {
            Vec::new()
        }
    };
    let mut filter = None;
    let mut op_args = Vec::new();
    let mut args = Vec::new();
    match op {
        // A list's parameters are its filter and its page (§10.4).
        OpKind::List => {
            filter = filter_family(f);
            op_args = page();
        }
        OpKind::JunctionList { .. } => op_args = page(),
        OpKind::CustomGet if matches!(classified, OpKind::JunctionList { .. }) => {
            path.extend(rest.iter().map(segment));
            op_args = page();
        }
        _ if matches!(method, "POST" | "PATCH") => args = rest.iter().map(|p| (p.name.as_str(), value(p))).collect(),
        _ => {
            for p in rest {
                if p.is_option() {
                    op_args.push((p.name.as_str(), value(p)));
                } else {
                    path.push_str(&segment(p));
                }
            }
        }
    }
    OpRoute { method, path, filter, op_args, args }
}

/// The statements of an op served as a custom op. A client with a `scope`
/// calls the scoped route when given the prefix argument and the unscoped
/// one otherwise: through `scopedPath` alone when both share a shape, else
/// by branching on the argument.
fn op_body(m: &ApiModule, f: &ApiFn, config: &Config, return_type: &str, scope: Option<&str>) -> Vec<String> {
    let unscoped = op_route(m, f, config, false);
    let Some(prefix_arg) = scope else { return vec![op_call(return_type, &unscoped, None)] };
    let scoped = op_route(m, f, config, true);
    if scoped == unscoped {
        return vec![op_call(return_type, &unscoped, scope)];
    }
    vec![
        format!("if ({prefix_arg}) {{\n  {}\n}}", op_call(return_type, &scoped, scope)),
        op_call(return_type, &unscoped, None),
    ]
}

/// `/${encodeURIComponent(parentId)}`: the TS expression `value` as one path
/// segment.
fn path_segment(value: &str) -> String {
    format!("/${{encodeURIComponent({value})}}")
}

/// `${toQueryString({ filter: query, opArg: { verbose } })}`: the query
/// string of the parameter `families`, each a `(family, TS object
/// expression)` pair, or nothing for none. `toQueryString` drops the members
/// left undefined.
fn query_string(families: &[(&str, String)]) -> String {
    if families.is_empty() {
        return String::new();
    }
    format!("${{toQueryString({})}}", object_literal(families))
}

/// The `filter` family a list sends (§7.3), as a TS object expression:
/// `query`, its `*Query` struct; `{ skill_id: skillId }`, its bare filters
/// keyed by their Rust names; or `{ ...query, skill_id: skillId }`, both.
/// `None` for a list that takes no filter.
fn filter_family(f: &ApiFn) -> Option<String> {
    let bare: Vec<(&str, String)> =
        f.bare_filters().into_iter().map(|p| (p.name.as_str(), snake_to_camel(&p.name))).collect();
    match (f.filter_struct(), bare.is_empty()) {
        (None, true) => None,
        (Some(_), true) => Some("query".to_string()),
        (None, false) => Some(object_literal(&bare)),
        (Some(_), false) => Some(format!("{{ ...query, {} }}", object_members(&bare))),
    }
}

/// `return callOp<R>('METHOD', path, { args });` for `route`, without the
/// argument object when there are no arguments.
fn op_call(return_type: &str, route: &OpRoute, scope: Option<&str>) -> String {
    let mut families = Vec::new();
    if let Some(filter) = &route.filter {
        families.push(("filter", filter.clone()));
    }
    if !route.op_args.is_empty() {
        families.push(("opArg", object_literal(&route.op_args)));
    }
    let path = fetch(&format!("{}{}", route.path, query_string(&families)), scope);
    let args = if route.args.is_empty() { String::new() } else { format!(", {}", object_literal(&route.args)) };
    format!("return callOp<{return_type}>('{}', {path}{args});", route.method)
}

/// `{ a, b: c }`, using the shorthand where a key and its value match.
fn object_literal(members: &[(&str, String)]) -> String {
    format!("{{ {} }}", object_members(members))
}

/// `a, b: c`: the members of [`object_literal`].
fn object_members(members: &[(&str, String)]) -> String {
    let members: Vec<String> = members
        .iter()
        .map(|(key, value)| if key == value { (*key).to_string() } else { format!("{key}: {value}") })
        .collect();
    members.join(", ")
}

/// True when `f` of `m` returns a page: pagination applies only to a list
/// returning `Vec<T>`; any other result type passes through unchanged.
pub(crate) fn is_paginated(m: &ApiModule, f: &ApiFn, config: &Config) -> bool {
    config.pagination_for(&m.name, f.surface).is_some() && f.return_type.starts_with("Vec<")
}

/// `PaginatedResult<T>` for the TS array type `T[]`.
fn paginated_result(array: &str) -> String {
    format!("PaginatedResult<{}>", array.strip_suffix("[]").unwrap_or(array))
}

/// Whether `f`'s `*Query` struct has a field a client must send, so the
/// `query` parameter is required. A struct the clients stage could not
/// resolve (no type pool, or not found in it) is not known to have one, and
/// its `query?` parameter stays optional.
pub(crate) fn query_required(f: &ApiFn, config: &Config) -> bool {
    f.filter_struct()
        .is_some_and(|q| config.required_query_structs.contains(&rust_type_to_ts(&extract_input_type(&q.ty))))
}

/// The `list` method for `f`, its collection at `/{base}`: its bare
/// filters first, then `query?` for its `*Query` struct, then `limit?` and
/// `offset?` when paginated.
///
/// A list served as a resource sends its filter as the `filter` family and
/// pages with the `page` family, reading a collection document; any other
/// list is an op (§10.4), sending the same `filter` family and paging with
/// the `opArg` family. Every paginated list returns `PaginatedResult`.
fn list_method(m: &ApiModule, f: &ApiFn, config: &Config, scope: Option<&str>) -> Method {
    let scope = scope_of(f, scope);
    let base = config.naming.url_for_module(m);
    let paginated = is_paginated(m, f, config);

    let mut params: Vec<String> = f
        .bare_filters()
        .into_iter()
        .map(|p| format!("{}: {}", snake_to_camel(&p.name), rust_type_to_ts(&strip_ref(&p.ty))))
        .collect();
    if let Some(query) = f.filter_struct() {
        let ts = rust_type_to_ts(&extract_input_type(&query.ty));
        // Bare filters are required and `limit`/`offset` follow, so a
        // required `query` never follows an optional parameter.
        let optional = if query_required(f, config) { "" } else { "?" };
        params.push(format!("query{optional}: {ts}"));
    }
    if paginated {
        params.push("limit?: number".to_string());
        params.push("offset?: number".to_string());
    }

    let ret = rust_type_to_ts(&f.return_type);
    let return_type = if paginated { paginated_result(&ret) } else { ret };

    let resource = match served(m, f, config) {
        Served::Resource(r) => r,
        Served::Op => {
            let body = op_body(m, f, config, &return_type, scope);
            return Method { params, return_type, body };
        }
    };
    let mut families: Vec<(&str, String)> = filter_family(f).map(|filter| ("filter", filter)).into_iter().collect();
    if paginated {
        families.push(("page", "{ offset, limit }".to_string()));
    }
    let path = fetch(&format!("/{base}{}", query_string(&families)), scope);
    let body = if paginated {
        vec![
            format!("const {{ data, meta }} = await httpGet<JsonApiPageDocument>({path});"),
            format!(
                "return {{ items: data.map({}), total: meta.total, limit: meta.limit, offset: meta.offset }};",
                flatten_fn(resource)
            ),
        ]
    } else {
        vec![
            format!("const {{ data }} = await httpGet<JsonApiCollectionDocument>({path});"),
            format!("return data.map({});", flatten_fn(resource)),
        ]
    };
    Method { params, return_type, body }
}
