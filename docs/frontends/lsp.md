# LSP

[Binary documents](../compiler/binary-documents.md#diagnostics-and-agents) use
read-only text projections for ranges. `jazyk/document` accepts `uri` and returns
original bytes, projection, and located diagnostics for a custom editor.

A project opens all [document sets](../compiler/document-sets.md). Graph operations address one set
explicitly. Each set owns `.jazyk/<set-id>/`; project file operations share the
physical files and report every membership. The document-set contract defines
configuration, aggregate reads, management commands, and grouped compilation.

`jazyk lsp` starts the language server over stdio, with Content-Length framed JSON-RPC.

The server is thin. It reads the [graph store](../compiler/graph.md) and
the rendered [diagrams](../compiler/diagrams.md#output-layout) and maps graph nodes to
editor positions. It shares the deterministic document-link check with the compiler.
Hovers, diagnostics, and every other request read only and never call the LLM. Its
writes are explicit, human-initiated [commands](#commands): answering a
[diagnostic prompt](../compiler/model/diagnostic.md#prompts), editing a document set,
and starting a compile.

Native document links use standard file URLs. Drive letters, Unicode, spaces and
URI delimiters round-trip through the URL library. A non-file URI does not name
a local document. Windows verbatim path prefixes and drive-letter URLs identify
the same project files.

## Editor setup

The launcher configures the language server separately from the ACP chat agent.
An ACP registration does not connect the language server.

- VS Code and Cursor use the extension in `bootstrap/editors/vscode/`.
- Zed uses the [Zed extension](../../bootstrap/editors/zed/README.md). It attaches to Markdown
  without replacing Zed's grammar. `jazyk zed` enables `jazyk` in the project's
  `.zed/settings.json` and records the executable under `lsp.jazyk.binary`.
  Other Markdown servers and settings remain intact. The extension is opt-in.
- The JetBrains choices use one [JetBrains plugin](../../bootstrap/editors/jetbrains/README.md), with
  the free LSP4IJ client. This includes IntelliJ IDEA, PhpStorm, WebStorm,
  PyCharm, GoLand, RustRover, CLion, Rider, and RubyMine. The plugin attaches to
  Markdown only in a Jazyk project. Each launcher records the executable in
  `.jazyk/ide.json`. ACP settings remain separate.

Source checkouts build the editor adapters using their README instructions.
The launcher installs available local artifacts. A missing adapter or dependency
produces an explicit setup instruction, never a claim that LSP is connected.
JetBrains plugin installation requires an IDE restart when it is already running.
The LSP4IJ installer uses temporary IDE configuration and cache directories, so
installing the client does not take the open IDE's profile lock.
An installed adapter is setup evidence. A successful LSP initialize exchange and
document diagnostics are connection evidence.

Editor adapters launch the existing native server. They never compile documents or
call a model on their own; only the explicit `jazyk.compile` command starts a build. Diagnostics, hover, definitions, references, completion and quick
fixes use standard LSP. Client support determines diagram images, code lenses,
and server-driven navigation. Freeform diagnostic answers remain available through
the CLI and ACP chat.

## Capabilities

- Diagnostics: open [diagnostics](../compiler/model.md#node-kinds) are published inline,
  anchored by locating each `quote` in the open document (see
  [shared fields](../compiler/model.md#shared-fields)). Quotes survive unrelated edits, so
  anchors stay put while typing. Findings of the
  [checks](../compiler/compilation.md#checks) publish the same way when their subject
  carries a quote: a `quality-unmeasured` warning sits on the sentence that states the
  quality, an `unplaced-behavior` warning on the behavior statement.
- Missing document links are checked against the current editor text and project
  files, even before compilation. The shared `broken-link` check highlights each
  link with UTF-16 ranges and names the missing target. Saved targets and file
  watcher notifications refresh open documents without requiring a graph change.
  A stored suppression for that source-target pair remains respected.
- Diagnostic ranges use the shared [location resolver](../compiler/model/diagnostic.md#locations).
  Every located quote in the document is published once per finding and range.
- A finding about a whole section (a section subject, e.g. `uncovered-section`, or
  a parked `reconcile-section` goal) is published on the section's heading text,
  never across its body. A section without a heading (the text before a
  document's first heading, a document with none) anchors on its first sentence: the
  text up to the first `.`, `!`, or `?` that ends a word in its first paragraph, or
  that paragraph's first line when no sentence ends in it. A `diagram` section anchors on its opening fence line. The
  anchor is located in the current text, so it follows edits above it. E.g.:

  ```markdown
  ## Payments                       ← uncovered-section lands here
  A payment is captured when its order ships.
  ```
  Prompt code actions use those same ranges, including related context.
  Related document locations identify the affected context without inventing a quote.
- Go to definition: entity → its defining mention.
- References: entity → all mentions across documents.
- Hover on an entity: the [card](../consumers/docsgen.md#entity-cards) in short,
  read from the same model docsgen renders. In order: the name with its stereotype and
  the `definition`; `Sits in`, the breadcrumb as text; the image of the view the
  entity is used in (the structural level view of the parent's level, the scope root's
  for a parentless entity), embedded as a markdown image whose source is the `file://`
  URI of the rendered `.svg` at `<out>/diagrams/<kind>/<slug>.svg`
  ([output layout](../compiler/diagrams.md#output-layout)), captioned with the view id;
  under it, when the entity has a level of its own, the link to its inside view's
  [diagram page](../consumers/docsgen.md#diagram-pages) with the child count, never a
  second image; then one line of links: the card, the requirements document with its
  requirement count, the level page of the parent's level (present when the parent has
  a level, the same condition as the image). A derived or decreed entity's last line
  says so and names the pending proposal. Blocks are separated by blank lines, so every
  markdown renderer keeps them apart. Editors render markdown images in hovers. E.g.:

  ```markdown
  **Funds** «component» · Funds gathers the balances and methods a customer spends at Checkout.

  Sits in: Public › Checkout › **Funds**

  ![view:class/checkout](file:///home/ana/shop/.jazyk/ds_main/diagrams/class/checkout.svg)

  Inside: [view:class/funds](file:///home/ana/shop/.jazyk/ds_main/docsgen/diagrams/class/funds.md) (3 children)

  [card](file:///.../docsgen/entities/funds.md) · [requirements (0)](file:///.../docsgen/funds.md) · [level: Checkout](file:///.../docsgen/levels/checkout.md)
  ```

  The image is the build output the last commit rendered; the hover never renders a
  view itself. When the `.svg` is missing (the renderer failed on its `.puml`), the
  caption stands alone, linked to the view's diagram page and marked `not rendered`;
  nothing is invented. When the ledger holds a row for one of the entity's requirements
  ([the ledger](../consumers/gen.md#the-ledger)), the hover appends a verification
  summary over those rows (verified over total, failing and stale counts); a project
  that never generated shows no such line. The links are plain `file://` URIs so any client navigates; the VS
  Code extension opens the walk's pages (cards, diagram pages, level pages) in the
  markdown preview beside the document, and leaves the requirements document link as a
  file link so it lands on the heading's line.
- Go to type definition on an entity: the entity's card at
  `<out>/docsgen/entities/<slug>.md`, at line 0. Definition stays the defining mention
  in the prose; the type definition is the walk. From the card the reader clicks
  through levels and diagrams in the editor's markdown preview, every link relative
  under the out directory.
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

## Commands

The server declares these commands under `executeCommandProvider`. Each runs only
when a client sends `workspace/executeCommand`, never as a side effect of another
request.

- `jazyk.openRequirement`: open a requirement's heading in its requirements document
  (see the code lens under [capabilities](#capabilities)).
- `jazyk.answerDiagnostic`: answer a [diagnostic prompt](../compiler/model/diagnostic.md#prompts)
  with an option index or `text` (see the code actions under [capabilities](#capabilities)).
- `jazyk.documentSets`: the project's [document sets](../compiler/document-sets.md) and
  their summaries, the reply of `jazyk sets list`.
- `jazyk.previewDocumentSet`: preview one set's configuration edit (resolved paths,
  overlaps, affected sets) without writing.
- `jazyk.saveDocumentSet`: apply that edit to `.jazyk/jazyk.toml`, with the same
  expected-revision check as the GUI.
- `jazyk.compile`: start a [build](../compiler/compilation.md#a-build) of one set,
  named by `setId`; a project with exactly one set may omit it. In a project with
  several sets a call without `setId` is refused with a message to choose a document
  set, and nothing starts. The reply names the started set and returns at once:
  - Inside the [GUI server](./gui.md) (the editor's `/lsp` WebSocket), the build is a
    GUI [job](./gui.md#jobs), queued, cancellable, and visible like any other.
  - Over stdio, the server runs it under the build lease with the LLM settings
    `jazyk lsp` resolved from its flags, the environment, and the configuration. A
    second call while it runs is refused. On `exit` the server cancels it and waits
    for it to stop at a boundary, so no build is cut off mid-commit.

## Rebuilds and refresh

The server compiles only when a person runs the [`jazyk.compile`](#commands) command;
otherwise builds run when the owner triggers them elsewhere (`jazyk compile`,
`jazyk watch`, the GUI, or `/compile` in an IDE chat). What the LSP knows about
pending goals reaches the IDE's chat surface through the
[proxy](./acp.md#lsp-and-the-proxy), not through the LSP itself. Refresh is
event-driven the LSP way: file watching belongs to the client, so the editor watches the
store's `status.yaml` and `build.yaml` with its native file events and notifies the server
(`workspace/didChangeWatchedFiles`); on any notification the server compares the
generation counter, reloads, and republishes every open document (see
[concurrency](../compiler/graph.md#concurrency)). The comparison is cheap: one line
of each set's `build.yaml` plus the configuration hash, read on every message. The
graph reloads only when one of them moved, so typing costs no reload. For clients
without file watchers the server keeps a slow background poll as a fallback (the
same comparison every 2 seconds), so a committed build always repaints eventually. Hover images are read from disk at hover time and a view's file path is
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
