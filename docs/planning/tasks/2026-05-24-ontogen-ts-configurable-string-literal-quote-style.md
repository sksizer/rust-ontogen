---
type: task
schema_version: '3'
status: closed/done
created: '2026-05-24'
impact: low
complexity: small
tags:
- emit-style
- ontogen-ts
- pumice-follow-up
related: []
autonomy: supervised
last_reviewed: '2026-05-24'
completion_note: |
  Shipped via #79 (merge f652501, 2026-05-24). Added
  `EmitConfig::quote_style: QuoteStyle` (default `Single` preserves
  current single-quoted emission; `Double` flips to `"..."` to match
  Prettier-default consumers like Pumice). Threaded the variant
  through all string-literal emit sites via a single `quote(c, s)`
  helper. AC-3 (Pumice rebump) tracked in the companion Pumice PR.
---
# ontogen-ts: make string-literal quote style configurable via EmitConfig (currently always single-quoted)

## Goal

Expose the TypeScript string-literal quote style as a configurable knob on `EmitConfig` so consumers can match their project-wide quote convention (most TS projects ship either Prettier's default of `"..."` or eslint's `'...'`). Today ontogen-ts hardcodes single-quoted output (e.g. `export type ThemePreference = 'light' | 'dark' | 'system'`), which regressed Pumice's pre-existing double-quoted generated TS when it bumped to alpha0.0.2. The Rust wire shape is unaffected; this is purely a generated-source style choice that has downstream formatter / lint-rule consequences.

## Today

ontogen-ts's enum-literal emission renders each variant as a single-quoted TS string. Concrete evidence in the generated `bindings.ts` (committed by Pumice in sksizer/pumice#225's regenerated `src-nuxt/app/generated/types.ts`):

```ts
export type CompletionKind = 'Natural' | 'EndedEarly';
export type IntervalKind = 'Focus' | 'ShortBreak' | 'LongBreak';
export type SessionStatus = 'completed' | 'cancelled' | 'interrupted';
```

The pre-bump (specta-emitted) file was double-quoted:

```ts
export type ThemePreference = "light" | "dark" | "system";
```

| Location | Role today |
|---|---|
| `crates/ontogen-ts/src/` | The emitter that renders `syn::Variant` and `syn::Lit` into TS literals. Likely a small `render_string_literal` helper or inline `format!` call producing `'...'`. |
| `crates/ontogen-ts/src/lib.rs#EmitConfig` | Carries config knobs (`external_types`, `bigint_behavior`, `case_default`, `strict_unsupported`). No `quote_style` field today. |
| `crates/ontogen-ts/tests/` | Existing fixtures and golden-file tests pin the single-quoted output as the canonical shape. Reversing or making this configurable will touch every fixture that checks string-literal output. |

## Proposed

Add a `quote_style: QuoteStyle` field to `EmitConfig`:

```rust
pub enum QuoteStyle {
    /// `'foo' | 'bar'` — matches the current ontogen-ts default and eslint's
    /// `quotes: ['error', 'single']` style.
    Single,
    /// `"foo" | "bar"` — matches Prettier's default and TypeScript's own
    /// documentation examples; what specta emitted in alpha0.0.1.
    Double,
}
```

