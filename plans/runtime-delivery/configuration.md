# Project configuration and scripts

Status: proposed syntax for the [discussion draft](README.md). These examples
illustrate the contract. They are not accepted schema or implemented features.

## Start small

Project prose states what to build and where it should go. The generated settings
provide the executable entrypoint. E.g.:

```toml
[runtimes.default]
image = "ghcr.io/example/book-tools:1"

[deliveries.book]
label = "Build book"
script = "scripts/build-book.sh"
products = ["dist/book.pdf", "dist/book.epub"]
```

The delivery key is its stable identity. `label` is the button text. If omitted,
derive a readable label from the key. `script` names an existing project file.
With no runtime reference, use `default`. With no runtime table, use a published,
versioned Jazyk base image. Its tool inventory is explicit and modest. Generation
selects another image when it needs a different toolchain.

The script can use any tools in its environment. It can call other scripts, build
and upload artifacts, or perform several phases. There is no `book`, `app-store`,
`website`, or `presentation` delivery kind. An empty delivery table is valid.

Script paths and working directories are project-relative. The default interpreter
is `/bin/sh`; `interpreter = ["python3"]`, for example, runs a Python script.
Interpreter arguments and script arguments are structured arrays. Paths are never
inserted into an unescaped shell string. Investigation and repair checks use named
script actions too. The model can create a focused diagnostic script and action,
then submit it as a finite run; there is no separate arbitrary-exec tool.

The requested image tag is resolved to a digest before starting. Record the digest,
platform, script hash, configuration hash, and input revision with the attempt.
An image must meet the adapter's supported ABI and execution requirements. Pull or
template preparation failure is a setup failure, with logs, before the script runs.

## Optional controls

All actions use the same small execution fields. A delivery is one such action.

| Field | Meaning |
| --- | --- |
| `script` | Existing entrypoint, required unless using a task or the Git shortcut. |
| `runtime` | Named image/environment, default `default`. |
| `cwd` | Project-relative working directory, default project root. |
| `args`, `interpreter` | Literal argument arrays. |
| `size` | Requested resource class, constrained by host policy. |
| `timeout_seconds` | Fixed execution bound for the complete action, constrained by host policy. |
| `products` | Paths or globs for durable outputs. |
| `caches` | Names of disposable cache profiles to restore and possibly save. |
| `env` | Non-secret environment values. |
| `secrets` | References to separately granted credentials, never their values. |

Provider names, provider credentials, account limits, billing multipliers, and
publication grants are host/account settings. The model cannot change them by
editing `jazyk.toml`. A runtime may request a network capability, but only the host
can grant it. The UI shows the resolved environment and effective limits.

`products` defaults to empty. A delivery that only pushes to a registry need not
retain a local file. Verification reports are declared by the test action, and
runtime logs are always managed by the harness. Neither needs a product glob.

## Reuse only when needed

An optional `[tasks.*]` table names an action reused by generation, verification,
and delivery. A delivery specifies either its own script or a task reference.
These are mutually exclusive. A task has either a script or an ordered step list,
never both. There are no dependency graphs, matrices, or expression language in v1.

```toml
[runtimes.node]
image = "node:24-bookworm"
size = "small"

[tasks.presentation]
runtime = "node"
script = "scripts/render-slides.sh"
products = ["dist/slides.pptx", "dist/slides.pdf"]

[deliveries.presentation]
label = "Create presentation"
task = "presentation"
```

Generation's existing ledger build changes from a separate copied command to an
action reference when this form is used. Its `produces` expectation still exists.
The ledger records the resolved action hash as evidence, not a second editable
definition. The project can retain a direct build command during transition; the
harness freezes it into a finite action rather than exposing arbitrary exec.

Project settings span document sets. A ledger reference identifies its set and
task. A delivery declares the relevant sets when evidence or generated inputs
depend on more than one. Identical task names cannot silently refer to different
set-relative working directories.

## Build cache before tests

The following is the optional structured form for a project that needs a cache
checkpoint between build and test. The ordered steps share one sandbox.

```toml
[runtimes.rust]
image = "rust:1-bookworm"

[caches.cargo]
paths = [".cache/cargo/registry", ".cache/cargo/git", "target"]
key_files = ["Cargo.lock", "Cargo.toml"]
restore = "exact"

[tasks.check]
runtime = "rust"
caches = ["cargo"]
env = { CARGO_HOME = "/project/.cache/cargo" }

[[tasks.check.steps]]
id = "build"
script = "scripts/build.sh"
save_caches = ["cargo"]

[[tasks.check.steps]]
id = "test"
script = "scripts/test.sh"
```

