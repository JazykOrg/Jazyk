# Specification projects and server builds

Status: proposed, 2026-10-03. The cloud server has its own documentation project,
graph output, and Rust application. It depends on the compiler's locally available
library source. These are separate from customer project state in PostgreSQL.

## Concrete layout

```text
docs/
  jazyk.toml                Existing compiler/product specification project
  main.md                  Documentation front door
  .jazyk/                  Existing persistent specification graph
  compiler/                Portable compiler contracts
  consumers/               Portable consumers, including delivery and verification
  frontends/               CLI, ACP, LSP, MCP and portable API contracts
  cloud/
    jazyk.toml              Separate cloud specification project
    cloud.md               Its root document
    accounts.md            Cloud requirements and adapter contracts
    github.md
    storage.md
    operations.md
    .jazyk/                Cloud specification graph, ledger, diagrams, traces
  web/                     Web specification, kept separate from compiler contracts

bootstrap/
  Cargo.toml               Compiler library and customer CLI, progressively split
  src/
  target/release/jazyk     Locally built compiler used for dogfooding

cloud/
  Cargo.toml               jazyk-cloud application
  src/                     HTTP, tenancy, PostgreSQL, GitHub, billing adapters
  target/release/jazyk-cloud
```

These are intended paths, not directories created by this planning change. The
web app stays in `bootstrap/app/` initially. It need not move for the cloud server
to have its own specification and build.

## Separate Jazyk output

Proposed `docs/cloud/jazyk.toml`:

```toml
[docs]
glob = ["**/*.md"]

[roots]
files = ["cloud.md"]

[gen]
deliverable = "../../cloud"
```

Run Jazyk with `docs/cloud/` as its project directory. The default output is then
`docs/cloud/.jazyk/`. This graph has its own ids, leases, generation ledger, and
history. The `gen.deliverable` path identifies generated/maintained server source;
it does not relocate the semantic graph into `cloud/`.

Keep three outputs distinct:

| Output | Purpose |
|---|---|
| `docs/cloud/.jazyk/` | Compiled cloud specification and its generation/verification metadata |
| `cloud/` | Rust server source, settings, migrations, and build files |
| `cloud/target/` | Cargo build artifacts, including the deployable executable |

Cloud customer source and graphs live behind the PostgreSQL workspace/state adapter.
They are not entries in the cloud specification graph or folders under this `.jazyk/`.

## Excluding duplicate input

The current `docs/jazyk.toml` uses `**/*.md`; a nested `jazyk.toml` does not by itself
prove exclusion from that glob. Add `!cloud/**` when creating the cloud docs project.
Keep the existing payload and fixture exclusions. Link the cloud front door from
`docs/main.md` for human navigation without importing its entire graph as core facts.
Validate cross-project document links against the compiler's actual link behavior;
do not imply cross-project semantic linking already exists.

Keep compiler/service interfaces in the portable specification. Cloud docs reference
those contracts and state their own adapter obligations. Avoid copying the compiler
requirements or importing its mutable `.jazyk/` as cloud input. A future independent
web specification can use `docs/web/jazyk.toml` and `!web/**` by the same rule; defer
splitting that graph until its docs move is a coherent task.

The current core graph remains intact. Existing cloud prose, if any has already
been compiled, needs an explicit relocation/export decision before exclusion.
Do not regenerate the compiler graph to accomplish a folder split. Plans remain
under `plans/cloud/`; they are not live dogfood input.

## Local library dependency

Initially the new `cloud/Cargo.toml` uses a path dependency on `../bootstrap`. The
compiler package already provides a Rust library. Add the core/service feature
boundary described in [architecture](./architecture.md#library-and-package-split)
before relying on it as a hosted compiler. After extraction, the path targets the
portable core crate instead. Cloud adapter dependencies point inward only.

Cargo builds the library from the checkout and links it into `jazyk-cloud`. It does
not consume the installed `jazyk` executable or require manually copying an `.rlib`.
Changing shared compiler code rebuilds the dependent server. Pin compatible sources
and a lockfile for CI/deployment.

Keep independent manifests/target directories initially to preserve the owner's
existing `bootstrap/target/release/jazyk` symlink. A future workspace can share Cargo
artifacts after updating scripts and that path deliberately. Extend ignore rules for
`cloud/target/` when the crate is created; the current rule names `bootstrap/target/`.

Illustrative commands after the server crate exists:

```sh
cargo build --manifest-path bootstrap/Cargo.toml --release
cargo build --manifest-path cloud/Cargo.toml --release
```

For dogfooding, invoke the locally built `jazyk` from `docs/cloud/` to compile the
server specification, inspect its graph, and later generate into `cloud/`. That is
the development-time use of the CLI. At service runtime, launch `jazyk-cloud` on EC2;
it calls the linked library for customer projects, with no CLI process per request.

## Delivery of the server itself

The cloud docs project may define a command delivery that builds/tests `cloud/`
through GitHub Actions. Its repository binding must identify the monorepo root and
include the compiler path dependency, Cargo manifests/lockfiles, embedded payloads,
and other required build inputs. A source-only archive of `cloud/src/` is insufficient.
Publish/build those dependencies at the same candidate revision without widening
the cloud documentation glob to compile the whole repository.

The local connector authorizes any selected monorepo input root separately from the
docs root; no tool gains arbitrary parent-directory access from `../../cloud`.
Account for this in candidate construction and project settings before implementation.
Deploy the resulting cloud binary and SQL schema through the service's operations
workflow. This planning task does not build, generate, or deploy that server.
