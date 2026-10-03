# Implementation order and decisions

Status: planning estimates, 2026-10-03. This is the unified sequence for Cloud and
delivery. The old subscription/delivery sequences are inputs, not additional work
to execute independently.

## Milestones

Estimates are engineering days for one maintainer, including docs-first changes and
focused validation. They are planning ranges, not delivery dates. Core state cleanup
and distributed local commits are substantial even with one EC2 instance.

| Phase | Scope | Exit condition | Estimate |
|---|---|---|---|
| 0. Contracts and fixtures | Workspace/commit context, delivery envelope, command TOML, separate cloud docs project | Git and Actions use one candidate/attempt/checkpoint model; LLM and CI tests have distinct evidence | 2–4 |
| 1. Portable multi-project runtime | Per-instance VFS, run-local LLM state, no-shell host profile, crate boundary | Two memory-backed projects compile concurrently without filesystem or state leakage | 8–14 |
| 2. PostgreSQL project service | Versioned storage, cloud graph commits, jobs/events, leases, idempotent receipts | Restart and competing writers preserve one history and correct authorization | 6–10 |
| 3. GitHub identity and projects | OAuth/App installation, minimal project UI, cloud workspace and GitHub source adapter | Signed-in users see only their projects; repository access is a separate grant | 5–9 |
| 4. Delivery memory and Git | TOML CRUD, candidate/observation/plan, separate durable store, GitHub and Git adapters | Publish pinned files without a container; survive lost receipt and compiler output reset | 6–10 |
| 5. Actions delivery and verification | Automatic workflow generation, publish/dispatch, run history, reports, criterion mapping, pending-CI scheduling | Code-test evidence matches candidate and run; LLM tests stay in Jazyk; unknown dispatch does not duplicate effects | 6–10 |
| 6. Local-remote and IDE parity | Connector revision/commit protocol, ACP/LSP proxy, local delivery receipts | Local graph remains authoritative; offline/reconnect never overwrites new local work | 8–14 |
| 7. Single-EC2 service deployment | systemd/TLS, local PostgreSQL, API resource limits, backup restore, GitHub reconciliation | Restart restores project state and reconciles CI; no customer commands execute on EC2 | 2–4 |
| 8. Complete user workflow | Project and delivery panels, status distinctions, import/conflicts, exports | Accountless → subscription → delivery → local use preserves project and decisions | 5–8 |
| 9. Public service gate | Stripe/credits, measured provider rates, operations, GitHub grants/revocation and CI recovery | Paid admissions and restore pass; Actions is the only command-delivery backend | 8–15 |

Phases 0–8 total approximately 48–83 engineering days. A private GitHub/Actions
vertical path appears during phases 3–5, before the full connector and production
experience. Run small slices end to end as each contract becomes usable. Re-estimate
after phases 1 and 5. Existing implementation or reusable adapters may reduce the
range; state cleanup, protocol gaps, and recovery requirements can increase it.

Do not build a runtime marketplace, caching system, generic cloud provider registry,
or universal migration planner before the first delivery works. Jazyk-hosted runtime,
container workers, provider selection, and interactive run tools are deferred. They
are not private-beta experiments or public-launch gates in the current sequence.

## Docs-first changes

Before each behavior change, write its accepted contract into canonical docs:

- `docs/compiler/workspaces.md`: contexts, virtual storage, atomic commits, epochs.
- `docs/cloud/jazyk.toml` and a parent `!cloud/**` exclusion: separate cloud graph
  output and `cloud/` deliverable, following [project layout](./projects.md).
- Existing compiler settings, graph, sessions, and control-plane pages: concurrency,
  no-shell profile, delivery configuration reference, operational state lifetime.
- `docs/consumers/delivery.md`: candidate, destination, preparation, attempt,
  verification, preservation, and checkpoint. Keep the existing internal compilation
  word `release` distinct from delivery.
- Existing generation/binding/criterion pages: artifact capture and revision-bound
  evidence from executed tests. Adapt the current direct build/test shell calls to
  pending CI in the hosted profile; retain LLM verification in Jazyk. No new authored
  graph kind is required initially.
- `docs/frontends/server.md` and existing CLI/ACP/LSP/MCP pages: multi-project API,
  tool capability checks, connector mapping, event streams, and run-file access.
- `docs/web/`: project selection, accountless/login flows, delivery panels and failures.
- `docs/cloud/`: PostgreSQL, accounts, GitHub, credits, and one-host operations.
- Prompt/skill payloads in the existing embedded directories for accepted model work.
  Reuse the same bytes in docs and code.

Use a separate on-demand delivery consumer worklist initially. Keep compiler goal
identities and gates; adapt readiness/scheduling for pending CI without automatically
publishing source or holding a build lease throughout the external run.
Delivery findings feed ordinary diagnostics/proposals and then harness-derived
generation work. Provider operations do not become semantic graph mutations.

## Required acceptance cases

### Compiler, storage, and customer binary

- Two projects with identical paths and graph ids compile concurrently without
  reading each other's files, credentials, fallback flags, events, or token counts.
- A guard rejects all project filesystem and process access in the hosted compiler.
  A no-op build still derives zero goals and makes zero model calls.
- Cancellation, stale epochs, source edits, and lost acknowledgements preserve
  acknowledged state and return the original receipt on retry.
- Local graph and human decisions stay authoritative through remote compilation,
  connector loss, cloud outage, and return to accountless use.
- The shipped customer binary and WASM dependency trees exclude cloud SQL, OAuth,
  Stripe, and container adapters. Existing CLI/GUI/ACP/LSP still function.
