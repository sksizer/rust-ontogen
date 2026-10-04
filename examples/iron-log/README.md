# Iron Log - Ontogen Example Project

A weight-lifting tracker demonstrating the full ontogen code generation pipeline.

## Domain Model

| Entity | Relations | Purpose |
|---|---|---|
| **Exercise** | - | Exercise catalog (name, muscle group, equipment) |
| **Workout** | many-to-many Tag | A training session (date, duration, notes) |
| **WorkoutSet** | belongs-to Workout, belongs-to Exercise | A single set (weight, reps, RPE) |
| **Tag** | - | Labels for categorizing workouts |

## Generated Stack

Running `cargo build` in `src-tauri/` triggers the full ontogen pipeline:

```
Schema (src/schema/*.rs)
  → SeaORM entities    (src/persistence/db/entities/generated/)
  → DB conversions     (src/persistence/db/conversions/generated/)
  → DTOs               (src/schema/dto/)
  → Store CRUD + hooks (src/store/generated/, src/store/hooks/)
  → API layer          (src/api/v1/generated/)
  → Axum HTTP routes   (src/api/transport/http/generated.rs)
  → Tauri IPC commands (src/api/transport/ipc/generated.rs)
  → TypeScript client  (src-nuxt/app/generated/transport.ts)
```

## Building

```bash
cd src-tauri
cargo build
```

This generates all code from the 4 schema entity files. The generated TypeScript
client uses `HttpTauriIpcSplit` - it auto-switches between Tauri IPC (desktop) and
HTTP fetch (browser) at runtime.

## Serve the HTTP API without Tauri

`src/bin/iron-log-http.rs` serves the generated Axum routes on their own,
over SQLite with a table created from each generated SeaORM entity:

```bash
cd src-tauri
cargo run --bin iron-log-http                   # http://127.0.0.1:3004, in-memory SQLite
PORT=39104 cargo run --bin iron-log-http        # any other port
IRON_LOG_DB=iron-log.sqlite cargo run --bin iron-log-http   # keep the data in a file
curl -s localhost:3004/api/workouts | jq
```

SQLite enforces the entities' foreign keys, so deleting a row that another
row still references (a tagged workout, a workout with sets, a tag in use)
currently answers `500 db_error`; [B-DLFK](../../docs/planning/backlog/B-DLFK-seaorm-delete-clears-junctions-and-refuses-referenced-rows.md) tracks the fix.

Every create, update and delete is published on the activity event routes,
`/api/events/activity-feed` (all kinds) and
`/api/events/activity-for-kind/{kind}` (`exercise`, `workout`,
`workout_set` or `tag`):

```bash
curl -sN localhost:3004/api/events/activity-for-kind/workout
```

`cargo run` on its own still starts the Tauri app.

The HTTP API speaks JSON:API ([wire contract](../../docs/jsonapi-wire-contract.md));
[the wire page](../../site/src/content/docs/examples/iron-log-wire.mdx) shows
real requests and responses captured from this server.

## Project Structure

```
iron-log/
├── src-tauri/
│   ├── build.rs              ← Pipeline wiring
│   ├── src/
│   │   ├── schema/           ← Entity definitions (ontogen input)
│   │   │   ├── exercise.rs
│   │   │   ├── workout.rs
│   │   │   ├── workout_set.rs
│   │   │   └── tag.rs
│   │   ├── persistence/      ← Generated SeaORM + hand-written helpers
│   │   ├── store/            ← Generated CRUD + lifecycle hooks
│   │   ├── api/              ← Generated API + transport layers
│   │   └── lib.rs            ← AppState + module declarations
│   └── Cargo.toml
└── src-nuxt/
    └── app/generated/        ← Generated TypeScript transport client
```

## Known Limitations

- The Tauri app does not initialize its database and no migrations are
  included. Only the headless HTTP server creates its tables, from the
  generated entities.

## See also

[`examples/iron-log-md`](../iron-log-md/) runs these same four entities on the
markdown store backend. Diffing the two generated `api/v1` trees is the
clearest demonstration of ADR 0001's invariant -- everything above the store is
byte-identical between backends:

```sh
diff -r src-tauri/src/api/v1/generated ../iron-log-md/src/api/v1/generated
```
