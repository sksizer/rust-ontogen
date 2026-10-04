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
use crate::resource::{Arity, Resource, list_takes_filter, member_name};
use crate::servers::classify::classify_op;
use crate::servers::parse::{ApiFn, ApiModule, EventFn, Param, is_page_param};
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
/// request with a body declares it: the only route that is neither a
/// resource nor a custom op is a filtered list, a `GET` with no body.
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
/// parameter family: `{ page: { offset: 0 } }` gives `page%5Boffset%5D=0`.
/// An array value repeats its key.
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
    /// As its resource (§5–§8).
    Resource(&'a Resource),
    /// A `list` taking a filter: its flat route, query and success shape.
    FilteredList,
    /// As a custom op (§10): arguments in `meta.args`, the result in
    /// `meta.result`. Custom ops, junction ops, and CRUD ops with no
    /// resource behind them (§10.4).
    Op,
}

/// How `f` of `module` is served, by the predicates the server's routes
/// follow ([`ResourceModel::serving`], [`list_takes_filter`]).
///
/// [`ResourceModel::serving`]: crate::resource::ResourceModel::serving
pub(crate) fn served<'a>(module: &ApiModule, f: &ApiFn, config: &'a Config) -> Served<'a> {
    match config.resources.serving(&module.name, f) {
        Some(resource) => Served::Resource(resource),
        None if classify_op(f) == OpKind::List && list_takes_filter(f) => Served::FilteredList,
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
                Served::FilteredList | Served::Op => None,
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

/// The method `f` of module `m` gets in either HTTP client, or `None` for an
/// event op. `path` turns a request path into the expression the call
/// fetches: `(path, true)` for a template literal's text, `(path, false)` for
/// a plain string's.
pub(crate) fn method(m: &ApiModule, f: &ApiFn, config: &Config, path: &dyn Fn(&str, bool) -> String) -> Option<Method> {
    let base = config.naming.url_for_module(m);
    let fetch = |p: &str| path(p, p.contains("${"));
    let ret = if f.return_type == "()" { "null".to_string() } else { rust_type_to_ts(&f.return_type) };
    let input_type = |i: usize| rust_type_to_ts(&extract_input_type(&f.params[i].ty));
    let id_path = format!("/{base}/${{encodeURIComponent(id)}}");
    let op = classify_op(f);
    let resource = match served(m, f, config) {
        Served::Resource(r) => Some(r),
        Served::FilteredList | Served::Op => None,
    };

    let method = match (&op, resource) {
        (OpKind::EventStream, _) => return None,
        (OpKind::List, _) => list_method(m, f, config, &base, path),

        (OpKind::GetById, Some(r)) => Method {
            params: vec!["id: string".to_string()],
            return_type: ret,
            body: vec![
                format!("const {{ data }} = await httpGet<JsonApiResourceDocument>({});", fetch(&id_path)),
                format!("return {}(data);", flatten_fn(r)),
            ],
        },
        (OpKind::Create, Some(r)) => Method {
            params: vec![format!("input: {}", input_type(0))],
            return_type: ret,
            body: vec![
                format!(
                    "const {{ data }} = await httpPost<JsonApiResourceDocument>({}, {}(input));",
                    fetch(&format!("/{base}")),
                    unflatten_fn(r)
                ),
                format!("return {}(data);", flatten_fn(r)),
            ],
        },
        (OpKind::Update, Some(r)) => Method {
            params: vec!["id: string".to_string(), format!("input: {}", input_type(1))],
            return_type: ret,
            body: vec![
                format!(
                    "const {{ data }} = await httpPatch<JsonApiResourceDocument>(\n  {},\n  {}(input, id),\n);",
                    fetch(&id_path),
                    unflatten_fn(r)
                ),
                format!("return {}(data);", flatten_fn(r)),
            ],
        },
        (OpKind::Delete, Some(_)) => Method {
            params: vec!["id: string".to_string()],
            return_type: "null".to_string(),
            body: vec![format!("await httpDelete({});", fetch(&id_path)), "return null;".to_string()],
        },

        // §10.4: CRUD ops with no resource behind them.
        (OpKind::GetById, None) => Method {
            params: vec!["id: string".to_string()],
            body: vec![op_call(&ret, "GET", &fetch(&id_path), &[])],
            return_type: ret,
        },
        (OpKind::Create, None) => Method {
            params: vec![format!("input: {}", input_type(0))],
            body: vec![op_call(&ret, "POST", &fetch(&format!("/{base}")), &[(&f.params[0].name, "input")])],
            return_type: ret,
        },
        (OpKind::Update, None) => Method {
            params: vec!["id: string".to_string(), format!("input: {}", input_type(1))],
            body: vec![op_call(&ret, "PATCH", &fetch(&id_path), &[(&f.params[1].name, "input")])],
            return_type: ret,
        },
        (OpKind::Delete, None) => Method {
            params: vec!["id: string".to_string()],
            return_type: "null".to_string(),
            body: vec![op_call("null", "DELETE", &fetch(&id_path), &[])],
        },

        // §10.4: junction ops, at their nested routes.
        (OpKind::JunctionList { child_segment }, _) => {
            let paginated = is_paginated(m, f, config);
            let mut params = ts_params_in_declaration_order(f);
            let mut route = format!("/{base}/{}/{child_segment}", path_segment(&f.params[0]));
            let return_type = if paginated {
                params.push("limit?: number".to_string());
                params.push("offset?: number".to_string());
                route.push_str(&op_args_query(&[("limit", "limit"), ("offset", "offset")]));
                paginated_result(&ret)
            } else {
                ret
            };
            Method { params, body: vec![op_call(&return_type, "GET", &fetch(&route), &[])], return_type }
        }
        (OpKind::JunctionAdd { child_segment }, _) => {
            let child = &f.params[1];
            let route = format!("/{base}/{}/{child_segment}", path_segment(&f.params[0]));
            Method {
                params: ts_params_in_declaration_order(f),
                return_type: "null".to_string(),
                body: vec![op_call("null", "POST", &fetch(&route), &[(&child.name, &snake_to_camel(&child.name))])],
            }
        }
        (OpKind::JunctionRemove { child_segment }, _) => {
            let route =
                format!("/{base}/{}/{child_segment}/{}", path_segment(&f.params[0]), path_segment(&f.params[1]));
            Method {
                params: ts_params_in_declaration_order(f),
                return_type: "null".to_string(),
                body: vec![op_call("null", "DELETE", &fetch(&route), &[])],
            }
        }

        // §10.1–§10.2: a GET takes its required arguments in the path and
        // its optional ones in the `opArg` family; a POST sends them all as
        // `meta.args`.
        (OpKind::CustomGet | OpKind::CustomPost, _) => {
            let action = config.naming.derive_action(&m.name, &f.name);
            let mut route = format!("/{base}");
            if !action.is_empty() {
                route.push_str(&format!("/{action}"));
            }
            let args: Vec<(String, String)> = f
                .params
                .iter()
                .map(|p| (p.name.clone(), if p.is_input() { "input".to_string() } else { snake_to_camel(&p.name) }))
                .collect();
            let call = if op == OpKind::CustomGet {
                for p in f.params.iter().filter(|p| !p.is_option() && !p.is_input()) {
                    route.push_str(&format!("/{}", path_segment(p)));
                }
                let optional: Vec<(&str, &str)> = f
                    .params
                    .iter()
                    .zip(&args)
                    .filter(|(p, _)| p.is_option())
                    .map(|(_, (name, value))| (name.as_str(), value.as_str()))
                    .collect();
                route.push_str(&op_args_query(&optional));
                op_call(&ret, "GET", &fetch(&route), &[])
            } else {
                let args: Vec<(&str, &str)> = args.iter().map(|(n, v)| (n.as_str(), v.as_str())).collect();
                op_call(&ret, "POST", &fetch(&route), &args)
            };
            Method { params: ts_params_in_declaration_order(f), return_type: ret, body: vec![call] }
        }
    };
    Some(method)
}

/// `${encodeURIComponent(parentId)}`: `p` as one path segment.
fn path_segment(p: &Param) -> String {
    format!("${{encodeURIComponent({})}}", snake_to_camel(&p.name))
}

/// `${toQueryString({ opArg: { verbose, include_done: includeDone } })}`, or
/// nothing for no arguments. `toQueryString` drops the ones left undefined.
fn op_args_query(args: &[(&str, &str)]) -> String {
    if args.is_empty() {
        return String::new();
    }
    format!("${{toQueryString({{ opArg: {} }})}}", object_literal(args))
}

/// `return callOp<R>('METHOD', path, { args });`, without the argument
/// object when there are no arguments.
fn op_call(return_type: &str, http_method: &str, path_expr: &str, args: &[(&str, &str)]) -> String {
    let args = if args.is_empty() { String::new() } else { format!(", {}", object_literal(args)) };
    format!("return callOp<{return_type}>('{http_method}', {path_expr}{args});")
}

/// `{ a, b: c }`, using the shorthand where a key and its value match.
fn object_literal(members: &[(&str, &str)]) -> String {
    let members: Vec<String> = members
        .iter()
        .map(|(key, value)| if key == value { (*key).to_string() } else { format!("{key}: {value}") })
        .collect();
    format!("{{ {} }}", members.join(", "))
}

/// True when `f` of `m` returns a page: pagination applies only to a list
/// returning `Vec<T>`; any other result type passes through unchanged.
fn is_paginated(m: &ApiModule, f: &ApiFn, config: &Config) -> bool {
    config.pagination_for(&m.name, f.surface).is_some() && f.return_type.starts_with("Vec<")
}

/// `PaginatedResult<T>` for the TS array type `T[]`.
fn paginated_result(array: &str) -> String {
    format!("PaginatedResult<{}>", array.strip_suffix("[]").unwrap_or(array))
}

/// The `list` method for `f`, its collection at `/{base}`. The `Transport`
/// interface takes its parameters and return type from here too.
///
/// A list served as a resource pages with the `page` family and reads a
/// collection document; a filtered list sends its filter and flat
/// `limit`/`offset` as plain query parameters and returns the body as sent;
/// any other list is an op (§10.4), paging with the `opArg` family. Every
/// paginated list returns `PaginatedResult`.
pub(crate) fn list_method(
    m: &ApiModule,
    f: &ApiFn,
    config: &Config,
    base: &str,
    path: &dyn Fn(&str, bool) -> String,
) -> Method {
    let query_param = f.params.iter().find(|p| p.ty.contains("Query"));
    let paginated = is_paginated(m, f, config);
    // A list that takes the page owns its limit/offset: they are never caller params.
    let plain_params: Vec<&Param> = f
        .params
        .iter()
        .filter(|p| !p.ty.contains("Query") && !p.is_input() && (!f.takes_page() || !is_page_param(p)))
        .collect();

    let mut params: Vec<String> = plain_params
        .iter()
        .map(|p| format!("{}: {}", snake_to_camel(&p.name), rust_type_to_ts(&strip_ref(&p.ty))))
        .collect();
    if let Some(qp) = query_param {
        params.push(format!("query?: {}", rust_type_to_ts(&extract_input_type(&qp.ty))));
    }
    if paginated {
        params.push("limit?: number".to_string());
        params.push("offset?: number".to_string());
    }

    let ret = rust_type_to_ts(&f.return_type);
    let return_type = if paginated { paginated_result(&ret) } else { ret };

    let body = match served(m, f, config) {
        Served::Resource(r) if paginated => vec![
            format!(
                "const {{ data, meta }} = await httpGet<JsonApiPageDocument>({});",
                path(&format!("/{base}${{toQueryString({{ page: {{ offset, limit }} }})}}"), true)
            ),
            format!(
                "return {{ items: data.map({}), total: meta.total, limit: meta.limit, offset: meta.offset }};",
                flatten_fn(r)
            ),
        ],
        Served::Resource(r) => vec![
            format!(
                "const {{ data }} = await httpGet<JsonApiCollectionDocument>({});",
                path(&format!("/{base}"), false)
            ),
            format!("return data.map({});", flatten_fn(r)),
        ],
        Served::Op => {
            let page =
                if paginated { op_args_query(&[("limit", "limit"), ("offset", "offset")]) } else { String::new() };
            let route = format!("/{base}{page}");
            vec![op_call(&return_type, "GET", &path(&route, paginated), &[])]
        }
        Served::FilteredList => {
            let path_expr = if query_param.is_some() && plain_params.is_empty() {
                let qs_arg = if paginated {
                    "toQueryString({ ...query, limit, offset })".to_string()
                } else {
                    "toQueryString(query ?? {})".to_string()
                };
                path(&format!("/{base}${{{qs_arg}}}"), true)
            } else {
                let qs = plain_params
                    .iter()
                    .map(|p| format!("{}=${{encodeURIComponent({})}}", p.name, snake_to_camel(&p.name)))
                    .collect::<Vec<_>>()
                    .join("&");
                if paginated {
                    path(&format!("/{base}?{qs}&${{toQueryString({{ limit, offset }}).slice(1)}}"), true)
                } else {
                    path(&format!("/{base}?{qs}"), true)
                }
            };
            vec![format!("return httpGet({path_expr});")]
        }
    };
    Method { params, return_type, body }
}
