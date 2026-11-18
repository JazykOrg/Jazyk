# Editor adapters

Each adapter launches the shared `jazyk lsp` server. Chat is configured separately
through ACP. No adapter compiles documents or calls a model.

- [VS Code and Cursor](vscode/README.md).
- [Zed](zed/README.md).
- [JetBrains IDEs](jetbrains/README.md), through LSP4IJ.

Adapters require an installed Jazyk binary. The launcher downloads versioned,
checksum-verified GitHub release assets when local builds are unavailable. A
manual `Release editor adapters` workflow builds and optionally publishes them.
No marketplace listing is required. The packages contain no Jazyk runtime.
`JAZYK_EDITOR_ARTIFACT_DIR` selects a local release directory with `SHA256SUMS`
for offline setup and launcher smoke checks.

After building the native binary and both new adapters, verify setup for every
launcher on macOS or Linux:

```sh
python3 bootstrap/editors/smoke-launchers.py bootstrap/target/release/jazyk
```

The smoke check uses fake editor programs, product metadata and dependency
installers in a temporary home. It checks project configuration, plugin artifact
installation, all nine JetBrains choices and repeat setup. It does not launch
real editors or connect to a model. A real editor check must still verify LSP
startup and a diagnostic on an open Markdown document.
