# Markdown Parser

The Markdown parser is the built-in [documentation parser](../docs-parser.md#documentation-parser)
for Markdown files. It splits a Markdown document into a tree of sections and subsections and
extracts cross-references. It supports any files with the `.md` and `.markdown` extensions.

## Sectioning

Section boundaries are split by:
- ATX headings (`#`, `##`, `###`, …) and/or Setext (`===`, `---`) where the heading level determines 
  nesting depth.
- Ordered/unordered Lists (`-`, `+`, `*`, `1.`)
- Embedded content including:
  - blockquotes (`>`)
  - code blocks (three backticks; optional format)
- Footnotes (`[^1]`)

Most notable absense is the horizontal rule (`---`, `***`, `___`) that does not split sections, it
is simply included as text within the containing section.

Each heading starts a new section and its content runs until the next heading of the same or higher
level. A heading of a deeper level becomes a reference to a new section.

There may be an implicit file-level root section in two cases:
- If content is present before the first heading in thefile, it becomes an implicit top-level section.
- If there is no content, but there are multiple same-level headings, an empty implicit section is added.
Otherwise, the top-level heading becomes the root section of the file.

### Location paths

A section's [location](../../model/section.md#section) fragment is the slugified path of
headings from the document root to the section. List items within a section are also addressed by index.
If there are multiple list items, the index continues incrementally.

E.g.:
```markdown
# A <-- doc.md#/a
## B <-- doc.md#/a/b
C
- D <-- doc.md#/a/b/0
- E <-- doc.md#/a/b/1
F
1. G <-- doc.md#/a/b/2
2. H <-- doc.md#/a/b/3
```

## Cross-references

The parser extracts raw cross-references and passes them to the linker as relocations to resolve. The
parser does not type them. Typed [relationships](../../model/relationship.md#relationship) between
entities are produced later, from requirements.

Cross-references come from:
- Markdown links to a section in this or another file (`[Write](./file.md#/file/write)`)
- Footnotes (`[^1]`)
- Nested subsections (`## B` inside `# A`)
- Nested list items (`- B` inside `# A`)

## Embedded content

Quoted text, fenced code blocks and embedded diagrams (e.g. Mermaid) are treated as a section with
text. They will be contained underneath its parent section, but the content within will be text
without parsing for further relationships.

## Diagrams

A diagram (e.g. mermaid) may be very helpful in explaining architectural structure of several
components (sections). Although the entire diagram is a section, it may end up having a large
number of relationships to various other sections or files.

The diagram tells a picture of how components are related and will not be split up into multiple
sections. Rather it is linked to from other sections to attach it as a reference and is likely
a great reference for understanding relationship types between other sections.
