---
status: open
---
# OF-019 - Document the OF-014 side-car's three consumer-side gotchas

- **Severity:** Low (documentation; the three issues are workaround-able and discoverable, but each cost a consumer a debugging session)
- **Source:** Pumice integration of the OF-014 spike, 2026-05-14. All three issues surfaced once pumice tried to use `pnpm tauri dev` + CI on hosted runners; the OF-014 ticket already documents the workarounds inline ("Trying this on your project" steps 7-8 and the "Things to watch for" list), but they need to land in user-facing docs so the next consumer doesn't have to read the spike ticket.
- **Related:** [OF-014](./OF-014-redesign-ts-bindings-pipeline.md) (the spike whose side-effects this documents), [OF-015](./OF-015-productionize-typescript-generation.md) (productionization; should fold this in as one of its acceptance criteria).

## Problem

The OF-014 spike introduced three side-effects that aren't intuitive from the ontogen API surface. Each one bit a real consumer (pumice) before they were diagnosed:

1. **`tauri dev` rebuild loop on first startup.** tauri-cli 2.10.x's dev watcher (`src/interface/rust.rs::build_ignore_matcher`) honours a custom `.taurignore` filename, *not* the standard `.gitignore`. Even with `src/bin/__ontogen_ts_export.rs` correctly gitignored, the side-car's write at the end of the first build trips tauri's watcher and queues another `cargo run`. The fix is a `src-tauri/.taurignore` with the same entry. Discoverability is the issue, not the fix.

2. **`cargo run` becomes ambiguous after the side-car lands.** Cargo sees `src/main.rs` (the app) and `src/bin/__ontogen_ts_export.rs` (the side-car) as two `[[bin]]` targets and fails any `cargo run` invocation without `--bin` with "could not determine which binary to run." `pnpm tauri dev` does exactly this; the symptom is the dev server starting and then immediately failing. Fix: add `default-run = "<app-bin-name>"` to the consumer's `[package]`. This is structural — any consumer with a single-binary crate that depends on ontogen will hit it the moment the side-car appears.

3. **CI disk pressure.** The side-car's separate `CARGO_TARGET_DIR` (deliberate, dodges the cargo target-dir lock from rust-lang/cargo#8938) doubles the target footprint. On default GitHub Actions runners (~14 GB free), this exhausts disk during the second tree's compile — pumice's CI failed with `No space left on device` from `rustls-webpki` on first attempt. The consumer's working pattern: env-gate the `.servers()` call in `build.rs`, trust the committed generated files, and add a manual-dispatch "drift" CI job that re-runs the full pipeline on a runner with freed disk.

All three are documented inline in [OF-014's "Trying this on your project"](./OF-014-redesign-ts-bindings-pipeline.md#trying-this-on-your-project) (steps 7-8 + the "Things to watch for" bullets). That section explicitly says "Lives here, not in user-facing docs, so don't link external consumers to it." This ticket is the lift to user-facing docs.

## Location

- `site/` (mdx pages under `guides/` or `cookbook/`) — primary target. The OF-014 recipe is currently spike-grade prose; user-facing docs need a stable shape.
- `examples/iron-log/` — should grow a `.taurignore`, a `default-run` in `Cargo.toml`, and (probably) a documented `IRON_LOG_SKIP_SERVER_CODEGEN` env-gate so the example demonstrates the full pattern an adopter copies from.
- `README.md` — at minimum, the prerequisites table or the "What ontogen does" section should mention that consumers using the side-car need to set `default-run` and (for tauri consumers) `.taurignore`.

## Proposed shape (sketch, not commitment)

A short "Integration gotchas" section in the user-facing TS pipeline guide:

- **`default-run`** — required when ontogen's side-car is in play. Show the one-line Cargo.toml addition.
- **`.taurignore`** — required for Tauri consumers. Show the contents.
- **CI disk pressure / opt-out env gate** — show the build.rs idiom and the corresponding CI workflow stanza. Probably as a "Productionizing" subsection rather than a default pattern, since not every consumer hits the disk wall.

OF-015's punch-list already covers the *upstream* productionization (idempotent side-car writes would actually eliminate gotcha 1 without `.taurignore`; emitting the side-car outside `src/bin/` would eliminate gotcha 2; an explicit `disable_codegen` ontogen config knob would replace gotcha 3's ad-hoc env-gate). This ticket is scoped to **documenting the current spike-grade workarounds**; the underlying issues live in OF-015 (gotchas 1+2) and warrant being broken out separately if they don't land in the OF-015 cleanup.

## Open questions

- Should gotcha 1 (`.taurignore`) be a runtime concern at all? Idempotent writes in `write_sidecar_source` would close it for tauri AND for any other watcher (notify, vscode, etc.). Worth folding into OF-015 as an AC rather than just documenting the workaround?
- Gotcha 2 (`default-run`) is structural to "side-car lives in `src/bin/`." If the side-car moved to `target/` or a workspace-style internal crate, the consumer wouldn't need `default-run`. Is that in OF-015's scope?
- Should the side-car emit a `.taurignore` snippet automatically if it detects `tauri.conf.json`? Feels too clever; a one-line doc entry is probably enough.

## Effort

- Documentation pass: ~1 hour. Mostly mechanical lift from OF-014's recipe section into a stable page.
- Example update: ~30 min. Add the three files / lines to `examples/iron-log/`.
- Optional upstream fix (idempotent side-car write): ~30 min including a test. Closes gotcha 1 without the doc lift, but the docs still want to mention `default-run` + CI disk.

## Notes

Pumice carries the workarounds in commit `e866993` (`fix(ontogen): suppress tauri dev-watcher rebuilds on the specta side-car`) and the surrounding history. See its [`docs/architecture/03-modules/ontogen-pipeline.md`](https://github.com/sksizer/pumice/blob/main/docs/architecture/03-modules/ontogen-pipeline.md) for the consumer-side write-up — could be the seed for the upstream user-facing page.
