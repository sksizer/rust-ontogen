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
use crate::clients::generators::{command_name, ts_params_in_declaration_order, typed_params};
use crate::resource::{Arity, JunctionRelationship, Resource, member_name};
use crate::schema::sort::{sort_fields, sort_keys};
use crate::servers::classify::classify_op;
use crate::servers::parse::{ApiFn, ApiModule, EventFn, Param};
use crate::servers::types::{
    collect_ts_import, extract_input_type, rust_type_to_ts, snake_to_camel, strip_ref, ts_param,
};

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
export class JsonApiError extends globalThis.Error {
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

async function httpRequest(method: string, path: string, body?: unknown): Promise<globalThis.Response> {
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

async function toJsonApiError(res: globalThis.Response): Promise<JsonApiError> {
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

async function httpDelete(path: string, body?: unknown): Promise<void> {
  await httpRequest('DELETE', path, body);
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
/// skipped. An array value is one parameter, its items joined by `,` as
/// `sort` takes them (§7.4), and skipped when empty; the server refuses a
/// repeated parameter, and filters take no array (§7.3).
const TO_QUERY_STRING: &str = "\
function toQueryString(params: Record<string, unknown>): string {
  const parts: string[] = [];
  const push = (key: string, value: unknown) => {
    if (value == null) return;
    const values = Array.isArray(value) ? value : [value];
    if (values.length > 0) parts.push(`${key}=${values.map((v) => encodeURIComponent(String(v))).join(',')}`);
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

/// `ListOptions` and one `{Entity}SortKey` union per resource an emitted
/// `list` sorts, in module order (§14.2), or nothing when no emitted list
/// sorts. The union enumerates the server's sort keys, ascending then
/// descending per field.
pub(crate) fn sort_types(modules: &[ApiModule], config: &Config) -> String {
    let sorted = sorted_resources(modules, config);
    if sorted.is_empty() {
        return String::new();
    }
    let mut out = "\
/** A list's sort keys, applied in order: `-` sorts a field descending, and `id` ascending is the last key unless named. */
export interface ListOptions<K extends string> {
  sort?: K[];
}

"
    .to_string();
    for r in sorted {
        // Never empty: a resource has an id, which always sorts.
        let keys = sort_keys(&sort_fields(&r.entity, &config.schema_enums));
        let union: Vec<String> = keys.iter().map(|k| format!("'{k}'")).collect();
        out.push_str(&format!("export type {} = {};\n\n", sort_key_type(r), union.join(" | ")));
    }
    out
}

/// Every resource an emitted `list` sorts, in module order.
fn sorted_resources<'a>(modules: &[ApiModule], config: &'a Config) -> Vec<&'a Resource> {
    let mut sorted: Vec<&Resource> = Vec::new();
    for m in modules {
        for f in m.functions.iter().filter(|f| is_emitted(&m.name, f, config)) {
            if let Some(r) = sort_resource(m, f, config)
                && !sorted.iter().any(|known| known.module == r.module)
            {
                sorted.push(r);
            }
        }
    }
    sorted
}

/// `TaskSortKey`.
fn sort_key_type(resource: &Resource) -> String {
    format!("{}SortKey", resource.entity.name)
}

/// The resource `f` of `m` sorts: its module's resource, when `f` is that
/// module's `list` and takes an order of the resource's entity (§7.4).
/// `None` for any other fn.
pub(crate) fn sort_resource<'a>(m: &ApiModule, f: &ApiFn, config: &'a Config) -> Option<&'a Resource> {
    let entity = f.sort_entity()?;
    let resource = config.resources.serving(m, f)?;
    (classify_op(m, f) == OpKind::List && resource.entity.name == entity).then_some(resource)
}

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

/** `T` is `JsonApiResourceIdentifier` for a relationship's linkage. */
interface JsonApiCollectionDocument<T = JsonApiResource> {
  data: T[];
}

interface JsonApiPageDocument<T = JsonApiResource> {
  data: T[];
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
  const relByField = new globalThis.Map(
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
    /// As a relationship of its module's resource (§9): a junction op of a
    /// resource module, which has no route of its own.
    Relationship(&'a Resource, JunctionRelationship<'a>),
    /// As a custom op (§10): arguments in `meta.args`, the result in
    /// `meta.result`. Custom ops, junction ops outside a resource module,
    /// and CRUD ops with no resource behind them (§10.4), a filtered `list`
    /// included.
    Op,
}

/// How `f` of `module` is served, by the predicates the server's routes
/// follow ([`ResourceModel::serving`] and [`ResourceModel::junctions`]).
///
/// [`ResourceModel::serving`]: crate::resource::ResourceModel::serving
/// [`ResourceModel::junctions`]: crate::resource::ResourceModel::junctions
pub(crate) fn served<'a>(module: &'a ApiModule, f: &ApiFn, config: &'a Config) -> Served<'a> {
    if let Some(resource) = config.resources.serving(module, f) {
        return Served::Resource(resource);
    }
    let Some(resource) = config.resources.by_module(&module.name) else { return Served::Op };
    match config.resources.junction_of(module, f) {
        Some(junction) => Served::Relationship(resource, junction),
        None => Served::Op,
    }
}

/// The resource an event op's frames carry, or `None` when they carry its
/// item as `meta.result` (§12).
pub(crate) fn event_resource<'a>(ev: &EventFn, config: &'a Config) -> Option<&'a Resource> {
    config.resources.by_item_type(&ev.item_type_ast)
}