- The cloud specification compiles into its own `.jazyk/`; parent docs globs exclude
  it. Building `jazyk-cloud` builds/links the local core library without launching
  the customer CLI or changing the existing local binary path.

### Delivery configuration and durability

- Model CRUD preserves unrelated TOML/comments and rejects stale revisions.
- A command edit regenerates only its owned workflow/helper. Manual edits create
  a conflict, and a stale generated workflow cannot run against a newer definition.
- Editing/deleting a definition never publishes, cancels, or erases history implicitly.
- A changed script/config/candidate invalidates the affected prepared plan/evidence.
- Removing `.jazyk/` preserves delivery bindings and checkpoints. Missing delivery
  memory requires observation/adoption, not assumed first installation.
- Lost response after external success leads to inspection, not duplicate push,
  dispatch, append, or migration. Partial delivery leaves the old checkpoint intact.
- Retained checkpoints can recover actual artifacts, not just digests or expired URLs.

### GitHub, Actions, and evidence

- Login succeeds without a repository grant. Repository selection does not grant
  access to unrelated repositories or create one implicitly.
- Branch conflicts/protection, workflow permissions, App revocation, and default-branch
  workflow setup produce actionable states without force-push workarounds.
- Dispatch records the actual run and rerun attempt. Evidence verifies the checked-out
  candidate separately from the workflow revision.
- Failed tests upload available reports. Missing, skipped, stale, malformed, unmapped,
  or expired reports never become passing criterion evidence.
- Aggregate command success cannot verify unrelated criteria. Setup/build failure
  stays distinct from assertion failure. All required current criteria govern the
  requirement verdict, including LLM criteria.
- LLM tests run through Jazyk with no CI dispatch. CI cannot overwrite LLM evidence,
  and a model cannot substitute its judgment for a missing programmatic result.
- Actions file tools expose candidate/uploaded files only. Run history and test
  details work from jazyk.dev without interactive access to a runner.
- Pending CI neither holds the writer lease nor dispatches twice on goal re-derivation.
  A changed criterion/test/source makes old results historical; local imports require
  the connector receipt before being reported saved.

### Single-host operation

- No delivery command, setup script, or self-hosted CI runner executes on EC2.
- Compilation and report/archive limits preserve API/database headroom and other
  projects' progress. CI jobs do not receive Jazyk account or database credentials.
- Cancel/restart/timeout preserves unknown external outcomes and reconciles GitHub.
- Importing an old uploaded-file diff conflicts with newer source and never overwrites it.
- Restoring an off-instance backup onto a fresh host recovers graph/delivery state
  and reconciles provider effects that happened after the backup.

### Delivery design fixtures

Keep the [original examples](../delivery/examples.md) as contract checks: a static
site with old assets, a database upgrade with supported older clients/new writes,
and a shared document with outside edits. Use deterministic mock destinations first.
Do not claim the first Git/Actions adapters implement these complete preservation
procedures. A passing build does not establish transition correctness.

Use targeted tests at the corresponding implementation stage. This planning change
needs link/style and example-schema checks, not a compiler build or paid model run.
Future binary changes finish with the repository-required release build.

## Decisions settled by this direction

- One EC2 with local PostgreSQL and the Rust API. Our own runtime is deferred.
- GitHub Actions is the only execution backend for delivery commands.
- LLM tests remain in Jazyk/jazyk.dev. Programmatic tests execute in CI and feed
  matching pass/fail evidence into the criterion ledger.
- Local-backed cloud compilation commits graph and human decisions to the local folder.
- Named deliveries in `jazyk.toml`, editable and executable by scoped model tools.
- Git publication without containers; project commands generate Actions workflows.
- Watch execution and inspect supported run files/results through the shared API.
- Separate the commercial server from the customer compiler distribution.
- Give its cloud specification a separate Jazyk project/output and link the Rust
  application against the local compiler library during its Cargo build.
- Preserve ACP/LSP, accountless local access, GitHub sign-in direction, and the
  established subscription path. Keep Canadian Stripe and production model choice open.

## Recommended defaults adopted for planning

These make the plan implementable without another round of abstract choices:

- Rust `jazyk-cloud` links compiler/consumer libraries; no Node wrapper.
- One GitHub App supplies OAuth identity and repository installation grants.
- PostgreSQL holds bounded cloud blobs and records initially; caches are absent.
- `.jazyk-delivery/` stores local operational memory independently of compiler output.
- A separate on-demand consumer worklist and full candidate snapshots first.
- One command field, e.g. `npm test`, with generated workflow/helper files.
- JUnit reports plus explicit criterion mappings for initial test ingestion.
- GitHub-only sign-in and existing-repository selection; no automatic repo creation.

## Choices still needed

- Production model supplier/catalog and task policy after benchmarking. Preserve
  the [owner's private local Codex experiment](../subscription/codex-testing.md);
  do not move its credentials onto the shared EC2 service.
- Credit rates, subscription currency/allowances, orchestration charging policy,
  local-result retention, artifact caps, and paid backup/recovery promises.
- API/database instance size and supported Actions toolchain/report profiles after
  measuring fixtures. Runtime provider selection is deferred with our own runtime.
- Which stateful or non-software destination to implement after Git/script/Actions,
  including its specific preservation and recovery obligations.

The Rust host and OAuth App versus GitHub App choice have recommendations above.
They are not blockers waiting for another permission question. Implemented behavior
still needs canonical specification and the acceptance gates before rollout.
