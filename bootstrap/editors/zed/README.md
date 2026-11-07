# Jazyk for Zed

This adapter starts `jazyk lsp` for Markdown. It preserves Zed's Markdown grammar
and other language servers. Chat uses the separate Jazyk ACP registration.

Build from this directory:

```sh
rustup target add wasm32-wasip2
PATH="$HOME/.cargo/bin:$PATH" cargo build --locked --release --target wasm32-wasip2
cp target/wasm32-wasip2/release/jazyk_zed.wasm extension.wasm
```

Run the built Jazyk CLI as `jazyk zed` from your project. The launcher installs
`extension.toml` and `extension.wasm` into Zed's extension directory and writes
project settings. Zed watches that directory for changes. If it has not refreshed,
run `zed: reload extensions` or restart Zed.

Alternatively, use `zed: install dev extension` and select this directory. Then
run `jazyk zed` to configure the project. No public extension listing is required.

Verify by opening a Markdown document with a missing local link. Jazyk should
appear in the language-server status and report `broken-link`. The Zed log must
show the configured executable starting with `lsp`. Opening the chat agent alone
is not an LSP check. Hover diagrams and code lenses depend on Zed's rendering.
