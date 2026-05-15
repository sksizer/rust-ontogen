---
status: in-progress
last_reviewed: 2026-05-14
---
# OF-023 - Move workspace members under a `crates/` subdirectory

- **Severity:** Low. Pure repo-hygiene cleanup; no behavioural impact. Affects build-script paths, workspace `Cargo.toml`, downstream consumer `Cargo.toml` path-deps (iron-log, Pumice).
- **Status:** Open. Filed 2026-05-14 to capture the layout decision made during the OF-015 design pass — `ontogen-ts` will land directly under `crates/ontogen-ts/`, and the existing siblings `ontogen-core/` + `ontogen-macros/` should follow for consistency.
- **Related:** [OF-015](./OF-015-productionize-typescript-generation.md) introduces `crates/ontogen-ts/` as the canonical location for the new sibling crate; OF-023 backfills the same convention to the existing crates.

## Problem

The repo currently has workspace members scattered at the root:

```
ontogen-core/
ontogen-macros/
src/                  (the root `ontogen` crate)
Cargo.toml            (workspace = [".", "ontogen-core", "ontogen-macros"])
```

The OF-015 design pass added `crates/ontogen-ts/` as the new sibling's path. Keeping the existing two crates at the root while the new one lives under `crates/` is an inconsistency that compounds with every future workspace addition. At four or more crates, `crates/` is the conventional Rust-ecosystem layout (matches `tokio`, `axum`, `serde`, `cargo`, `rustc`, etc.).

This ticket is the lift to bring the existing crates in line.

## Direction

Move `ontogen-core/` → `crates/ontogen-core/` and `ontogen-macros/` → `crates/ontogen-macros/`. The root `Cargo.toml` workspace, the root `ontogen` crate's path-deps, the example crate (`examples/iron-log/src-tauri/Cargo.toml`), and any docs that reference the absolute paths are updated to match.

## Location

Files that need touching:

- **`Cargo.toml`** (workspace root): `members` array updated to `[".", "crates/ontogen-core", "crates/ontogen-macros", "crates/ontogen-ts"]`. Path-dep entries for `ontogen-core` / `ontogen-macros` in `[dependencies]` updated to `path = "crates/ontogen-core"` / `path = "crates/ontogen-macros"`.
- **`examples/iron-log/src-tauri/Cargo.toml`**: `ontogen` build-dep at `path = "../../../"` is unaffected (root path doesn't move). `ontogen-macros` regular-dep at `path = "../../../ontogen-macros"` becomes `path = "../../../crates/ontogen-macros"`.
- **`docs/`**, **`site/`**, **`README.md`**: search for any references to the old paths (`ontogen-core/`, `ontogen-macros/`) and update.
- **`build.rs`** files (if any reference workspace paths): update.
- **`.gitignore`**: if it has path-specific entries for the moved crates, update.

## Scope

In:

1. **Move** `ontogen-core/` and `ontogen-macros/` to `crates/`. Use `git mv` so history is preserved cleanly.
2. **Update** the root `Cargo.toml` workspace `members` and `[dependencies]` path entries.
3. **Update** `examples/iron-log/src-tauri/Cargo.toml` path-deps.
4. **Search and update** docs/site references.
5. **Verify** with `just full-check` + a clean build of iron-log.

Out:

- **Moving the root `ontogen` crate's `src/` under `crates/ontogen/src/`.** That's a much bigger lift (changes the workspace from "root-crate + members" to "pure workspace"), affects the package shape on crates.io, and isn't motivated by the OF-015 design pass. Separate ticket if desired.
- **Renaming any of the crates.** Pure relocation, no name changes.

## Effort

Small. Probably 1-2 hours total — `git mv`, two `Cargo.toml` edits, a `grep -r "ontogen-core\|ontogen-macros" docs site README.md` to catch reference rot, `just full-check`, `cargo build` in iron-log, commit. The risk is incidental — easy to forget one path reference and have CI fail; the grep pass + `cargo check` catches both.

## Acceptance criteria

- [ ] AC-1: `ontogen-core/` is relocated to `crates/ontogen-core/` and `ontogen-macros/` is relocated to `crates/ontogen-macros/`, performed with `git mv` so history is preserved (`git log --follow crates/ontogen-core/Cargo.toml` and `git log --follow crates/ontogen-macros/Cargo.toml` both reach pre-move history).
- [ ] AC-2: Root `Cargo.toml` workspace `members` becomes `[".", "crates/ontogen-core", "crates/ontogen-macros"]`, and its `[dependencies]` entries for `ontogen-core` / `ontogen-macros` point at `path = "crates/ontogen-core"` / `path = "crates/ontogen-macros"` (versions and other keys unchanged).
- [ ] AC-3: `examples/iron-log/src-tauri/Cargo.toml`'s `ontogen-macros` path-dep updates from `../../../ontogen-macros` to `../../../crates/ontogen-macros`. The `ontogen` build-dep at `../../../` is left alone (root path is unchanged).
- [ ] AC-4: A repo-wide search for the legacy paths (`grep -rIn --exclude-dir=target --exclude-dir=node_modules -e 'ontogen-core/' -e 'ontogen-macros/' .`) returns zero stale references in tracked files outside `docs/tasks/` (historical task entries may keep their pre-move wording).
- [ ] AC-5: `just full-check` passes cleanly in the worktree after the move.
- [ ] AC-6: `cargo build` succeeds in `examples/iron-log/src-tauri/` against the relocated path-deps (build script + crate compile end-to-end).
- [ ] AC-7: Workspace root `ontogen` crate (`src/` at the repo root) is **not** moved — its package shape on crates.io is preserved per the "Out of scope" note.

## Open questions

- **Coordinate with Pumice?** Pumice has the same `ontogen-macros` path-dep style (if it path-deps at all rather than git-deps). Worth a heads-up to the maintainer so the migration doesn't surprise them. If Pumice git-deps to ontogen at a specific rev, this is invisible.
- **Single PR vs. split?** The mechanical move is one atomic change; splitting into per-crate moves would create awkward intermediate states. Single commit / single PR is cleaner.

## Notes

- This ticket assumes the standard Rust ecosystem convention. There's no universal mandate; cargo / tokio / axum / serde all use `crates/`, but plenty of two-or-three-crate workspaces leave members at root. The argument for moving is forward-looking: at the rate of new workspace members the OF-015 design pass is creating (`ontogen-ts` now, possibly more as OF-020/021/022 land), `crates/` will earn its keep within months.
- The root `ontogen` crate stays at `src/` (not under `crates/ontogen/`) for the reasons in scope. If a future ticket wants to normalize everything under `crates/`, that's a separate lift involving the package metadata and crates.io publication path.
