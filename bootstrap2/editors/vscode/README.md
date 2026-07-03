# Jazyk for VS Code

Language support for [Jazyk](https://jazyk.org), natural language compiled into a semantic
graph. The extension is a thin client: it launches `jazyk lsp` and relays LSP traffic, so
everything shown (diagnostics, go to definition, find references, hover, completion) comes
from the graph store.

The server is read-only. It never compiles. Diagnostics refresh when `jazyk compile` or
`jazyk watch` runs beside the editor: each build bumps the store's generation counter, and
the server reloads the graph and republishes when it moves. The project root is found by
walking up to a `jazyk.toml`.

## Requirements

A built `jazyk` binary. With no setting, the extension looks for
`bootstrap2/target/release/jazyk`, then `bootstrap2/target/debug/jazyk` inside the
workspace, then falls back to `jazyk` on `PATH`. Set `jazyk.server.path` to override.

```sh
cd ../..        # the bootstrap2 crate
cargo build --release
```

## Build & run the extension

```sh
npm install
npm run compile
```

Then press <kbd>F5</kbd> in VS Code to launch an Extension Development Host, and open a
folder containing a `jazyk.toml`.

## Settings

- `jazyk.server.path`: path to the `jazyk` binary. Empty means workspace build, then `PATH`.
