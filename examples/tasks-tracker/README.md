# tasks-tracker — a planning vault as an application

ADR 0001's flagship use case: a tracker whose records mirror this repo's own
`docs/planning/tasks/*.md` frontmatter (compound statuses like
`open/ready`, an epic wikilink, tags, a markdown body of sections), served
two ways from one generated pipeline.

## HTTP

The HTTP API speaks JSON:API ([wire contract](../../docs/jsonapi-wire-contract.md)).
Lists page 20 at a time. The task list is hand-written in
`src/api/v1/task.rs`: it takes a `ListTasksQuery { status, epic_id }`, so it
answers `filter[status]` and `filter[epic_id]`, and its own `count` gives the
filtered `meta.total`. It replaces the generated `list` and `count`; the rest
of the task module is generated.

```sh
cargo run
curl -s localhost:3002/api/tasks | jq
# -g keeps curl from reading the brackets as a URL glob
curl -sg 'localhost:3002/api/tasks?page[offset]=0&page[limit]=5' | jq '.meta, .links'
# the hand-written filtered list (src/api/v1/task.rs); meta.total counts the filtered set
curl -sg -H 'Accept: application/vnd.api+json' \
  'localhost:3002/api/tasks?filter[status]=closed/done&filter[epic_id]=markdown-backend' | jq
# POST without an id — the slug derives from the title:
curl -s -X POST localhost:3002/api/tasks -H 'content-type: application/vnd.api+json' \
  -d '{"data":{"type":"tasks","attributes":{"title":"Review the stack","status":"open/ready","created":"2026-06-06","body":"## Goal\n\nReview.\n"},"relationships":{"epic":{"data":{"type":"epics","id":"markdown-backend"}},"tags":{"data":[{"type":"tags","id":"codegen"}]}}}}'
cat data/vault/tasks/review-the-stack.md
```

## MCP (the generated tool registry)

```sh
cargo run -- mcp-tools                 # every entity op as an MCP tool, with JSON schemas
cargo run -- mcp-call task_list '{}'   # agent-shaped reads over the vault
cargo run -- mcp-call task_list '{"status":"closed/done","limit":5}'   # the same filter, as tool arguments
cargo run -- mcp-call task_create '{"title":"From an agent","status":"open","created":"2026-06-06","body":"…"}'
```

The registry (`generated_tool_registry()`) is transport-agnostic — name,
description, JSON schema, and an async handler per tool — so wiring it into
any MCP server runtime is a loop. The CLI here dispatches it directly to
keep the example dependency-free.

Epic membership is a derived question (walk `tasks/`, filter `epic_id`, as
`filter[epic_id]` does) — deliberately not stored on the epic. The vault stays greppable, diffable,
and Obsidian-navigable; the tracker is just one lens over it.

The vault is an OKF 0.2 bundle (every record carries a `type`); the optional
index files and `generated` stamps are left off here. See [the markdown backend
guide](../../site/src/content/docs/guides/markdown-backend.mdx).
