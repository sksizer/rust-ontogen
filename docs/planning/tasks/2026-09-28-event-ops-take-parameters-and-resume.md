---
type: task
schema_version: '3'
status: closed/done
created: '2026-09-28'
last_reviewed: '2026-09-28'
impact: high
complexity: large
tags:
- servers
- events
- sse
- ipc
related:
- 2026-09-28-generated-subscriptions-resume-and-report-lag.md
completion_note: |
  Shipped in [#184](https://github.com/sksizer/rust-ontogen/pull/184). Event fns take params
  (sync or async, `Receiver<T>` or `Result<Receiver<T>, E>`); SSE writes `id:` from
  `ontogen_core::events::EventSeq` and feeds `Last-Event-ID` to `resume`; lag is an explicit
  frame; IPC has `<fn>_subscribe` / `<fn>_unsubscribe` over a per-subscriber `Channel`.
  Runtime support is in ontogen-core's `events` feature. Deviation: `start_event_forwarding`
  and the TS `onX` stay for the parameterless sync shape so existing consumers keep building.
relevance_note: Driven by the dev monorepo's push-channel milestone (determined decision D-0050). The consumer bumps its pin once this and its client-side sibling release.
---
# Event ops take parameters, carry a sequence, and report lag

## Goal

An api fn that returns `broadcast::Receiver<T>` becomes an event op. Today that
op can only be a global firehose: it takes no parameters, its IPC side emits to
every window, and a slow subscriber loses events without knowing. The dev
monorepo wants to push per-vault file changes and per-job status to its
frontend. That needs a subscription that names what it wants, one delivery
channel per subscriber, a sequence a client can resume from, and an explicit
signal when events were dropped.

[ADR 0003](../../architecture/0003-api-design-over-backwards-compatibility.md)
allows changing the event op shape before 1.0. Consumers on an older pin keep
the old shape until they bump.

## Today

- `src/servers/parse.rs` — `EventFn` records only `name` and `surface`. An
  event fn is detected by its return type (`Receiver<T>`); any parameter after
  state is ignored.
- `crates/ontogen-core/src/ir.rs` — `OpKind::EventStream` carries no params.
- `src/api/mod.rs` — `convert_scanned_event` builds an `ApiFnMeta` with no
  params and `return_type: "EventStream"`.
- `src/servers/generators/http.rs` — `<fn>_sse` calls `svc::<fn>(&state)`,
  wraps the receiver in `BroadcastStream`, and drops lag errors with
  `filter_map(|r| r.ok())`. Events carry `event:` and `data:` but no `id:`.
  The scoped variant calls `state.subscribe_<fn>_for(&project)`.
- `src/servers/generators/ipc.rs` — `start_event_forwarding` spawns one task
  per event fn that calls `handle.emit(name, &delta)`: a global broadcast to
  every webview, started once at app setup, never per subscriber.
- `src/servers/generators/mcp.rs` — skips event ops.

## Proposed

An event fn may take parameters after state, like any other op:

```rust
pub fn vault_note_changes(state: &AppState, vault_id: String, kinds: Option<Vec<ChangeKind>>)
    -> broadcast::Receiver<Sequenced<VaultChange>>;
```

- **Parameters.** `EventFn` and the IR carry the params. HTTP takes them as
  path/query params, the same rules a `CustomGet` uses. IPC takes them as
  invoke args.
- **Sequence.** When the item type implements a small ontogen trait (for
  example `ontogen::EventSeq { fn event_id(&self) -> String }`), the SSE
  handler writes it as the event `id:`. The HTTP handler reads
  `Last-Event-ID` and passes it to the event fn as a `resume` param when the
  fn declares one (a param named `resume: Option<String>`, or an attribute).
  Ontogen does not replay history itself; the fn decides what resume means.
- **Lag.** A `RecvError::Lagged(n)` becomes an explicit frame: SSE
  `event: lag` with `data: {"skipped": n}`, IPC a `Lag { skipped }` message
  on the channel. The stream stays open. A closed sender ends the stream.
- **IPC per subscriber.** Each event op generates two commands:
  `<fn>_subscribe(args…, channel: tauri::ipc::Channel<EventFrame<T>>) -> SubscriptionId`
  and `<fn>_unsubscribe(id)`. The subscribe command calls the event fn,
  spawns a task that forwards into that channel, and records the task under
  the id. The task ends on unsubscribe, on the sender closing, or on the first
  failed `channel.send` (a closed webview). A `Channel` has no close
  callback, so send failure plus explicit unsubscribe are the two cleanup
  paths, and both are tested.
- `start_event_forwarding` and the global `emit` path are removed. A consumer
  that wants a global broadcast subscribes once.
- **Wire frame.** One enum on both transports:
  `EventFrame<T> = Event { id: Option<String>, data: T } | Lag { skipped: u64 }`.
  SSE maps it onto `id:`/`event:`/`data:`; IPC sends it as is.
- MCP still skips event ops, with a comment saying why.

## Approach

1. Extend `EventFn`, `OpKind::EventStream` and `convert_scanned_event` to keep
   params. Reuse the `ApiFn` param parsing rather than writing a second one.
2. Add `EventFrame<T>` and the `EventSeq` trait to the runtime crate the
   generated code already depends on.
3. Rewrite the SSE handler: params in, `id:` from `EventSeq`, `Last-Event-ID`
   into `resume`, lag frame instead of `filter_map(ok)`. Do the scoped variant
   the same way.
4. Replace `start_event_forwarding` with the subscribe/unsubscribe command
   pair and a subscription registry in generated code (a `Mutex<HashMap<u64,
   AbortHandle>>` behind a `OnceLock`, or state the consumer provides; pick
   one and say why in the generated doc comment).
5. Update `examples/` that declare event fns and regenerate their committed
   output.
6. Tests: a parameterized event fn generates params on both transports; a
   lagged receiver yields one lag frame and keeps streaming; a dropped IPC
   channel ends its task on the next send; unsubscribe ends it at once;
   `Last-Event-ID` reaches `resume`.

## Files to touch

- `src/servers/parse.rs`, `crates/ontogen-core/src/ir.rs`, `src/api/mod.rs`
- `src/servers/generators/http.rs`, `src/servers/generators/ipc.rs`,
  `src/servers/generators/mcp.rs`
- the runtime crate that holds shared generated-code types
- `examples/` event declarations and their generated output

## Acceptance criteria

- [ ] An event fn with parameters generates an SSE route and an IPC
      subscribe command that both take those parameters.
- [ ] SSE events carry `id:` when the item implements `EventSeq`, and a
      reconnect's `Last-Event-ID` reaches the fn's `resume` param.
- [ ] A lagged subscriber receives a `lag` frame with the skipped count; no
      generated code path drops a lag error silently.
- [ ] Each IPC subscriber gets its own `tauri::ipc::Channel`; no generated
      code calls `AppHandle::emit` for an event op.
- [ ] The forwarding task ends on unsubscribe and on the first failed
      channel send, proven by tests.
- [ ] `just full-check` passes and the examples' committed output is current.

## Out of scope

- The generated TypeScript side. That is
  [its sibling task](./2026-09-28-generated-subscriptions-resume-and-report-lag.md).
- Replaying missed events. The consumer's event fn owns history; ontogen only
  carries the resume token and reports lag.
- MCP event support.

## Dependencies

- none
