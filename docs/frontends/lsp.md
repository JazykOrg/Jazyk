# LSP

`jazyk lsp` starts the language server over stdio, with Content-Length framed JSON-RPC.

The server is thin and read-only. It reads the [graph store](../compiler/graph.md) and maps
graph nodes to editor positions. It runs no analysis of its own and never calls the LLM.
The one write path is explicit and human-initiated: answering a
[diagnostic prompt](../compiler/model/diagnostic.md#prompts) through a
[code action](#capabilities), which applies a suggested edit deterministically or
hands the reply to an [answer session](./acp.md#answer-sessions).

## Capabilities

- Diagnostics: open [diagnostics](../compiler/model.md#node-types) are published inline,
  anchored by locating each `quote` in the open document (see
  [shared fields](../compiler/model.md#shared-fields)). Quotes survive unrelated edits, so
  anchors stay put while typing.
- Go to definition: entity → its defining mention.
- References: entity → all mentions across documents.
- Hover on an entity: the entity's definition, requirements, and relationships from the graph.
  Hover content is a rendered pack from the [context engine](../compiler/context.md), so it
  matches what the compiler and the [MCP server](./mcp.md) show. When the
  [ledger](../consumers/gen.md#the-ledger) exists, the hover appends a verification
  summary for the entity (verified over total, failing and stale counts).
- Hover inside a requirement's located quote: the requirement card, three parts, each part
  linked. The hover range is the located quote, so the whole statement highlights.
  - The requirement: the id, the EARS sentence, the derived
    [status](../consumers/gen.md#status-is-derived-never-stored), and a link to the
    requirement's heading in its entity's requirements document at
    `<out>/docsgen/<slug>.md`.
  - The code: the requirement's implementing
    [sites](../consumers/gen.md#traceability), each linking to its file at the line the
    site relocates to, marked when the site moved or was lost. A manifest file that
    carries no site links to the file itself.
  - The test: the kind and label, a link to the artifact at the line the test name sits
    on, the status with the last run time and the run command, and the evidence tail
    from the last verdict.
  - Links are absolute `file://` URIs with an `#L<line>` fragment, so any client
    navigates. The requirement link also carries `?req=<id>`, and so does the test link
    when the artifact is an `llm` test's criteria (metadata under the out directory, not
    part of the product). A client that reads the parameter (the
    [GUI](./gui.md#editor)) opens the requirement itself; a client that ignores it still
    lands on the file.
  - Without a ledger row the card is the requirement part alone, and the code and test
    parts read as not generated. Nothing is invented: the card shows what the ledger
    records, never a guess at which file implements what.
- Code actions: a published diagnostic that carries a
  [prompt](../compiler/model/diagnostic.md#prompts) offers its options as code
  actions on the anchored range, so the question sits inline in the file where the
  finding is. An `edit` option is a quick fix ("Apply: `<label>`"); an `answer`
  option is "Answer: `<label>`". Both run `jazyk.answerDiagnostic` on the server:
  - An `edit` option applies as a [dual write](./acp.md#dual-write-tools) on disk and
    resolves the diagnostic immediately; the republish removes it from the editor.
    Editors reload externally changed files as they do for any tool.
  - An `answer` option records the reply and hands it to an
    [answer session](./acp.md#answer-sessions); the diagnostic republishes as
    resolved when the handling turn lands.
  - Freeform replies need a text input, which base LSP does not define; clients with
    an input surface (the [GUI](./gui.md#questions), the VS Code extension's
    `Jazyk: answer` command) send `jazyk.answerDiagnostic` with `text` instead of an
    option index. Base clients still get every option.
- Completion: entity names and aliases, from the name index (see
  [derived data](../compiler/graph.md#derived-data)).
- Code lens: every requirement sourced in the open document shows one lens above its
  located quote: the requirement id, plus its verification status when the
  [ledger](../consumers/gen.md#the-ledger) exists. The attachment is visible without
  hovering. A lens is emitted only where the quote locates, so a broken quote never
  shows a misplaced lens. Clicking runs `jazyk.openRequirement` (declared under
  `executeCommandProvider`); the server answers with a `window/showDocument` request
  that opens the requirement's heading in its entity's requirements document at
  `<out>/docsgen/<slug>.md` (see [documentation generation](../consumers/docsgen.md)).
  Navigation is server-driven, so any LSP client gets it without client-side commands.
- Document links: every whole-word occurrence of an entity name or alias in an open
  document links to that entity's requirements document at `<out>/docsgen/<slug>.md`
  (see [documentation generation](../consumers/docsgen.md)). A reader clicks any mention
  of a concept and lands on its assembled requirements. Links are emitted only when the
  target file exists.

## Rebuilds and refresh

The server does not compile, and no editor integration starts a build; compiles run
through the [ACP bridge](./acp.md) when the owner triggers them (`jazyk compile`,
`jazyk watch`, the GUI, or `/compile` in an IDE chat). What the LSP knows about
pending work reaches the IDE's chat surface through the
[proxy](./acp.md#lsp-and-the-proxy), not through the LSP itself. Refresh is
event-driven the LSP way: file watching belongs to the client, so the editor watches the store's
`status.yaml` with its native file events and notifies the server
(`workspace/didChangeWatchedFiles`); on any notification the server compares the
generation counter, reloads, and republishes every open document (see
[concurrency](../compiler/graph.md#concurrency)). For clients without file watchers the
server keeps a slow background poll as a fallback, so a committed build always repaints
eventually.

## Build activity in the log

The server tails build activity into its log channel (stderr, which editors surface as
the extension's output panel):

- when the store lock appears or disappears, one line marks the build starting or
  ending,
- when the generation counter moves, one line per committed mutation, read from the
  [journal](../compiler/graph.md#journal): the work item, the operation, and the node
  id.

The log is a mirror of the journal, so watching the output panel during a build shows
the same additions, updates, and deletions the audit trail records.
