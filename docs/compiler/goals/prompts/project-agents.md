> [!IMPORTANT]
> This is a [Jazyk](https://jazyk.org) project. The prose documents in the
> document sets below are the source code. Jazyk compiles them into a semantic
> graph, then generates and verifies the deliverable from it. Change behavior by
> changing the documents.

## Jazyk tools

- When `jazyk` MCP tools are available, use them. Their server instructions and
  tool replies are more specific than this section. Follow them when they
  differ.
- Without them, add an MCP server named `jazyk` that runs `jazyk mcp all`
  (`jazyk setup` writes it for detected agents), or use the read-only commands
  `jazyk status`, `jazyk explain`, and `jazyk preview`.
- `jazyk compile`, `check`, `gen`, and `test` start model sessions that cost
  money. Run them only when the user asks.

## This project

{SETS}

`.jazyk/jazyk.toml` is authoritative. `list_document_sets` returns the current
sets.

## What Jazyk manages

Rules outside this section are the project's own. Jazyk rewrites this section
when it updates. Delete the section to opt out.

- **`.jazyk/`.** Do not edit anything in it by hand. Graph nodes, `status.yaml`,
  and the journal change only through Jazyk tools, which enforce identity,
  provenance, and gates. Change `jazyk.toml` with `update_project_settings`.
- **Documents.** Prefer `write_document` (send the `baseHash` from your read)
  over a raw file write. To change one requirement, use `revise_requirement`,
  `add_requirement`, `retract_requirement`, or `edit_fact`: they move the prose
  and the graph together. A plain edit is valid input too; the next compile
  reconciles it.
- **Deliverable.** Files matched by a set's `source` are the deliverable.
  Generation and verification record into Jazyk's ledger through the generation
  and verification tools.
- **Ambiguity.** When documents are unclear or contradict each other, surface it
  through the Jazyk tools or to the user. Never settle it silently in code.
- **Diagrams and docsgen** are projections of the graph. Never edit them.
- **Feedback.** If a Jazyk instruction, tool, or error is wrong or confusing,
  call `report_feedback`.
