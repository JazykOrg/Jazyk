# Binary documents

Native Jazyk reads DOCX and text-bearing PDF documents. Original files remain the
source. No editable Markdown copy is created. Document sets select these files by
the same paths and globs as Markdown. The default native build includes the readers.

The DOCX reader extracts body paragraphs and table-cell text. The PDF reader uses
PDFium and records page rectangles. The [proof](document-proof.md) records the
tested profile. Headers, notes, numbering, tracked revisions, OCR, and complex PDF
reading order are not certified. An unreadable projection produces a parse error.
The GUI can still show the original file and explain why compilation cannot read it.

## Projection structure

The DOCX projection writes document structure as Markdown, so sections, the
outline, and link checks behave as they do for a Markdown document:

- A paragraph with a heading level becomes a Markdown heading of that level. The
  level comes from the paragraph's outline level, else from its style (`Title`,
  `Heading1` to `Heading6`, or a style named `heading N`).
- A Word hyperlink becomes `[text](target)`. An external link keeps its relationship
  target verbatim. A bookmark link becomes `[text](#bookmark)`.

E.g.:

```text
# Gateway contract

The gateway shall return a response within 3 seconds.

See the [operator notes](notes.md).
```

The markup exists only in the projection. A view that locates a quote in the
original reads it as visible text: heading markers are dropped and a link reads as
its text. The PDF projection carries no heading or link markup. PDF link
annotations are a viewer concern.

## Hashing

The compiler hashes the extracted text for dirtiness, alignment, release gates,
and document status. File saves use a SHA-256 revision of the original bytes.
Formatting changes therefore invalidate an editor's save revision without forcing
semantic reconciliation when the extracted prose is unchanged.

## Native PDF runtime

`JAZYK_PDFIUM` selects a PDFium dynamic library. Otherwise the binary checks its
adjacent `lib` directory and the system library path. A missing library produces
an actionable reader error. It does not silently discard PDF input. The native
runtime is independent of the browser's PDFium WASM module.

## Document API

The native API supports the following routes:

- `GET /api/docs/document?path=...` returns original bytes as `base64`, `format`,
  byte `revision`, `writable`, `projection`, and `diagnostics`. A failed extraction
  returns `projection: null` and `extractionError` alongside the original bytes.
  The path must name a DOCX or PDF within the open project.
- The projection carries text, ordered source blocks, source locations, and reader
  limitations. Diagnostic locations require a unique quote in the current
  revision. Unlocated diagnostics remain in the list.
- `PUT /api/docs/document?path=...` accepts `base64` and `baseHash`. Existing DOCX
  saves require the current byte revision. A mismatch returns `409`. New DOCX or
  PDF imports omit `baseHash` and create exclusively. Existing PDF writes return
  `405`. The destination must belong to a document set.
- Validate the document package before writing. Serialize writes through a project
  document lock and replace an existing file atomically. Refuse malformed data,
  oversized files, path escapes, symlinks, and nested-project writes.
- `GET /api/docs/content` exposes the extracted text for read-only consumers.
  Text write endpoints continue to refuse DOCX and PDF.

The capability descriptor exposes `binaryDocuments: true` only where these native
routes are available. The browser-only engine does not claim native format support.

## Diagnostics and agents

LSP ranges refer to the extracted text projection. A custom view relocates them
using quotes and the original byte revision. `jazyk/document` reads the same native
document response for an IDE client. It does not make LSP a binary renderer.

ACP workers and graph tools consume the sections produced by the native reader.
`jazyk document-read FILE` exposes the projection for command-line consumers.
The project MCP tool `read_document` accepts a relative `path` and returns compiler
text, source locations, and diagnostics without binary base64. ACP clients using
the project tools can read a DOCX or PDF before compiling it.
Text-based suggested edits must not rewrite binary packages. DOCX human edits go
through the format editor and binary save endpoint. PDF remains read-only.
