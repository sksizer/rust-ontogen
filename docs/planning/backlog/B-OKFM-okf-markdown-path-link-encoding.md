---
type: backlog
schema_version: '2'
id: B-OKFM
tags:
- markdown
- okf
last_reviewed: '2026-10-03'
---

# LinkEncoding::MarkdownPath: render relations as bundle-relative markdown links

Add an opt-in `LinkEncoding::MarkdownPath` that renders relation values as bundle-relative `/<dir>/<id>.md` markdown links, so OKF graph readers (which build the graph from body markdown links) see relations. Relations stay `[[id]]` wikilinks by default.

Deferred from epic E0005 (decision 3, amended in decision 4; see the gap table in `docs/planning/epics/okf-markdown-vault.md`). The T-9NJO pilot decides whether the SDLC corpus flips it on.
