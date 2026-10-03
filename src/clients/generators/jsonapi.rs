//! The JSON:API half shared by both TypeScript HTTP clients (wire contract
//! §14): the request helpers, `JsonApiError`, and one flatten/unflatten pair
//! per resource, emitted from the same relationship table the server uses.
//!
//! JSON:API is applied and removed inside the generated client, so callers
//! keep the flat entity shapes the IPC transport also uses.

use ontogen_core::ir::OpKind;
use ontogen_core::naming::to_snake_case;

use crate::clients::config::Config;
use crate::clients::generators::command_name;
use crate::resource::{Arity, Resource};
use crate::servers::classify::classify_op;
use crate::servers::parse::{ApiFn, ApiModule};

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
/// request with a body declares it; the server's routes that are not yet
/// JSON:API read any `+json` body.
///
/// `httpPut` is emitted only for `with_put`: an update op in a module with no
/// entity behind it keeps its flat `PUT` route until phase 1c.
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
    out
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

/// The serde member name of a Rust field: `r#type` is written `type`.
fn member(field: &str) -> &str {
    field.strip_prefix("r#").unwrap_or(field)
}

/// One resource's relationship table, `flattenX` and `unflattenX` (§14.3).
pub(crate) fn resource_codec(resource: &Resource) -> String {
    let entity = &resource.entity.name;
    let table = format!("{}_RESOURCE", to_snake_case(entity).to_uppercase());
    let id_field = member(&resource.id_field);

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
                member(&rel.field),
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
        out.push_str(&format!("    {}: {read}(r.relationships?.['{}']),\n", member(&rel.field), rel.name));
    }
    out.push_str(&format!("  }} as {entity};\n}}\n\n"));

    out.push_str(&format!(
        "function {}(input: object, id?: string): JsonApiWriteDocument {{\n  return unflattenResource({table}, \
         input, id);\n}}\n\n",
        unflatten_fn(resource)
    ));
    out
}

fn is_crud(op: &OpKind) -> bool {
    matches!(op, OpKind::List | OpKind::GetById | OpKind::Create | OpKind::Update | OpKind::Delete)
}

/// True when `f` is emitted at all: it has a command name the config does
/// not skip.
fn is_emitted(module: &str, f: &ApiFn, config: &Config) -> bool {
    let cmd = command_name(module, f, config);
    !cmd.is_empty() && !config.ts_skip_commands.contains(&cmd)
}

/// The resource behind `module`'s CRUD methods, or `None` when the module has
/// no entity behind it and keeps its flat handlers.
pub(crate) fn crud_resource<'a>(module: &ApiModule, config: &'a Config) -> Option<&'a Resource> {
    config.resources.by_module(&module.name)
}

/// Every resource with at least one emitted CRUD method, in module order:
/// the resources that need a flatten/unflatten pair.
pub(crate) fn crud_resources<'a>(modules: &[ApiModule], config: &'a Config) -> Vec<&'a Resource> {
    modules
        .iter()
        .filter(|m| m.functions.iter().any(|f| is_crud(&classify_op(f)) && is_emitted(&m.name, f, config)))
        .filter_map(|m| crud_resource(m, config))
        .collect()
}

/// True when a module with no entity behind it emits an update, which keeps
/// its `PUT` route until phase 1c.
pub(crate) fn needs_put(modules: &[ApiModule], config: &Config) -> bool {
    modules
        .iter()
        .filter(|m| crud_resource(m, config).is_none())
        .any(|m| m.functions.iter().any(|f| matches!(classify_op(f), OpKind::Update) && is_emitted(&m.name, f, config)))
}