/// Every resource that needs a flatten/unflatten pair, in module order: each
/// with at least one emitted method served as it or as one of its
/// relationships; then each target a junction list of entities reads; then,
/// with `events`, each an event op's frames carry.
pub(crate) fn served_resources<'a>(modules: &'a [ApiModule], config: &'a Config, events: bool) -> Vec<&'a Resource> {
    let emitted = |m: &'a ApiModule| {
        m.functions.iter().filter(move |f| is_emitted(&m.name, f, config)).map(move |f| (f, served(m, f, config)))
    };
    let mut resources: Vec<&Resource> = modules
        .iter()
        .filter_map(|m| {
            emitted(m).find_map(|(_, served)| match served {
                Served::Resource(r) | Served::Relationship(r, _) => Some(r),
                Served::Op => None,
            })
        })
        .collect();
    let targets = modules.iter().flat_map(emitted).filter_map(|(f, served)| match served {
        Served::Relationship(_, j) if j.lists_entities && j.list.name == f.name => {
            config.resources.by_module(&j.target_module)
        }
        _ => None,
    });
    let events = modules.iter().flat_map(|m| &m.events).filter(|_| events).filter_map(|ev| event_resource(ev, config));
    for r in targets.chain(events).collect::<Vec<_>>() {
        if !resources.iter().any(|known| known.module == r.module) {
            resources.push(r);
        }
    }
    resources
}

/// Every type a client imports from the bindings (or stubs, when they lack
/// it), sorted: each emitted method's return and parameter types, each
/// flattened resource's entity and, with `events`, each event op's item and
/// parameter types.
pub(crate) fn imported_types(modules: &[ApiModule], config: &Config, events: bool) -> Vec<String> {
    let mut types = Vec::new();
    for m in modules {
        for f in m.functions.iter().filter(|f| is_emitted(&m.name, f, config)) {
            collect_ts_import(&rust_type_to_ts(&f.return_type), &mut types);
            for p in typed_params(f) {
                collect_ts_import(&rust_type_to_ts(&extract_input_type(&p.ty)), &mut types);
            }
        }
        for ev in m.events.iter().filter(|_| events) {
            collect_ts_import(&rust_type_to_ts(&ev.item_type), &mut types);
            for p in &ev.params {
                collect_ts_import(&rust_type_to_ts(&strip_ref(&p.ty)), &mut types);
            }
        }
    }
    for r in served_resources(modules, config, events) {
        collect_ts_import(&r.entity.name, &mut types);
    }
    types.sort();
    types.dedup();
    types
}

/// Globals every JSON:API client names as they are, which an imported type
/// of the same name would shadow. The ones a schema may well name an entity
/// after (`Error`, `Event`, `EventSource`, `Map`, `MessageEvent`,
/// `Response`) are written `globalThis.…` instead and can be imported.
const CLIENT_GLOBALS: &[&str] = &["Array", "JSON", "Object", "Promise", "Record", "String"];

/// The ones the transport's event helpers add.
const TRANSPORT_GLOBALS: &[&str] = &["Math", "ReturnType"];

/// The top-level types and the class every JSON:API client declares.
const CLIENT_DECLARATIONS: &[&str] = &[
    "JsonApiCollectionDocument",
    "JsonApiError",
    "JsonApiErrorObject",
    "JsonApiPageDocument",
    "JsonApiRelationship",
    "JsonApiResource",
    "JsonApiResourceDef",
    "JsonApiResourceDocument",
    "JsonApiResourceIdentifier",
    "JsonApiWriteDocument",
    "ListOptions",
    "PaginatedResult",
];

