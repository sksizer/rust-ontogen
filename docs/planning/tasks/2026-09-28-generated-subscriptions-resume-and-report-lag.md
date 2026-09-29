---
type: task
schema_version: '9'
state: closed/done
created: 2026-09-28
last_reviewed: 2026-09-28
related:
- 2026-09-28-event-ops-take-parameters-and-resume.md
tags:
- clients
- events
- typescript
impact: high
complexity: medium
relevance_note: Driven by the dev monorepo's push-channel milestone (determined decision D-0050). Ships in the same release as its server-side sibling.
completion_note: |
  Shipped in [#185](https://github.com/sksizer/rust-ontogen/pull/185). Typed
  `subscribeX(args, handlers)` on the Transport interface and both transports, with
  onEvent/onLag/onOpen/onError. HTTP reconnects with capped exponential backoff and jitter,
  resuming from the last seen id; IPC uses a per-subscription Channel and unsubscribes by id.
  A vitest drives a generated fixture transport. Legacy `onX` stays for parameterless sync ops.
---
# Generated TS subscriptions resume and report lag

## Goal

The generated TypeScript `onX(callback)` is untyped, takes no parameters,
reconnects on a fixed 3 s timer, and cannot tell its caller that events were
missed. Once event ops take parameters and carry a sequence (the
[server-side sibling](./2026-09-28-event-ops-take-parameters-and-resume.md)),
the client must expose them, so a consumer can catch up after a gap instead of
polling.

## Today

- `src/clients/generators/transport.rs` `generate_http_transport` (~684) —
  emits `onX(callback: (payload: unknown) => void)` over `EventSource`.
  `onerror` closes and retries after 3000 ms. The payload is `unknown`.
- `generate_ipc_transport` (~964) — emits `onX` over `listen(name)`, the
  global Tauri event bus.
- `generate_transport_interface` — skips `OpKind::EventStream`, so the
  `Transport` interface has no event methods; only the concrete transports do.

## Proposed

Each event op yields one method on the `Transport` interface and both
transports:

```ts
subscribeVaultNoteChanges(
  args: { vaultId: string; kinds?: ChangeKind[] | null },
  handlers: {
    onEvent: (data: VaultChange, id: string | null) => void
    onLag?: (skipped: number) => void
    onOpen?: () => void        // first connect and every reconnect
    onError?: (err: unknown) => void
  },
): Promise<() => void>        // unsubscribe
```

- Payloads are typed from the event item type, like any other return type.
- HTTP: `EventSource` sends `Last-Event-ID` on its own reconnect. When the
  generated code reconnects itself, it passes the last seen id as the
  `resume` query param. Retry uses capped exponential backoff with jitter.
- IPC: creates a `Channel`, calls `<fn>_subscribe`, routes `Event`/`Lag`
  frames to the handlers, and the returned function calls `<fn>_unsubscribe`.
- `onOpen` fires on every (re)connect. That is the hook a consumer uses to run
  its own catch-up read. The generated client does no domain catch-up.

## Approach

1. Emit the interface method in `generate_transport_interface`.
2. Rewrite the HTTP emission: typed args, `lag` listener, backoff, `onOpen`.
3. Rewrite the IPC emission over `Channel` and the subscribe/unsubscribe pair.
4. Regenerate the examples; add a vitest for the HTTP client against a fake
   `EventSource` (reconnect sends the last id; `lag` reaches `onLag`).

## Files to touch

- `src/clients/generators/transport.rs`
- `examples/` generated TS output
- the admin-layer vitest suite, or a new test beside it

## Acceptance criteria

- [ ] The `Transport` interface declares a typed `subscribeX(args, handlers)`
      for every event op.
- [ ] The HTTP client resumes from the last seen id and backs off with a cap.
- [ ] The IPC client uses a per-subscription `Channel` and unsubscribes on the
      returned function.
- [ ] A `lag` frame reaches `onLag` on both transports; `onOpen` fires on
      every reconnect.
- [ ] `just full-check` passes.

## Out of scope

- Domain catch-up (for example calling a `get_changes` read on `onOpen`).
  That belongs to the consumer.

## Dependencies

- [Event ops take parameters, carry a sequence, and report lag](./2026-09-28-event-ops-take-parameters-and-resume.md)
