# iron-log-md — iron-log's schema on the markdown store backend

The twin of [`examples/iron-log`](../iron-log): the **same four entities**,
generated through the **same pipeline**, with one line of difference that
matters — the store backend. Workouts live as editable markdown files under
`data/vault/` instead of SQLite rows.

```sh
cargo run
# in another shell:
curl -s localhost:3001/api/workouts | jq
curl -s -X POST localhost:3001/api/workouts -H 'content-type: application/json' \
  -d '{"id":"w-1","date":"2026-06-06","tags":["strength"],"created_at":"2026-06-06T08:00:00Z"}'
cat data/vault/workouts/w-1.md      # wikilinked tags, plain YAML
$EDITOR data/vault/workouts/w-1.md  # add a field, edit prose…
curl -s -X PUT localhost:3001/api/workouts/w-1 -H 'content-type: application/json' \
  -d '{"duration_minutes":60}'
cat data/vault/workouts/w-1.md      # …your edits survived the generated update
```

## The byte-identical demo

ADR 0001's load-bearing invariant: everything above the store is identical
between backends. See it directly —

```sh
diff -r ../iron-log/src-tauri/src/api/v1/generated src/api/v1/generated
```

(zero output). The mechanical, CI-enforced proof lives in the root
workspace: `tests/backend_parity.rs`.

## What differs from iron-log (the entire consumer delta)

| | iron-log (SeaORM) | iron-log-md (markdown) |
|---|---|---|
| `Store` holds | `db: Arc<DatabaseConnection>` | `vault: markdown_store::VaultHandle` |
| generated CRUD calls | `self.db()` | `self.vault()` |
| junction plumbing | `sync_junction`/`load_junction_ids` | none — m2m is a wikilink list in frontmatter |
| `AppError` | `DbError(String)` | `Md(String)` + `From<markdown_store::Error>` |
| storage | SQLite file | `data/vault/<entity>/<id>.md`, editable anywhere |

Declared omission: the TauriIpc transport generator (compiling the Tauri
stack for a headless demo buys nothing; iron-log exercises it). The schema
files add explicit `directory = "…"` attributes — the markdown backend reads
them; SeaORM never did.

Known follow-up surfaced by this example: the HTTP generator emits axum-0.7
route syntax (`:param`); axum 0.8 — which iron-log itself pins — rejects it
at router-build time, so iron-log's HTTP server panics on boot on main.
This twin pins axum 0.7 until the generator emits `{param}`.