/// The ones the transport adds, with the name it imports Tauri's `Channel`
/// under.
const TRANSPORT_DECLARATIONS: &[&str] = &["EventFrame", "IpcChannel", "SubscriptionHandlers", "Transport"];

/// Refuses a type the client (`transport`: the split transport, else the
/// HTTP client) imports from the bindings under a name the client already
/// gives a global it uses or a type it declares, which TypeScript rejects.
///
/// # Errors
///
/// The first such type, naming what the client uses its name for.
pub(crate) fn check_imported_types(modules: &[ApiModule], config: &Config, transport: bool) -> Result<(), String> {
    let sort_keys: Vec<String> = sorted_resources(modules, config).into_iter().map(sort_key_type).collect();
    for name in imported_types(modules, config, transport) {
        let use_ =
            if CLIENT_GLOBALS.contains(&name.as_str()) || (transport && TRANSPORT_GLOBALS.contains(&name.as_str())) {
                "the JavaScript global it uses"
            } else if CLIENT_DECLARATIONS.contains(&name.as_str())
                || (transport && TRANSPORT_DECLARATIONS.contains(&name.as_str()))
                || sort_keys.contains(&name)
            {
                "a name it declares"
            } else {
                continue;
            };
        let client = if transport { "transport" } else { "HTTP client" };
        return Err(format!(
            "ontogen: the TypeScript {client} cannot be generated: it imports the type `{name}` from the bindings, \
             but `{name}` is {use_}. Rename the type."
        ));
    }
    Ok(())
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
    /// The `options?: ListOptions<…>` of a list that sorts, after every
    /// other parameter, the route-prefix parameter included, so that adding
    /// it moves no positional argument.
    pub options: Option<String>,
    pub return_type: String,
    /// Its statements, in order. A statement spanning lines indents its
    /// continuation lines relative to its first.
    pub body: Vec<String>,
}

impl Method {
    /// The parameter list: its own parameters, then `prefix`, the
    /// route-prefix parameter of a client that takes one, then `options`.
    pub(crate) fn signature(&self, prefix: Option<String>) -> String {
        self.params.iter().cloned().chain(prefix).chain(self.options.clone()).collect::<Vec<_>>().join(", ")
    }
}

/// The method `f` of module `m` gets in either HTTP client, whose
/// parameters and return type the `Transport` interface declares, or `None`
/// for an event op. `scope` names the route-prefix parameter (`projectId`) of a
/// client that calls scoped routes through `scopedPath`, and is `None` for
/// one that calls only unscoped routes.
pub(crate) fn method(m: &ApiModule, f: &ApiFn, config: &Config, scope: Option<&str>) -> Option<Method> {
    let scope = scope_of(m, f, config, scope);
    let base = config.naming.url_for_module(m);
    let fetch = |p: &str| fetch(p, scope);
    let ret = if f.return_type == "()" { "null".to_string() } else { rust_type_to_ts(&f.return_type) };
    let input_type = |i: usize| rust_type_to_ts(&extract_input_type(&f.params[i].ty));
    let id_path = format!("/{base}/${{encodeURIComponent(id)}}");
    let op = classify_op(m, f);
    let resource = match served(m, f, config) {
        Served::Resource(r) => r,
        Served::Relationship(_, junction) => return Some(relationship_method(m, f, &junction, config, scope)),
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
            let body = vec![op_call(&return_type, &op_route(m, f, config), scope)];
            return Some(Method { params, options: None, return_type, body });
        }
    };

    let method = match op {
        OpKind::List => list_method(m, f, config, scope),
        OpKind::GetById => Method {
            params: vec!["id: string".to_string()],
            options: None,
            return_type: ret,
            body: vec![
                format!("const {{ data }} = await httpGet<JsonApiResourceDocument>({});", fetch(&id_path)),
                format!("return {}(data);", flatten_fn(resource)),
            ],
        },
        OpKind::Create => Method {
            params: vec![format!("input: {}", input_type(0))],
            options: None,
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
            options: None,
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
            options: None,
            return_type: "null".to_string(),
            body: vec![format!("await httpDelete({});", fetch(&id_path)), "return null;".to_string()],
        },
        _ => unreachable!("a resource serves CRUD ops only"),
    };
    Some(method)
}

