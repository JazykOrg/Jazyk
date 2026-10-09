# Document proof fixtures

These files were authored for the Jazyk proof. The generators are
`web/proofs/documents/fixtures.mjs` and `web/proofs/documents/rich-fixtures.mjs`
in the private editor repository.

- `contract.docx`: heading, requirement, literal Markdown link text, Unicode, and a 2×2 table.
- `contract.pdf`: three text lines in a single PDF page.
- `no-text.pdf`: a blank page, used to verify the no-text refusal. It is not an OCR fixture.
- `structured.docx`: Word heading styles, a table, and real hyperlinks to a Markdown
  document, a PDF, a missing document, and a web page.
- `linked.pdf`: two pages with link annotations to a Markdown document, the second
  page, a DOCX, and a web page.

Run from `bootstrap`:

```sh
cargo test --lib documents::tests
cargo test --lib lsp::tests
cargo test --lib webapi::docs::tests
```

To include the native PDF test, set `JAZYK_PDFIUM` to a compatible PDFium
dynamic library and add `-- --include-ignored` to the first command.
