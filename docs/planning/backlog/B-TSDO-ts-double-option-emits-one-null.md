---
type: backlog
schema_version: '2'
id: B-TSDO
tags:
- typescript
- generator
last_reviewed: '2026-10-04'
---

# Emit `Option<Option<T>>` as `T | null`, not `T | null | null`

The `double_option` fields of an `UpdateXInput` (`Option<Option<T>>`: absent, null, or a value) come out in the generated TypeScript as `T | null | null`, for example `notes?: string | null | null;` in `examples/iron-log-md/generated-ts/types.ts`. It is harmless, since the union collapses, but noisy. The emitter should render each `Option` layer once, giving `T | null`.

This predates E0004 phase 3a.