/// The scope `f` of `m`'s calls take from a client's `scope`: only a
/// store-scoped op is served under the route prefix; any other keeps its
/// unscoped route. A relationship's routes are its resource's, scoped as
/// the `get_by_id` that reads its parent is.
pub(crate) fn scope_of<'a>(m: &ApiModule, f: &ApiFn, config: &Config, scope: Option<&'a str>) -> Option<&'a str> {
    let routed_by = match served(m, f, config) {
        Served::Relationship(..) => {
            config.resources.get_by_id(m).expect("a junction relationship's module serves get_by_id")
        }
        _ => f,
    };
    scope.filter(|_| routed_by.first_param_is_store)
}

/// The method of a junction op of a resource module, which calls its
/// relationship's routes (§14.2). Its list reads the related resources, or
/// the linkage when it lists ids, paged with the `page` family when
/// paginated; its add and remove send the one identifier to the linkage.
fn relationship_method(
    m: &ApiModule,
    f: &ApiFn,
    junction: &JunctionRelationship,
    config: &Config,
    scope: Option<&str>,
) -> Method {
    let mut params = ts_params_in_declaration_order(f);
    let parent = format!("/{}{}", config.naming.url_for_module(m), path_segment(&ts_param(&f.params[0].name)));
    let rel = ontogen_jsonapi::links::encode_path_segment(&junction.name);
    let linkage = format!("{parent}/relationships/{rel}");
    if f.name != junction.list.name {
        let verb = if junction.add.is_some_and(|add| add.name == f.name) { "httpPost" } else { "httpDelete" };
        let child = ts_param(&f.params[1].name);
        let body = format!("{{ data: [{{ type: '{}', id: {child} }}] }}", junction.target_type);
        return Method {
            params,
            options: None,
            return_type: "null".to_string(),
            body: vec![format!("await {verb}({}, {body});", fetch(&linkage, scope)), "return null;".to_string()],
        };
    }

    let paginated = is_paginated(m, f, config);
    let ret = rust_type_to_ts(&f.return_type);
    let (path, item, doc) = if junction.lists_entities {
        let target = config.resources.by_module(&junction.target_module).expect("a junction target is a resource");
        (format!("{parent}/{rel}"), flatten_fn(target), "")
    } else {
        (linkage, "(i) => i.id".to_string(), "<JsonApiResourceIdentifier>")
    };
    let return_type = if paginated {
        params.push("limit?: number".to_string());
        params.push("offset?: number".to_string());
        paginated_result(&ret)
    } else {
        ret
    };
    Method {
        params,
        options: None,
        return_type,
        body: read_collection(&path, Vec::new(), scope, &item, doc, paginated),
    }
}

/// The statements that read the collection at `path`, sending the query
/// parameter `families`, and return its items, each mapped by `item`: a
/// `PaginatedResult` from `data` and `meta` when `paginated`, paged with the
/// `page` family. `doc` is the documents' type argument, empty for
/// resources.
fn read_collection(
    path: &str,
    mut families: Vec<(&str, String)>,
    scope: Option<&str>,
    item: &str,
    doc: &str,
    paginated: bool,
) -> Vec<String> {
    if paginated {
        families.push(("page", "{ offset, limit }".to_string()));
    }
    let path = fetch(&format!("{path}{}", query_string(&families)), scope);
    if paginated {
        vec![
            format!("const {{ data, meta }} = await httpGet<JsonApiPageDocument{doc}>({path});"),
            format!("return {{ items: data.map({item}), total: meta.total, limit: meta.limit, offset: meta.offset }};"),
        ]
    } else {
        vec![
            format!("const {{ data }} = await httpGet<JsonApiCollectionDocument{doc}>({path});"),
            format!("return data.map({item});"),
        ]
    }
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
struct OpRoute<'a> {
    method: &'static str,
    path: String,
    filter: Option<String>,
    op_args: Vec<(&'a str, String)>,
    args: Vec<(&'a str, String)>,
}

/// The route `f` of `m` is called at, scoped or not: the route prefix
/// (§11.1) only goes in front of it.
fn op_route<'a>(m: &ApiModule, f: &'a ApiFn, config: &Config) -> OpRoute<'a> {
    let op = classify_op(m, f);
    let base = format!("/{}", config.naming.url_for_module(m));
    let value = |p: &Param| if p.is_input() { "input".to_string() } else { ts_param(&p.name) };
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
        _ if matches!(method, "POST" | "PATCH") => {
            args = rest.iter().map(|p| (member_name(&p.name), value(p))).collect()
        }
        _ => {
            for p in rest {
                if p.is_option() {
                    op_args.push((member_name(&p.name), value(p)));
                } else {
                    path.push_str(&segment(p));
                }
            }
        }
    }
    OpRoute { method, path, filter, op_args, args }
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
        f.bare_filters().into_iter().map(|p| (member_name(&p.name), ts_param(&p.name))).collect();
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
/// `offset?` when paginated, and `options?` when it sorts.
///
/// A list served as a resource sends its filter as the `filter` family,
/// its sort keys as `sort` and pages with the `page` family, in the order
/// the server's links write them (§4.3), reading a collection document;
/// any other list is an op (§10.4), sending the same `filter` family and
/// paging with the `opArg` family. Every paginated list returns
/// `PaginatedResult`.
fn list_method(m: &ApiModule, f: &ApiFn, config: &Config, scope: Option<&str>) -> Method {
    let scope = scope_of(m, f, config, scope);
    let base = config.naming.url_for_module(m);
    let paginated = is_paginated(m, f, config);

    let mut params: Vec<String> = f
        .bare_filters()
        .into_iter()
        .map(|p| format!("{}: {}", ts_param(&p.name), rust_type_to_ts(&strip_ref(&p.ty))))
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

    let Served::Resource(resource) = served(m, f, config) else {
        let body = vec![op_call(&return_type, &op_route(m, f, config), scope)];
        return Method { params, options: None, return_type, body };
    };
    let options = sort_resource(m, f, config).map(|r| format!("options?: ListOptions<{}>", sort_key_type(r)));
    let mut families: Vec<(&str, String)> = filter_family(f).map(|filter| ("filter", filter)).into_iter().collect();
    if options.is_some() {
        families.push(("sort", "options?.sort".to_string()));
    }
    let body = read_collection(&format!("/{base}"), families, scope, &flatten_fn(resource), "", paginated);
    Method { params, options, return_type, body }
}

