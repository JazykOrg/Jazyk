# Parsing

Parsing turns a source document into a section tree. It is deterministic, format-specific
code. No LLM is involved. Parsing is the only stage that reads source files; everything
downstream works on sections in the [graph store](./graph.md#storage-layout),
including whole-file envelopes for opaque inputs.

## Format handlers

- A handler claims a file by its format and produces the section tree.
- Markdown (`.md`, `.markdown`) is built in.
- Plain text (`.txt`) is built in and read by the Markdown handler. Plain prose parses
  as paragraphs; a line that happens to look like Markdown reads as Markdown.
- Native builds also read [DOCX and PDF](./binary-documents.md), within the documented
  extraction profile. The browser-only engine projects Markdown and plain text.
  Other files compile as opaque inputs.
- Custom handlers per project are planned. The
  configuration format is specified in
  [project settings](./project-settings.md).
- Every selected file is a whole-file input. The inventory hashes its original
  bytes. A handler may project those bytes into sections. A file without a handler
  compiles as one opaque `blob` section at `/`, including formats that happen to be
  valid UTF-8. Its contents are never decoded as prose.
- A blob section carries a compiler-generated envelope: project-relative path,
  byte size, SHA-256, and incoming references with their surrounding document
  sections. The envelope is context, never quoted source. Requirements about the
  file quote the referring document. No fact may quote the envelope.
- `reconcile-section` compiles the whole blob. The model decides its role from the
  referring documents and existing facts. A covered mark names requirements that
  reference the input. An input with no stated obligation may be non-normative
  with a specific justification. An unknown format alone is not a warning.
- Byte changes reopen compilation. Changes to a referring section also reopen
  blob compilation. The blob follows its referring documents in ingest order.
  Unchanged inputs derive no new work. Selected input bytes participate in compile
  approvals and downstream readiness, even when no requirement references them.
- A handler failure or unreadable input yields `parse-error`. A previously stored
  input keeps its last section tree while unreadable. A new unreadable input
  contributes no sections. Neither may pass downstream readiness. The diagnostic
  resolves when the input is readable again or leaves the selection.
- Text projection hashes remain section hashes. Whole-file inventory hashes
  identify original bytes. UTF-8 prose retains its existing SipHash identity. DOCX and PDF use SHA-256 of their original package
  bytes, including formatting changes. Blob inventory revisions hash their envelope
  so referring context also invalidates compilation. `sha256` in that envelope
  identifies the exact original bytes. An empty prose document yields `empty-file`;
  an empty blob still requires a decision.

## Section tree

A heading is one to six `#` followed by a space, a tab, or the end of the line,
indented by at most three spaces (deeper indentation is code, as in CommonMark); a
`#` glued to a word is prose. A section is a heading and its body, or a fenced block inside that body: a code block,
or a diagram. Sections form a tree per document.

Content before the first heading, and a document with no headings at all, forms a
`preamble` section referenced `/`, with no title and no parent. Its whole text is its
body: it owes [coverage](./compilation.md#coverage) and derives a `reconcile-section`
goal like any heading section with a body. No prose is invisible to extraction because
of where it sits. A file of only blank lines yields no sections (that is
[`empty-file`](./compilation.md#checks) territory).

The first heading of a document, whatever its level, is the `root` section: the section
that names the document. The document's title is its title, and a bare document name
given to `read_section` reads it ([read tools](./tools.md#read-tools)). Every other
heading is a `heading`. Headings nest by level under the nearest shallower heading
before them; a heading with no shallower heading before it (the root, and any later
heading at the top level) has no parent. The preamble is never a parent of a heading.

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

List items and blockquotes stay inside their section's body; no handler produces a
section for them. The closed list of kinds names only what a handler produces today,
and a handler that splits finer adds its kinds when it lands.

Each section carries:

- `title`: the heading text, or a block's info string (`plantuml` for a bare fence
  opening a diagram; empty for a bare code fence).
- `kind`: `preamble`, `root`, `heading`, `code-block`, or `diagram`.
- `order`: position among siblings.
- `parent`: the internal reference of the parent. The preamble, the root, and a later
  top-level heading have none.
- `raw`: the verbatim source text. Concatenating the `raw` of the heading kinds
  (`preamble`, `root`, `heading`) in tree order reconstructs the document; the block
  kinds repeat text their parent already holds.
- `hash`: a content hash of `raw`, used for [diffing](#section-diffing) and
  [alignment](./alignment.md).
- `lines`: the line range in the source file, for editor integration.

## References

- A section's internal reference is its path inside the document, derived from heading
  slugs. E.g. `/cli/commands/compile`. A slug repeated under one parent takes a numeric
  suffix in document order (`/notes`, `/notes-2`), the GitHub convention, so no
  heading's prose vanishes behind a sibling's reference; a heading with no title slugs
  to `section`.
- A block's internal reference is its parent's reference followed by `/<kind>-<n>`,
  `n` counting the parent's blocks of that kind in order from 1. E.g.
  `/cli/commands/diagram-1`, `/cli/commands/code-block-2`; a block in the preamble is
  `/code-block-1`. A block reference a heading already holds (a `Diagram 1` heading
  beside the parent's first diagram) takes the next numeric suffix, as a repeated slug
  does (`/cli/commands/diagram-1-2`), so neither section hides the other.
- The full reference joins the document path and the internal reference with `#`.
  E.g. `docs/cli.md#/cli/commands/compile`.
- Links between documents in the source (relative markdown links) are recorded and feed
  the [reconciler's scheduling](./reconciler.md#scheduling) as the document link graph.
- Document links use the CommonMark parser, including reference links and optional
  titles. Images, code spans, code blocks, and raw HTML are not document links.
  Only relative targets in a format a [handler](#format-handlers) claims are checked
  (`.md`, `.markdown`, `.txt`, and DOCX and PDF where the native readers are built),
  matching the extension case-insensitively.
  External URLs, anchor-only links, assets, absolute paths, and paths escaping the
  project are ignored. Strip the query and fragment before decoding URL escapes.
  Invalid UTF-8, control characters, backslashes, and URI schemes are rejected.
- Link locations are computed from the current source text. They are not graph
  identities or stored byte offsets. Read APIs expose zero-based line numbers and
  UTF-16 character columns over the whole Markdown link occurrence.

## Section diffing

On every build, the parser's output is matched against the stored tree of every document
by [alignment](./alignment.md). Three outcomes:

- a section whose title or body changed (whitespace-insensitively) is dirty. A heading
  renamed in place (same body, same position) is this case: its reference follows its
  slug, so its anchors are carried to the new reference mechanically, and the section
  is dirty,
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
