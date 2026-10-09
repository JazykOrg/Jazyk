# Binary document proof

Status: experimental. Enabled only by the native Cargo feature `document-proof`.

The proof reads DOCX and PDF bytes in the Jazyk binary without a browser or office
service. `compile`, including a build limited to zero model sessions, uses that
reader through the existing parsing and alignment pipeline. A zero-session build
proves parsing, dirtiness, and deterministic diagnostics. It does not prove model
extraction or convergence.

## Source and projection

- Original DOCX and PDF files are authoritative. No editable Markdown copy is created.
- The reader produces text, ordered source blocks, a SHA-256 byte revision, and
  format-specific locations. Projection offsets belong to that revision only.
- DOCX evaluation covers body paragraphs and table cells. Formatting preservation
  belongs to the editor's DOCX serializer. The reader does not rewrite the package.
- PDF evaluation covers text-bearing pages and their text rectangles. Scanned pages
  fail with an OCR-required message. Reading order on complex layouts is not proven.
- Unsupported content and the limited extraction profile are reported. This proof
  does not advertise full Word feature coverage or production PDF interpretation.
- Reopening saved bytes recomputes the projection. Quote matching locates diagnostics.
  An ambiguous or stale quote is unavailable rather than attached to arbitrary text.

## Evaluation surface

The `document_proof` example serves a single explicit fixture project on loopback.
It returns original bytes, native projections, and diagnostics from the existing
LSP implementation. Its save endpoint accepts DOCX bytes with the previously read
revision, validates them, and replaces the same fixture file. PDF writes are refused.
Text document writes refuse DOCX and PDF paths.

The browser evaluation lives in the private editor repository. It compares EigenPal
and Docxodus with the same input and native reader. PDF rendering has a separate
diagnostic overlay. These controls are not part of the production editor.

## IDE integration

LSP supplies diagnostics and actions. An IDE extension supplies the custom PDF or
DOCX view. Standard LSP does not render binary files and cannot apply text edits to
their byte streams. A production extension needs a read-only PDF custom editor and
a DOCX custom editor with binary save, dirty state, undo, and conflict handling.

The proof uses a read-only extracted text projection for LSP ranges. Its client maps
those ranges onto the original document. The custom editor owns DOCX writes. Jazyk
findings are view decorations and are not saved as Word comments or PDF annotations.

## Acceptance

- The native compiler parses DOCX and PDF fixture paths directly.
- A real deterministic Jazyk warning appears on the corresponding visible text.
- Both DOCX candidates save a user edit into the original DOCX path.
- With the browser closed, native reading and compilation observe that edit.
- A second save from an outdated revision fails without replacing the file.
- PDF diagnostics remain correctly placed after zooming. PDF writes fail.
- Report results separately for deterministic checks, model sessions, save-back,
  layout fidelity, and IDE integration. A browser proof does not claim a shipped
  VS Code custom editor.