/// Refuses a list whose TS method would take two parameters under one name,
/// which TypeScript rejects. The generator names every parameter but a bare
/// filter: `query` for the `*Query` struct, `limit` and `offset` for the
/// page, `options` for the sort keys of a list that sorts and, on the
/// `Transport` methods (`transport`), the route prefix parameter
/// camelCased. A bare filter is camelCased too, so it may land on any of
/// these or on another bare filter, and so may the prefix parameter.
///
/// # Errors
///
/// The first list whose method would collide, naming its TS method, its fn
/// and the argument or prefix parameter to rename.
pub(crate) fn check_list_params(modules: &[ApiModule], config: &Config, transport: bool) -> Result<(), String> {
    for m in modules {
        for f in m.functions.iter().filter(|f| classify_op(m, f) == OpKind::List && is_emitted(&m.name, f, config)) {
            let method = snake_to_camel(&command_name(&m.name, f, config));
            let collide = |named: &str, ts: &str, use_: &str, rename: &str| {
                Err(format!(
                    "ontogen: the TypeScript method `{method}` cannot be generated: `{}::{}` {named}, which the \
                     method takes as `{ts}`, the parameter name it uses for {use_}, so the two would collide. \
                     Rename the {rename}.",
                    m.name, f.name
                ))
            };
            // (TS name, what the method takes under it), in signature order.
            let mut taken: Vec<(String, String)> = Vec::new();
            if f.filter_struct().is_some() {
                taken.push(("query".into(), "the list's `*Query` filter struct".into()));
            }
            if is_paginated(m, f, config) {
                for page in ["limit", "offset"] {
                    taken.push((page.into(), "the page's `limit` and `offset`".into()));
                }
            }
            if list_method(m, f, config, None).options.is_some() {
                taken.push(("options".into(), "the list's sort keys".into()));
            }
            if transport && let Some(prefix) = &config.route_prefix {
                let param = &prefix.params[0].name;
                let ts = ts_param(param);
                if let Some((_, use_)) = taken.iter().find(|(name, _)| *name == ts) {
                    let named = format!("is called with the route prefix parameter `{param}`");
                    return collide(&named, &ts, use_, "route prefix parameter");
                }
                taken.push((ts, "the route prefix parameter".into()));
            }
            for p in f.bare_filters() {
                let ts = ts_param(&p.name);
                if let Some((_, use_)) = taken.iter().find(|(name, _)| *name == ts) {
                    let named = format!("takes an argument named `{}`", p.name);
                    return collide(&named, &ts, use_, "argument");
                }
                taken.push((ts, format!("the argument `{}`", p.name)));
            }
        }
    }
    Ok(())
}
