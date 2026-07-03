# Section

A section is a unit of document structure: a heading and its body, a list item, a code
block, a blockquote, or a diagram. Sections form a tree per document. They carry no
semantic meaning. All meaning lives in [entities](./entity.md),
[requirements](./requirement.md), and [relationships](./relationship.md) extracted from
section text.

Sections exist for three purposes:

- Provenance. Entity mentions and requirement sources name a section, and their `quote`
  is located in the section's `raw` by string search. See
  [shared fields](../model.md#shared-fields).
- Reconstruction. Concatenating `raw` in tree order rebuilds the document. See
  [reconstruction](../parsing.md#reconstruction).
- Navigation. "Show the documentation around this entity" resolves to its sections.

## Fields

As produced by [parsing](../parsing.md#section-tree):

- `title`: the heading or item text.
- `kind`: `root`, `heading`, `list-item`, `code-block`, `blockquote`, or `diagram`.
- `order`: position among siblings.
- `parent`: the internal reference of the parent section. The root section has none.
- `raw`: the verbatim source text.
- `hash`: a content hash of `raw`, the input to
  [section diffing](../parsing.md#section-diffing).
- `lines`: the line range in the source file, for editor integration.

Sections are stored per document under the graph store's `docs/` shard, not under
`graph/`. See [storage layout](../graph.md#storage-layout).

## References

A section reference joins the document path and the internal reference with `#`. The
internal reference is the section's path inside the document, derived from heading slugs.
E.g. `docs/cli.md#/cli/commands/compile`. See [references](../parsing.md#references).

## Coverage

Every section carries a coverage state in the store (`unprocessed`, `covered`, or
`non-normative`, with a `note` and `claimedBy`). Coverage is the completeness meter of a
build. See [coverage](../reconciler.md#coverage).
