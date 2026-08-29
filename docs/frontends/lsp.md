# LSP

`jazyk lsp` starts the language server over stdio, with Content-Length framed JSON-RPC.

The server is thin and read-only. It reads the [graph store](../compiler/graph.md) and
the rendered [diagrams](../compiler/diagrams.md#output-layout) and maps graph nodes to
editor positions. It runs no analysis of its own and never calls the LLM. The one write
path is explicit and human-initiated: answering a
[diagnostic prompt](../compiler/model/diagnostic.md#prompts) through a
[code action](#capabilities), which applies a suggested edit deterministically or
hands the reply to an [answer session](./acp.md#answer-sessions).

## Capabilities

- Diagnostics: open [diagnostics](../compiler/model.md#node-kinds) are published inline,
  anchored by locating each `quote` in the open document (see
  [shared fields](../compiler/model.md#shared-fields)). Quotes survive unrelated edits, so
  anchors stay put while typing. Findings of the
  [checks](../compiler/compilation.md#checks) publish the same way when their subject
  carries a quote: a `quality-unmeasured` warning sits on the sentence that states the
  quality, an `unplaced-behavior` warning on the behavior statement.
- Go to definition: entity → its defining mention.
- References: entity → all mentions across documents.
- Hover on an entity: the entity's definition, stereotype, parent, attributes,
  requirements, and relationships, rendered as [`load`](../compiler/context.md#tools)
  renders them for that target at depth 1, so the hover matches what a
  [session](../compiler/sessions.md) and the [MCP server](./mcp.md) see
  ([rendering](../compiler/context.md#rendering)). Above the text sits the image of the
  entity's most relevant view, embedded as a markdown image whose source is the
  `file://` URI of the rendered `.svg` at `<out>/diagrams/<kind>/<slug>.svg`
  ([output layout](../compiler/diagrams.md#output-layout)); editors render markdown
  images in hovers. Beside it, the link to the entity's page at
  `<out>/docsgen/<slug>.md`, where every relevant view is embedded
  ([documentation generation](../consumers/docsgen.md)). E.g.:

  ```markdown
  ![Order (state)](file:///home/ana/shop/jazyk-out/diagrams/state/order.svg)
  [ent:order: requirements document](file:///home/ana/shop/jazyk-out/docsgen/order.md)
  ```

  The most relevant view is chosen deterministically, never by judgment:
  - a view whose slug is the entity's own, in this order: `view:state/<slug>` (the
    entity has a derived [state machine](../compiler/model/state-machine.md)),
    `view:component/<slug>` (a «system»), `view:object/<slug>` (a type with instances);
  - else the smallest view (fewest members) that lists the entity as a member, ties
    broken by the [kind catalog](../compiler/model/view.md#kinds) order, then by id;
  - no such view: the text hover alone. Nothing is invented.

  The image is the build output the last commit rendered; the hover never renders a
  view itself. When the ledger exists ([the ledger](../consumers/gen.md#the-ledger)), the
  hover appends a verification summary for the entity (verified over total, failing and
  stale counts).
- Hover inside a requirement's located quote: the requirement card, three parts, each part
  linked. The hover range is the located quote, so the whole statement highlights.
  - The requirement: the id, the `statement`, its facets, its edges and transition when
    it has them, the derived
    [status](../consumers/gen.md#status-is-derived-never-stored), and a link to the
    requirement's heading in its entity's requirements document at
    `<out>/docsgen/<slug>.md`. When a flow view lists the requirement as a member, the
    image of the smallest such view sits above the card by the same rule as for
    entities, so the step shows in its flow.
  - The code: the requirement's implementing
    [sites](../consumers/gen.md#traceability), each linking to its file at the line the
    site relocates to, marked when the site moved or was lost. A manifest file that
    carries no site links to the file itself.
  - The test: the kind and label, a link to the artifact at the line the test name sits
    on, the status with the last run time and the run command, and the evidence tail
    from the last verdict.
  - Links are absolute `file://` URIs with an `#L<line>` fragment, so any client
    navigates; image sources are `file://` URIs too. The requirement link also carries
    `?req=<id>`, and so does the test link when the artifact is an `llm` test's
    criteria (metadata under the out directory, not part of the product). A client that
    reads the parameter (the [GUI](./gui.md#editor)) opens the requirement itself; a
    client that ignores it still lands on the file.
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
    resolved when that session lands.
  - A [ratification proposal](../compiler/model/diagnostic.md#ratification-proposals)
    (`ratification-pending`) offers its `edit` option as the quick fix that inserts the
    proposed sentence: applying it is the dual write that flips the fact's provenance
    to `quote`; its "retract" answer retracts the fact. A `decision` prompt offers its
    options the same way.
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
pending goals reaches the IDE's chat surface through the
[proxy](./acp.md#lsp-and-the-proxy), not through the LSP itself. Refresh is
event-driven the LSP way: file watching belongs to the client, so the editor watches the
store's `status.yaml` with its native file events and notifies the server
(`workspace/didChangeWatchedFiles`); on any notification the server compares the
generation counter, reloads, and republishes every open document (see
[concurrency](../compiler/graph.md#concurrency)). For clients without file watchers the
server keeps a slow background poll as a fallback, so a committed build always repaints
eventually. Hover images are read from disk at hover time and a view's file path is
stable across builds, so a rebuilt diagram shows at the next hover without a reload.

## Build activity in the log

The server tails build activity into its log channel (stderr, which editors surface as
the extension's output panel):

- when the store lock is taken or released, one line marks the build starting or
  ending,
- when the generation counter moves, one line per committed mutation, read from the
  [journal](../compiler/graph.md#journal): the entry kind (a session's goal batch, or a
  store-level kind such as `edit`, `gc`, `decree`), the operation, and the node id;
  then one line per goal the entry resolved (with its justification) or opened (with
  its cause).

The log is a mirror of the journal, so watching the output panel during a build shows
the same additions, updates, deletions, and goal movements the audit trail records.
