# iron-log-mac — one training log, two backends, a native macOS app

> **Experimental.** `ServerGenerator::Uniffi` is a draft. Its output, and the
> Swift API this app calls, may change in any release.

A 17-entity training log, taken from a real app's schema, stored in **SQLite** or in a
**markdown vault** and served to a SwiftUI app **in-process**: no HTTP server or webview
in between. Swift calls the Rust core directly through bindings that
[UniFFI](https://mozilla.github.io/uniffi-rs/) generates from the module ontogen emits.

Building it was also a test of ontogen, and **[GAPS.md](GAPS.md)** reports where it fell
short, with timings.

```text
shared/   schema, hooks, domain, hand-written ops, the test scenario
   │ symlinked into each crate's src/
   ├── sqlite/    ──ontogen──▶ SeaORM entities + store ─┐
   └── markdown/  ──ontogen──▶ vault I/O + store ───────┤
                                                        ▼
            src/native/generated.rs   (byte-identical for both backends)
                                                        │ cargo build + uniffi-bindgen-swift
                                                        ▼
          app/IronLogFFI.xcframework + app/Sources/IronLogCore/Generated/*.swift
                                                        │ swift build
                                                        ▼
                                              IronLogMac (SwiftUI)
```

The app has one source. Which backend it uses is decided by the library it links.

## Run it

Needs Xcode (Swift 6) and the Rust toolchain.

```sh
scripts/test.sh                       # Rust scenario + Swift tests, on both backends

BACKEND=sqlite scripts/bundle-app.sh    # → target/Iron Log SQLite.app
BACKEND=markdown scripts/bundle-app.sh  # → target/Iron Log Markdown.app
open "target/Iron Log SQLite.app"
```

Add `--install` to copy the app to `/Applications`. Each app keeps its data in
`~/Library/Application Support/IronLog<Backend>`, or in `$IRON_LOG_DATA` when that's set.
The status bar shows the backend and the data path.

On the markdown build, every record is a file you can open: try
`vault/exercises/bench-press.md`. A hand edit shows on the app's next read, but the store
doesn't watch the disk, so nothing refreshes until the next write.

## The model

| Shape | Where |
|---|---|
| Self-referential tree | `Muscle.parent_id` and `Muscle.heads` |
| `many_to_many` | `Muscle.groups`, `Session.tags` |
| Junction with attributes, as an entity | `ExerciseMuscle` (role, contribution) |
| Shared primary key, by convention | `Effort` and its `LiftingFacet`, `RepsFacet`, `DistanceFacet` |
| Polymorphic pointer, by convention | `Note.subject_kind` and `subject_id` |
| Structured list and markdown body | `Exercise.cues`, `Exercise.description` |

[`shared/api/v1/training.rs`](shared/api/v1/training.rs) holds what CRUD can't express:
`log_set` (a write across three entities), `session_detail` (a join),
`volume_by_muscle` and `best_sets` (rollups), and `delete_session_cascade`. It's written
only against the generated store, so one source serves both backends. ontogen picks these
up like generated ops, so Swift gets `api.trainingVolumeByMuscle(personId:since:)`.

## Testing the two backends against each other

[`shared/tests/scenario.rs`](shared/tests/scenario.rs) runs one training history on
whichever backend includes it. Where the backends differ on purpose, a constant says so,
so each divergence is a tested fact. Each backend adds probes only it can run, such as
corrupting a column or hand-editing a file. The Swift tests repeat the core flows through
the FFI.

## What ontogen generates for Swift

| Rust | Swift |
|---|---|
| `NativeApi`, one async method per API fn, named as its IPC command | `api.sessionList(sort: ["-started_at"]) async throws -> [Session]` |
| A `#[uniffi::remote(Record)]` mirror of every type the signatures reach, found by scanning `src/` | `public struct Session` etc.; the schema types carry no UniFFI attributes |
| `UpdateSessionInput.title: Option<Option<String>>` | `String??`: `nil` leaves the field alone, `.some(nil)` clears it |
| `ApiError::Failed { code, message }`, coded as the HTTP transport codes the same `AppError` | `catch ApiError.Failed(let code, _)` with `code == "session_not_found"` |
| `entity_changes_subscribe()` → `EntityChangesSubscription` with `next() -> Option<EntityChangeFrame>` | wrapped as an `AsyncStream` in [`Feeds.swift`](app/Sources/IronLogCore/Feeds.swift) |

The hand-written side per backend is small: each `src/lib.rs` calls
`uniffi::setup_scaffolding!`, owns the tokio runtime, and exports `open_iron_log(data_dir)`
and `backend_name()`.

## The UniFFI generator: settled and not yet

Settled:

- **Remote records work on the crate's own types.** The schema stays free of UniFFI derives.
- **`Option<Option<T>>` round-trips as `T??`,** as the tests show.
- **The consumer owns the runtime.** No `async_runtime = "tokio"` and no async-compat fallback.
- **The generated module doesn't depend on the backend.** SeaORM and markdown produce the
  same file.

Not yet:

- **Cancellation doesn't propagate.** Cancelling a Swift task doesn't cancel the Rust
  future, and a `next()` in flight waits for the next change.
- **No Swift-side generation.** The `AsyncStream` adapter is hand-written per subscription.
- **Limited Swift API shape.** There is one flat object (`sessionList`, not
  `api.sessions.list`), and its name is fixed.
- **Unsupported configuration.** `route_prefix` is refused. External types (`chrono`,
  `uuid`) need a hand-written `uniffi::custom_type!`.
- **No Swift CI.** CI runs the Rust half on Linux. The Swift package needs a macOS runner.
- **No distribution signing.** Bundles are ad-hoc signed, with no notarization.
