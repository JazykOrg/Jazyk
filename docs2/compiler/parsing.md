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
  raised by the [deterministic checks](./reconciler.md#waves): with no sections there is
  nothing for parsing to attach a diagnostic to.

## Section tree

A section is a heading and its body, a list item, a code block, a blockquote, or a
diagram. Sections form a tree per document.

Content before the first heading, and a document with no headings at all, forms a
`preamble` section referenced `/`, with no title and no parent. No prose is invisible
to extraction because of where it sits. A file of only blank lines yields no sections
(that is [`empty-file`](./reconciler.md#waves) territory).

Each section carries:

- `title`: the heading or item text.
- `kind`: `preamble`, `root`, `heading`, `list-item`, `code-block`, `blockquote`, or
  `diagram`.
- `order`: position among siblings.
- `parent`: the internal reference of the parent. The root section has none.
- `raw`: the verbatim source text. Concatenating `raw` in tree order reconstructs the
  document.
- `hash`: a content hash of `raw`, used for [diffing](#section-diffing).
- `lines`: the line range in the source file, for editor integration.

## References

- A section's internal reference is its path inside the document, derived from heading
  slugs. E.g. `/cli/commands/compile`.
- The full reference joins the document path and the internal reference with `#`.
  E.g. `docs/cli.md#/cli/commands/compile`.
- Links between documents in the source (relative markdown links) are recorded and feed
  the [reconciler's scheduling](./reconciler.md#scheduling) as the document link graph.

## Section diffing

On every build, the parser's output is diffed against the stored tree per document:

- a section whose `hash` is new or changed is dirty,
- a removed section is dirty and its anchored nodes become stale anchors,
- a section with the same `hash` under a new reference moved: stored references on
  entity mentions and requirement sources are rewritten mechanically, and nothing is
  marked dirty.

The diff is the sole source of the [dirty set](./reconciler.md#dirty-set).

## Reconstruction

`raw` is stored verbatim so documents can be rebuilt from the graph. Clean text formats
reconstruct byte-faithfully. Lossy formats (e.g. PDF) reconstruct approximately. This
keeps the graph a faithful mirror of the sources, not a summary of them.
