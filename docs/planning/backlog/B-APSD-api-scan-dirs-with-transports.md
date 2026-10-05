---
type: backlog
schema_version: '2'
id: B-APSD
tags:
- api
- pipeline
- build
last_reviewed: '2026-10-05'
---

# `api_scan_dirs` does nothing in a pipeline that has transports

`Pipeline::api_scan_dirs` adds directories the API stage scans for hand-written functions. The servers and clients stages do not read the API stage's output: each scans its own `api_dir`s. Since the pre-0.9.0 fix for silently dropped collection routes, a hand-written `list` or `count` found only through `api_scan_dirs` fails the build when a servers or clients stage is configured, and every transport `api_dir` is already scanned. So with transports configured, an `api_scan_dirs` entry either fails the build or has no effect. It still matters for a pipeline without transports, or for a direct `gen_api` call, where it decides which generated CRUD functions are replaced.

## Proposal

Decide between:

- **Refuse it with transports.** `Pipeline::build` returns a `CodegenError` when `api_scan_dirs` is set alongside a servers or clients stage, naming the transports' `api_dir`s as the place for hand-written functions.
- **Remove it from `Pipeline`.** Keep `ApiConfig::scan_dirs` for direct `gen_api` callers, and have the pipeline scan exactly the transports' `api_dir`s.

Either is a breaking change for a build script that sets it today; the upgrading guide says what to delete.
