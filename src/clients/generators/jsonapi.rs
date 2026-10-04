//! The JSON:API half shared by both TypeScript HTTP clients (wire contract
//! §14): the request helpers, `JsonApiError`, one flatten/unflatten pair per
//! resource, emitted from the same relationship table the server uses, and
//! the `list` call.
//!
//! JSON:API is applied and removed inside the generated client, so callers
//! keep the flat entity shapes the IPC transport also uses.

use ontogen_core::ir::OpKind;
use ontogen_core::naming::to_snake_case;

use crate::clients::config::Config;
use crate::clients::generators::command_name;
use crate::resource::{Arity, Resource, member_name};
use crate::servers::classify::classify_op;
use crate::servers::parse::{ApiFn, ApiModule, Param, is_page_param};
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
/// request with a body declares it: the routes that are not served as a
/// resource (custom, junction and entity-less ones, and a filtered list) read
/// any `+json` body.
///
/// `httpPut` is emitted only for `with_put`, when some update is not served
/// as a resource and so keeps its flat `PUT` route.
pub(crate) fn http_helpers(with_put: bool) -> String {
    let mut out = String::from(
        "// ── HTTP Helpers ──

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

",
    );
    if with_put {
        out.push_str(
            "async function httpPut<T>(path: string, body: unknown): Promise<T> {
  const res = await httpRequest('PUT', path, body);
  return res.json();
}

",
        );
    }
    out.push_str(
        "async function httpDelete(path: string): Promise<void> {
  await httpRequest('DELETE', path);
}

",
    );
    out.push_str(TO_QUERY_STRING);
    out
}

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

/// The resource `f` of `module` is served as, or `None` when its route keeps
/// a flat body and success shape ([`ResourceModel::serving`], the predicate
/// the server's routes follow).
///
/// [`ResourceModel::serving`]: crate::resource::ResourceModel::serving
pub(crate) fn served_resource<'a>(module: &ApiModule, f: &ApiFn, config: &'a Config) -> Option<&'a Resource> {
    config.resources.serving(&module.name, f)
}

/// Every resource with at least one emitted method served as it, in module
/// order: the resources that need a flatten/unflatten pair.
pub(crate) fn served_resources<'a>(modules: &[ApiModule], config: &'a Config) -> Vec<&'a Resource> {
    modules
        .iter()
        .filter_map(|m| {
            m.functions.iter().filter(|f| is_emitted(&m.name, f, config)).find_map(|f| served_resource(m, f, config))
        })
        .collect()
}

/// True when an emitted update is not served as a resource: its route takes
/// a flat `PUT`.
pub(crate) fn needs_put(modules: &[ApiModule], config: &Config) -> bool {
    modules.iter().any(|m| {
        m.functions.iter().any(|f| {
            classify_op(f) == OpKind::Update
                && is_emitted(&m.name, f, config)
                && served_resource(m, f, config).is_none()
        })
    })
}

/// A `list` method of either HTTP client.
pub(crate) struct ListMethod {
    /// Its parameters, ahead of any route-prefix parameter.
    pub params: Vec<String>,
    pub return_type: String,
    /// Its statements, in order.
    pub body: Vec<String>,
}

/// The `list` method for `f`, its request path under `plural`. `path` turns
/// a path into the expression the call fetches: `(path, true)` for a
/// template literal's text, `(path, false)` for a plain string's.
///
/// A list served as a resource pages with the `page` family and reads a
/// collection document; any other list sends its filter and flat
/// `limit`/`offset` as plain query parameters and returns the body as sent.
/// Either way a paginated list returns `PaginatedResult`.
pub(crate) fn list_method(
    m: &ApiModule,
    f: &ApiFn,
    config: &Config,
    plural: &str,
    path: &dyn Fn(&str, bool) -> String,
) -> ListMethod {
    let resource = served_resource(m, f, config);
    let query_param = f.params.iter().find(|p| p.ty.contains("Query"));
    // Pagination applies only to a list returning `Vec<T>`; any other result
    // type passes through unchanged.
    let paginated = config.pagination_for(&m.name, f.surface).is_some() && f.return_type.starts_with("Vec<");
    // A list that takes the page owns its limit/offset: they are never caller params.
    let plain_params: Vec<&Param> = f
        .params
        .iter()
        .filter(|p| !p.ty.contains("Query") && !p.ty.contains("Input") && (!f.takes_page() || !is_page_param(p)))
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
    let return_type =
        if paginated { format!("PaginatedResult<{}>", ret.strip_suffix("[]").unwrap_or(&ret)) } else { ret.clone() };

    let page_args = if resource.is_some() { "page: { offset, limit }" } else { "limit, offset" };
    let path_expr = if query_param.is_some() && plain_params.is_empty() {
        let qs_arg = if paginated {
            format!("toQueryString({{ ...query, {page_args} }})")
        } else {
            "toQueryString(query ?? {})".to_string()
        };
        path(&format!("/{plural}${{{qs_arg}}}"), true)
    } else if !plain_params.is_empty() {
        let qs = plain_params
            .iter()
            .map(|p| format!("{}=${{encodeURIComponent({})}}", p.name, snake_to_camel(&p.name)))
            .collect::<Vec<_>>()
            .join("&");
        if paginated {
            path(&format!("/{plural}?{qs}&${{toQueryString({{ {page_args} }}).slice(1)}}"), true)
        } else {
            path(&format!("/{plural}?{qs}"), true)
        }
    } else if paginated {
        path(&format!("/{plural}${{toQueryString({{ {page_args} }})}}"), true)
    } else {
        path(&format!("/{plural}"), false)
    };

    let body = match resource {
        Some(r) if paginated => vec![
            format!("const {{ data, meta }} = await httpGet<JsonApiPageDocument>({path_expr});"),
            format!(
                "return {{ items: data.map({}), total: meta.total, limit: meta.limit, offset: meta.offset }};",
                flatten_fn(r)
            ),
        ],
        Some(r) => vec![
            format!("const {{ data }} = await httpGet<JsonApiCollectionDocument>({path_expr});"),
            format!("return data.map({});", flatten_fn(r)),
        ],
        None => vec![format!("return httpGet({path_expr});")],
    };
    ListMethod { params, return_type, body }
}
