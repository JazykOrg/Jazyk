# Editor adapters

Each adapter launches the shared `jazyk lsp` server. Chat is configured separately
through ACP. No adapter compiles documents or calls a model.

- [VS Code and Cursor](vscode/README.md).
- [Zed](zed/README.md).
- [JetBrains IDEs](jetbrains/README.md), through LSP4IJ.

Build the relevant artifacts before running the launcher from a source checkout.
Missing artifacts produce installation instructions while the IDE still opens.
The artifacts remain local and are not committed.

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
