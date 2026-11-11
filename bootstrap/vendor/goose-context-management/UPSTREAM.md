# Upstream source

This directory contains `goose-context-management` version `0.1.0-alpha.13`,
licensed under Apache-2.0.

- Repository: https://github.com/aaif-goose/goose
- Inspected source revision: `9fbb9a48e1d952daede4ba3f234a22bfe9265027`.
- Published release: https://crates.io/crates/goose-context-management/0.1.0-alpha.13
- Local patch commit: `cdc07ba913ba9887cdcc12dc559f09b7ff838e37`.
- Local patch branch: `jazyk/wasm-compaction` in `~/dev/goose-jazyk-wasm`.
- Patch: use upstream `MaybeSend`/`MaybeSync` and target-specific `async_trait`
  for compaction models, token estimators and the provider wrapper. Add
  Promise-backed WASM tests and native-only Tokio test dependencies.

The manifest is normalized from the published crate, with registry dependencies
instead of workspace/path references. The GDK dependency is pinned exactly.
No Goose application code or private operations are included.

This is a temporary source dependency until an upstream published release includes
the portability change. Replace it with that exact registry release after running
Jazyk native, hosted and browser acceptance checks.

The patched source is published as `jazyk-goose-context-management` version
`0.1.0-alpha.13-jazyk.1`. Its Rust library retains `goose_context_management`.
The upstream copyright and Apache-2.0 license are preserved.