`save_caches` runs only after that step exits successfully. The supervisor pauses
before the next step, seals a cache snapshot, then continues. Failed tests do not
discard a previously sealed build cache. A failed build does not save it. Cache
storage failure is normally a warning, not a failed build.

A single-script action saves requested caches only after script success. It does
not guess where the build ended. Use ordered steps when a cache must be saved
before a later test failure. All steps and checkpoints are fixed before dispatch;
the model cannot append commands or direct a checkpoint during the run. Avoid
requiring scripts to call a proprietary Jazyk command for routine cache control.

After the action ends, the host collects retained file changes, products, reports,
and logs, then destroys the container. Repeating an action starts a fresh environment
from its selected project revision and declared cache. A timeout or resource-size
change applies only to the next run. A failed step normally skips the remaining
execution steps and enters automatic collection/teardown.

## Test actions and selection

Tests use the same action shape. A small optional suite configuration identifies
the execution script and report format. The ledger owns criterion bindings.

```toml
[tests.unit]
runtime = "node"
script = "scripts/test-unit.sh"
selection = "manifest"
reports = ["test-results/unit.xml"]
report_format = "junit"
```

The harness supplies a read-only JSON selection file and its path through
`JAZYK_TEST_SELECTION`. It contains selected requirement and criterion IDs, bound
test identities, exact selectors, and whether the request is `all`. The generated
script translates these into its framework's arguments without shell evaluation
of selectors. The harness does not learn every framework's command syntax.

For a project with no suite adapter, retain the existing per-row `run` and `cwd`
contract and freeze selected commands into a finite test action. Selection capabilities
are explicit. A suite that only supports `all` must say so; do not claim a filter
ran when it was ignored. See [verification](generation-testing.md#selection-and-results).

## GitHub sync shortcut

```toml
[deliveries.github]
kind = "github-sync"
label = "Sync to GitHub"
repository = "example/book"
branch = "main"
include = ["docs/**", "scripts/**", "src/**", "tests/**", ".jazyk/jazyk.toml", ".jazyk/.gitignore"]
exclude = ["dist/**"]
```

This action publishes a canonical project snapshot through a Git adapter. It
allocates no sandbox and does not run project code. A selected repository and
branch are visible in settings and on the run. Credentials stay in Cloud's GitHub
App binding or the user's local Git credential mechanism.

The example uses explicit globs. The proposed default, if `include` is omitted,
is all canonical authored and generated project files plus explicitly promoted
products. Always exclude Git internals, cache/scratch files, secrets, conversations,
and compiler operational state. `.jazyk/jazyk.toml` and `.jazyk/.gitignore` remain
eligible. Optional graph exports are explicit artifacts, not a copy of mutable
`.jazyk` state. Git ignore rules inform the preview but are not the sole security
or retention policy.

Preserve compare-and-swap against the remote head and managed-file ownership.
Never force-push or delete remote-only files. A concurrent remote edit produces a
conflict with an inspect/import path. Initial adoption of conflicting paths needs
an explicit choice. A retry with the same operation ID returns the same attempt.
Do not automatically retry an uncertain external write.

Call this one-way `Sync to GitHub` in v1 and explain its direction. Import/pull is
a separate operation. Continuous bidirectional sync would require merge handling
and is an alternative, not an implication of this button.

The current GitHub projection supports only text. Binary products need the shared
binary manifest and a suitable Git blob API or local Git path, with size limits.
Large products should use artifact downloads, Releases, LFS, or a scripted upload
chosen by the project. An unsupported large binary is reported before mutation.

One project can have several sync deliveries for different repositories or
branches. Move away from one implicit project-wide target while reusing its saved
binding as a default during transition. Each destination gets its own permission
binding, head expectation, and publication checkpoint.

## How the model creates definitions

Generation packages include the relevant delivery requirements, current settings,
existing entrypoints, runtime capabilities, and effective limits. The model can
write scripts and stage a restricted settings patch using a dedicated tool.

The harness validates TOML, named references, entrypoint existence, normalized
paths, supported capabilities, and permitted settings fields. It preserves unrelated
settings and comments. It records the originating requirements and input hashes in
generation metadata. A missing delivery requirement can produce a proposal or
question, not an invented external destination.

Saving a definition does not execute it or grant credentials. A build/test action
can run within existing generation authority. Publishing follows the selected
[autonomy policy](decisions.md#delivery-autonomy). Changing scripts must not turn a
previously harmless action into a way around that policy.
