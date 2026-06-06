# tasks-tracker — a planning vault as an application

ADR 0001's flagship use case: a tracker whose records mirror this repo's own
`docs/planning/tasks/*.md` frontmatter (compound statuses like
`open/ready`, an epic wikilink, tags, a markdown body of sections), served
two ways from one generated pipeline.

## HTTP

```sh
cargo run
curl -s localhost:3002/api/tasks | jq
# POST without an id — the slug derives from the title:
curl -s -X POST localhost:3002/api/tasks -H 'content-type: application/json' \
  -d '{"title":"Review the stack","status":"open/ready","created":"2026-06-06","epic_id":"markdown-backend","tags":["codegen"],"body":"## Goal\n\nReview.\n"}'
cat data/vault/tasks/review-the-stack.md
```

## MCP (the generated tool registry)

```sh
cargo run -- mcp-tools                 # every entity op as an MCP tool, with JSON schemas
cargo run -- mcp-call task_list '{}'   # agent-shaped reads over the vault
cargo run -- mcp-call task_create '{"title":"From an agent","status":"open","created":"2026-06-06","body":"…"}'
```

The registry (`generated_tool_registry()`) is transport-agnostic — name,
description, JSON schema, and an async handler per tool — so wiring it into
any MCP server runtime is a loop. The CLI here dispatches it directly to
keep the example dependency-free.

Epic membership is a derived question (walk `tasks/`, filter `epic_id`) —
deliberately not stored on the epic. The vault stays greppable, diffable,
and Obsidian-navigable; the tracker is just one lens over it.
