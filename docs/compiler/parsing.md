# Parsing

Parsing turns a source document into a section tree. It is deterministic, format-specific
code. No LLM is involved. Parsing is the only stage that reads source files; everything
downstream works on sections in the [graph store](./graph.md#storage-layout).

## Format handlers

- A handler claims a file by its format and produces the section tree.
- Markdown (`.md`, `.markdown`) is built in.
- Custom handlers per project are planned; Markdown is the only handler today. The
  configuration format is specified in
  [project settings](./project-settings.md).
- A file matched by the docs glob with no handler yields an `unsupported-format`
  diagnostic. A handler failure yields `parse-error`. An empty file yields `empty-file`,
  raised by the [deterministic checks](./compilation.md#checks): with no sections there is
  nothing for parsing to attach a diagnostic to.

## Section tree

A section is a heading and its body, or a fenced block inside that body: a code block,
or a diagram. Sections form a tree per document.

Content before the first heading, and a document with no headings at all, forms a
`preamble` section referenced `/`, with no title and no parent. No prose is invisible
to extraction because of where it sits. A file of only blank lines yields no sections
(that is [`empty-file`](./compilation.md#checks) territory).

A fenced block (three or more backticks or tildes, closed by a fence of the same
character at least as long, or running to the end of its section when unclosed) is a
section of its own, a child of the section whose body holds it, ordered before that
section's subheadings. A block whose info string names PlantUML (`plantuml`, `puml`,
`uml`) or whose first line opens one (`@start...`) is a `diagram`
([diagrams as input](./diagrams.md#diagrams-as-input)); any other fence is a
`code-block`. The block's `raw` is the block with its fences. The parent keeps its whole
body, the block included, so a quote locates in the section that states it and a change
inside a block dirties the block and its parent both. A `code-block` section derives no
reconcile goal and owes no coverage mark of its own (the parent's session reads the
block inside the body it covers); a `diagram` section does, as input of its own.

List items and blockquotes stay inside their section's body: the kinds `list-item` and
`blockquote` are reserved for a handler that produces them, and the Markdown handler
does not.

Each section carries:

- `title`: the heading text, or a block's info string (`plantuml` for a bare fence
  opening a diagram; empty for a bare code fence).
- `kind`: `preamble`, `root`, `heading`, `list-item`, `code-block`, `blockquote`, or
  `diagram`.
- `order`: position among siblings.
- `parent`: the internal reference of the parent. The root section has none.
- `raw`: the verbatim source text. Concatenating the `raw` of the heading kinds
  (`preamble`, `root`, `heading`) in tree order reconstructs the document; the block
  kinds repeat text their parent already holds.
- `hash`: a content hash of `raw`, used for [diffing](#section-diffing) and
  [alignment](./alignment.md).
- `lines`: the line range in the source file, for editor integration.

## References

- A section's internal reference is its path inside the document, derived from heading
  slugs. E.g. `/cli/commands/compile`.
- A block's internal reference is its parent's reference followed by `/<kind>-<n>`,
  `n` counting the parent's blocks of that kind in order from 1. E.g.
  `/cli/commands/diagram-1`, `/cli/commands/code-block-2`; a block in the preamble is
  `/code-block-1`.
- The full reference joins the document path and the internal reference with `#`.
  E.g. `docs/cli.md#/cli/commands/compile`.
- Links between documents in the source (relative markdown links) are recorded and feed
  the [reconciler's scheduling](./reconciler.md#scheduling) as the document link graph.

## Section diffing

On every build, the parser's output is matched against the stored tree of every document
by [alignment](./alignment.md). Three outcomes:

- a section whose title or body changed (whitespace-insensitively) is dirty,
- a section with the same title and body under a new reference (in any document)
  moved: stored references on entity mentions and requirement sources are rewritten
  mechanically, and nothing is marked dirty,
- a section that was edited, moved and edited, split, merged, or removed has its anchors
  relocated to their best candidate as a proposal, decided by the
  [`place-anchors` goal](./goals/place-anchors.md); an anchor with no candidate becomes a
  stale anchor.

The alignment result is the sole source of the [dirty set](./reconciler.md#dirty-set).

## Reconstruction

`raw` is stored verbatim so documents can be rebuilt from the graph. Clean text formats
reconstruct byte-faithfully. Lossy formats (e.g. PDF) reconstruct approximately. This
keeps the graph a faithful mirror of the sources, not a summary of them.
