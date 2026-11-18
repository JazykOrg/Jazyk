# Jazyk for Zed

This adapter starts `jazyk lsp` for Markdown. It preserves Zed's Markdown grammar
and other language servers. Chat uses the separate Jazyk ACP registration.

Build from this directory:

```sh
rustup target add wasm32-wasip2
PATH="$HOME/.cargo/bin:$PATH" cargo build --locked --release --target wasm32-wasip2
cp target/wasm32-wasip2/release/jazyk_zed.wasm extension.wasm
```

Run `jazyk zed` from your project. It writes `.zed/settings.json`, prepares the
adapter source, and prints its directory. In Zed, run `zed: install dev extension`
and select that directory. Zed builds and manages the development extension.
The launcher does not write into Zed's private installed-extension directory.
A source checkout uses this directory directly; a packaged Jazyk downloads the
source ZIP from the versioned GitHub editor release.

Verify by opening a Markdown document with a missing local link. Jazyk should
appear in the language-server status and report `broken-link`. The Zed log must
show the configured executable starting with `lsp`. Opening the chat agent alone
is not an LSP check. Hover diagrams and code lenses depend on Zed's rendering.
