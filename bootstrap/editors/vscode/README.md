# Jazyk for VS Code

Language support for [Jazyk](https://jazyk.org), natural language compiled into a semantic
graph. The extension is a thin client: it launches `jazyk lsp` and relays LSP traffic, so
everything shown (diagnostics, go to definition, find references, hover, completion) comes
from the graph store.

The server is read-only. It never compiles. Diagnostics refresh when `jazyk compile` or
`jazyk watch` runs beside the editor: each build bumps the store's generation counter, and
the server reloads the graph and republishes when it moves. The project root is found by
walking up to a `jazyk.toml`.

## Hover and the walk

Hovering an entity name shows its card in short, read from the graph: the name with its
stereotype and definition, where it sits (the breadcrumb), the diagram of the level it is
used in (the `.svg` the last build rendered, captioned with the view id), an `Inside`
link to its own level's diagram page with the child count, and one line of links: the
card, the requirements document, the level page. A derived grouping names its pending
proposal. Hovering inside a requirement's sentence shows the requirement card instead
(the statement, the code, the test).

The links to the walk's pages (cards under `jazyk-out/docsgen/entities/`, diagram pages
under `docsgen/diagrams/`, level pages under `docsgen/levels/`) open in the markdown
preview to the side, so the card's own links click through to levels and diagrams. The
requirements document link opens the file at the requirement's heading. Go to Type
Definition (the editor's context menu or the command palette) on an entity opens its
card; Go to Definition still jumps to the defining sentence in the prose.

## Requirements

A built `jazyk` binary. With no setting, the extension looks for
`bootstrap/target/release/jazyk`, then `bootstrap/target/debug/jazyk` inside the
workspace, then falls back to `jazyk` on `PATH`. Set `jazyk.server.path` to override.

```sh
cd ../..        # the bootstrap crate
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
