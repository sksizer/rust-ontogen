# Iron Log — Ontogen Example Project

A weight-lifting tracker demonstrating the full ontogen code generation pipeline.

## Domain Model

| Entity | Relations | Purpose |
|---|---|---|
| **Exercise** | — | Exercise catalog (name, muscle group, equipment) |
| **Workout** | many-to-many Tag | A training session (date, duration, notes) |
| **WorkoutSet** | belongs-to Workout, belongs-to Exercise | A single set (weight, reps, RPE) |
| **Tag** | — | Labels for categorizing workouts |

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
client uses `HttpTauriIpcSplit` — it auto-switches between Tauri IPC (desktop) and
HTTP fetch (browser) at runtime.

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

- Client generation (TypeScript transport, HTTP client, admin registry) is
  performed inline by `servers::generate_transport()`, which dispatches both
  server and client variants through `ServersConfig::client_generators`.
- The `strip_wikilink` stubs in `persistence/fs_markdown/` are no-ops required
  by generated store code for `belongs_to` / `many_to_many` fields. Projects
  without markdown persistence still need these.
- No database initialization or migrations are included. SeaORM 2 will handle
  schema creation from entity definitions.
