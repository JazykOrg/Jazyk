# Document proof fixtures

These files were authored for the Jazyk proof. The generator is
`web/proofs/documents/fixtures.mjs` in the private editor repository.

- `contract.docx`: heading, requirement, literal Markdown link text, Unicode, and a 2×2 table.
- `contract.pdf`: three text lines in a single PDF page.
- `no-text.pdf`: a blank page, used to verify the no-text refusal. It is not an OCR fixture.

Run from `bootstrap`:

```sh
cargo test --features document-proof --lib documents::tests
cargo test --features document-proof --lib lsp::tests
cargo test --features document-proof --lib webapi::docs::tests
```

To include the native PDF test, set `JAZYK_PROOF_PDFIUM` to a compatible PDFium
dynamic library and add `-- --include-ignored` to the first command.
