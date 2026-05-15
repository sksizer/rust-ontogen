# Architecture decision records (ADRs)

Numbered, accepted decisions that shape ontogen's structure. None written
yet; this directory accumulates as decisions earn their keep.

## When to file an ADR

Anything that meets all three:

- A non-obvious architectural choice (multiple plausible options were considered).
- Long-lived consequences — the decision shapes future work, not just the
  PR that captures it.
- Worth explaining to a future reader who wasn't in the room when the
  call was made.

A "non-obvious" call doesn't need to be controversial. The OF-014 → OF-015
pivot to ontogen-ts is exactly the kind of decision an ADR captures: a
side-car-vs-AST-emitter tradeoff with multiple realistic options and
consequences for the next year of work. It hasn't been written up as an ADR
yet because the design narrative lives inline in
[OF-015](../planning/tasks/OF-015-productionize-typescript-generation.md);
promoting it to an ADR is a reasonable follow-up.

## Conventions

Files are numbered sequentially with zero-padding (`0001-foo.md`,
`0002-bar.md`). Once a number is assigned, it doesn't change — supersession
gets its own new-numbered ADR that references the old one.

Shape: each ADR is a single Markdown file with sections:

- **Status** — one of `proposed`, `accepted`, `superseded`, `deprecated`.
- **Context** — what forced the decision; what was unknown or contested.
- **Decision** — the call that was made.
- **Consequences** — positive, negative, and follow-on work the decision
  implies.
- **Alternatives considered** — what was rejected and why.
- **Notes** — anything else worth recording (often: pointers to the
  surrounding planning docs, dates of revisits, etc.).

See [adr.github.io](https://adr.github.io/) for the broader convention; ours
is a light adaptation.

## Relationship to planning docs

ADRs cover decisions about the *codebase shape*. Task and epic docs in
[`../planning/`](../planning/) cover *units of work* (what to build next,
who owns it, when it shipped). These overlap occasionally — a single piece
of work may include an architectural decision worth promoting to an ADR —
but they answer different questions:

- "Why is the code structured this way?" → ADR
- "What's the next thing to ship?" → planning/tasks + planning/epics
- "Where is the project going at the milestone level?" → [`../roadmap.md`](../roadmap.md)
