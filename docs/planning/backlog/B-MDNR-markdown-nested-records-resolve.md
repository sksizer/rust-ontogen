---
type: backlog
schema_version: '2'
id: B-MDNR
tags:
- markdown
- store
- links
- parity
last_reviewed: '2026-10-05'
---

# Nested markdown records are listed but not reachable by id

The markdown store's listing walks an entity directory to any depth: `WalkOptions::max_depth` defaults to `None` (`crates/markdown-store/src/walk.rs`), so `tasks/nested/d.md` is listed as the record `d`. A lookup resolves only the top level: `VaultLayout::record_path` (`crates/markdown-store/src/layout.rs`) is `{root}/{dir}/{id}.md`, so `get_task("d")`, `update_task("d", …)` and `delete_task("d")` answer `TaskNotFound`. Over HTTP the list emits `links.self` `/api/tasks/d` for that record and the link is a `404`, which breaks the wire contract's rule that a server serves every link it emits (§8.2). Two files with the same stem in different subfolders (`tasks/a/x.md`, `tasks/b/x.md`) are listed as two records with the same id.

The test `listing_is_in_id_byte_order_even_across_nested_directories` asserts the listing side on purpose: nested folders are how Obsidian-style vaults are organised, so the fix must not be to stop listing nested files.

## Options

- **Resolve nested ids.** Keep listing to any depth and make a lookup find the record wherever its stem sits: an id → path index built from the walk (per `VaultHandle`, invalidated on write), or a walk on lookup. Duplicate stems become an error the listing reports (or a build-time/open-time check), since an id must name one record. `create` keeps writing at the top level unless the layout says otherwise.
- **Path ids.** Make the id the path below the entity directory (`nested/d`). This changes the id rule (`/` is refused today, wire contract §8.2) and every link, so it is a larger break; it removes the duplicate-stem problem.
- **Depth-limited listing by default.** Default `max_depth` to 1 and document nested folders as opt-in with one of the above. Smallest change, but a vault that relies on folders silently loses records from list, which is the failure the pre-0.9.0 stem fix avoided for unreachable names.

The first option keeps today's ids and folder vaults; it needs a decision on duplicate stems and on where `create` writes.

## What done looks like

- A listed record is reachable by its id through get, update, delete and the relationship routes, on every layout, or it is not listed.
- Duplicate stems are refused or reported, not listed twice.
- Runtime tests in `crates/markdown-store` and an `http_router` case in `crates/markdown-pilot` with a nested record.
