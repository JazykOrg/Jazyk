# Documentation Parser

A documentation parser reads a specific format of documentation and converts it into a common
structured format. Parsers isolate format specific concerns so the rest of the
[compiler](../compiler.md#compiler) operates on one uniform representation.

A parser also abstracts the capabilities of the underlying format, such as cross-reference extraction
and diagram support.

## Parsed format

Every parser produces the same output: a tree of [sections](../model/section.md#section). The tree
mirrors the document's own nesting (e.g. a heading and its subheadings).

The parser does not extract entities or requirements. That happens in later compilation steps.
Cross-references found in the text (e.g. a markdown link to another file) are kept as raw references
for the linker to resolve.

## Parser template

Each parser reads its specific file and converts it into the common format. A parser implements:

1. Support detection: given a file path, report whether this parser handles it (by extension and/or
   by content inspection).
2. Parse: given a supported file, produce the tree of sections.
3. Render (optional): given sections, re-emit the file in this format. The inverse of parse.

Verbatim [reconstruction](../artifacts/reproducibility.md#reproducibility) concatenates each section's
stored `raw` source and needs no parser cooperation, so `render` is optional. A parser provides it
only to support the regenerated path (re-emitting normalized content) and
[documentation generation](../../docsgen.md#documentation-generation) writing proposals back to file.
Lossy formats may render back approximately.

Parsers are either built in to the compiler or supplied by the project as
[custom handlers](../project-settings.md#handlers).

## Built-in formats

- [Markdown](./docs-parser/markdown.md#markdown-parser)