Wire the field through the emitter: every place that renders a quoted string literal consults `config.quote_style` and emits the matching delimiter. Default value: keep `Single` to preserve byte-identical emission for current consumers (don't break anyone implicitly); migrating consumers (like Pumice) flip the knob explicitly in their `build.rs`.

End state: Pumice (sksizer/pumice#225 follow-up) can pass `quote_style: QuoteStyle::Double` in `ClientsConfig` to restore the pre-bump style, removing one source of churn in its `types.ts` review diff. Future consumers can match their existing project style without monkey-patching the output post-emit.

## Approach

1. **Inventory quote-literal emit sites.** Grep `crates/ontogen-ts/src/` for `'{...}'` and `format!("'{}", ...)` calls that produce TS string literals. Expect 1-3 sites (enum variants, possibly string default values, possibly tag/content discriminator rendering — though shape-changing serde attrs are phase-2 / out of scope today).
2. **Add the `QuoteStyle` enum + `EmitConfig` field.** Place the enum in `crates/ontogen-ts/src/lib.rs` next to `BigIntBehavior`. Default the new field to `QuoteStyle::Single` (current behavior).
3. **Thread the field through.** Each emit site reads `config.quote_style` and dispatches on the variant. A small helper (`fn quote(c: &EmitConfig, s: &str) -> String`) keeps the dispatch in one place.
4. **Test fixtures.** Add a fixture pair: a struct emitted under `QuoteStyle::Single` and the same struct emitted under `QuoteStyle::Double`. Golden files verify byte-identical-except-quote behavior.
5. **Document the knob** in the ontogen-ts API docs and in any `EmitConfig` reference page on the site.

## Files to touch

| Location | Kind | Change |
|---|---|---|
| `crates/ontogen-ts/src/lib.rs` | modify | add `QuoteStyle` enum, `quote_style: QuoteStyle` field on `EmitConfig` (default `Single`). |
| `crates/ontogen-ts/src/` | modify | route every quoted-literal emit site through a `quote(config, s)` helper that dispatches on `quote_style`. |
| `crates/ontogen-ts/tests/` | modify | add fixtures asserting both quote styles for at least one struct with a string-literal union; existing fixtures keep the current single-quoted shape. |
| `site/src/content/docs/reference/` | modify | document `quote_style` in the ontogen-ts EmitConfig reference page. |

## Acceptance criteria

- [ ] AC-1: Unit test: a struct with a `#[serde(rename_all = "lowercase")]` enum emits as `'a' | 'b'` under `QuoteStyle::Single` and `"a" | "b"` under `QuoteStyle::Double`.
- [ ] AC-2: `cargo build` in `examples/iron-log/src-tauri/` succeeds with byte-identical generated TS — the default value `Single` preserves current behavior.
- [ ] AC-3: Pumice (sksizer/pumice#225 follow-up): setting `quote_style: QuoteStyle::Double` in `ClientsConfig` produces `types.ts` with double-quoted literals throughout, matching Pumice's pre-bump style.
- [ ] AC-4: `just full-check` passes on the rust-ontogen branch.

## Out of scope

- **Other emit-style choices** — semicolon vs comma for record separators, snake_case vs camelCase for field names (already driven by `serde(rename_all)`), tabs vs spaces for indentation, trailing-newline conventions. Each is its own ticket if a real consumer surfaces a need. This task is specifically about the quote-style knob that the Pumice bump exposed.
- **Auto-detecting project style** from a `.prettierrc` or `eslint.config.*` — fragile and surprising. Explicit `EmitConfig` field is the right contract.
- **Changing the default to `Double`** — would silently churn every current consumer's emitted output. If desirable, do it as a separate breaking-change ticket once consumers have had a chance to set the knob explicitly.

## Dependencies

- none. Pure additive feature.

## Discovery context

- Surfaced during the assessment of sksizer/pumice#225. The pre-bump (specta-emitted) `types.ts` had double-quoted enum literals; the post-bump (ontogen-ts) emission switched to single-quoted. The Pumice bump's review diff carries a chunk of pure-style noise as a result, which made the substantive changes harder to review. Making the quote style configurable lets consumers eliminate that noise on bumps.
- Companion to `[[2026-05-23-ontogen-ts-transitive-walk-long-tail-field-types]]` and `[[2026-05-23-ontogen-ts-serde-default-as-ts-optional]]` — the third ontogen-ts gap surfaced by the Pumice bump. All three should be tackled together when prioritizing the ontogen-ts emit-shape backlog.

## Post-mortem

_Captured by /sdlc:task-work on 2026-05-24. PR: pending._

### Acceptance criteria coverage

- AC-1: auto — `cargo test -p ontogen-ts` (`enum_c_style_quote_style_single_default` + `enum_c_style_quote_style_double` in `emit.rs`; `emit_quote_style_default_single_quoted` + `emit_quote_style_double_double_quoted` in `tests/end_to_end.rs`). Both delimiter arms asserted against a `#[serde(rename_all = "lowercase")]` enum.
- AC-2: auto — `cargo build` in `examples/iron-log/src-tauri/` succeeded; `git status examples/` reported no diff, confirming byte-identical TS under the unchanged `Single` default.
- AC-3: deferred-user — Pumice consumer-side verification (sksizer/pumice#225 follow-up). The knob is wired and re-exported (`ontogen_ts::QuoteStyle`); Pumice's follow-up PR will set `quote_style: QuoteStyle::Double` on its `ClientsConfig` and confirm the resulting `types.ts` matches the pre-bump shape. Not exercisable from this repo without a Pumice checkout.
- AC-4: auto — `just full-check` exit 0 on the worktree.

### What worked

- The Step-2 grep for `format!("'{...}", ...)` patterns landed both call sites in one shot — the emit surface is genuinely tiny here (two lines in `emit_enum_named`).
- The `quote()` helper kept the dispatch in one place; the call-site edits were one-liners that compile and read cleanly.
- The existing `EmitConfig::default()` test made it easy to assert the new field defaults correctly without a fresh test scaffold.

### Friction and automation gaps

- The Step-3a quality-check baseline capture (`quality_baseline.py capture ...`) hit a sandbox denial in this run despite the project's `sdlc.yaml` declaring `just full-check`. The runner had to skip the baseline and proceed without `--diff-against-baseline` at Step 7; in this case it didn't matter (no pre-existing drift), but a runner that hits actual drift on a fresh main would have to triage manually. The sandbox grant for `Bash(/Users/sksizer2/.claude/plugins/sdlc/scripts/quality_baseline.py *)` needs to be added to project `.claude/settings.local.json` (or the helper invoked via an already-permitted wrapper) so the baseline path can run without intervention. → [[2026-05-24-task-work-baseline-capture-sandbox-grant]]
- Step 5b's rebase surfaced a conflict on the task file because the start-commit added `last_reviewed:` after the verify-stamp commit had already touched the frontmatter on the task branch. The conflict was trivial (keep both fields) but cost a manual edit + `git rebase --continue`. `start_task.py`'s rebase could either (a) auto-resolve the trivial frontmatter-merge case via a YAML-aware merge driver, or (b) reorder so the verify-stamp lands on top of the start-commit, removing the conflict entirely. → upstream/sdlc (cross-repo dispatch unavailable in this run; classification-failed, see report)

### Spawned follow-up tasks

- [[2026-05-24-task-work-baseline-capture-sandbox-grant]] — sandbox permission for `quality_baseline.py`, created
- upstream-plugin (sdlc): `start_task.py` rebase trivial-frontmatter conflict — classification-failed (cross-repo PR dispatch not exercisable from this run)
