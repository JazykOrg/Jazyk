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
| 0. Contracts and fixtures | Workspace/commit context, delivery envelope, TOML schema, provider capability matrix | Git, script, and Actions examples use one candidate/attempt/checkpoint model | 2–4 |
| 1. Portable multi-project runtime | Per-instance VFS, run-local LLM state, no-shell host profile, crate boundary | Two memory-backed projects compile concurrently without filesystem or state leakage | 8–14 |
| 2. PostgreSQL project service | Versioned storage, cloud graph commits, jobs/events, leases, idempotent receipts | Restart and competing writers preserve one history and correct authorization | 6–10 |
| 3. GitHub identity and projects | OAuth/App installation, minimal project UI, cloud workspace and GitHub source adapter | Signed-in users see only their projects; repository access is a separate grant | 5–9 |
| 4. Delivery memory and Git | TOML CRUD, candidate/observation/plan, separate durable store, GitHub and Git adapters | Publish pinned files without a container; survive lost receipt and compiler output reset | 6–10 |
| 5. Actions delivery | Generated workflow/entrypoint, install/publish/dispatch, watch, artifacts, report parser | Test evidence matches candidate and run; unknown dispatch does not duplicate effects | 4–7 |
| 6. Local-remote and IDE parity | Connector revision/commit protocol, ACP/LSP proxy, local delivery receipts | Local graph remains authoritative; offline/reconnect never overwrites new local work | 8–14 |
| 7. Single-EC2 deployment and runtime | systemd/TLS, local PostgreSQL, rootless worker, materialization, files/logs/diffs, backup restore | Same script runs on the private host with bounded resources; compilation starts no runtime | 5–9 |
| 8. Complete user workflow | Project and delivery panels, status distinctions, import/conflicts, exports | Accountless → subscription → delivery → local use preserves project and decisions | 5–8 |
| 9. Public service gate | Stripe/credits, measured provider rates, operations, isolated runner adapter or Actions-only scripts | Paid admissions and restore pass; customer scripts cannot execute beside the account database | 8–15 plus provider-specific isolation work |

Phases 0–8 total approximately 49–85 engineering days. A private GitHub/Actions
vertical path appears during phases 3–5, before the full connector and container
experience. Run small slices end to end as each contract becomes usable. Re-estimate
after phases 1 and 5. Existing implementation or reusable adapters may reduce the
range; state cleanup, protocol gaps, and recovery requirements can increase it.

Do not build a runtime marketplace, caching system, generic cloud provider registry,
or universal migration planner before the first delivery works. The private container
adapter and GitHub Actions path are both in this plan; they serve different capabilities.

## Docs-first changes

Before each behavior change, write its accepted contract into canonical docs:

- `docs/compiler/workspaces.md`: contexts, virtual storage, atomic commits, epochs.
- Existing compiler settings, graph, sessions, and control-plane pages: concurrency,
  no-shell profile, delivery configuration reference, operational state lifetime.
- `docs/consumers/delivery.md`: candidate, destination, preparation, attempt,
  verification, preservation, and checkpoint. Keep the existing internal compilation
  word `release` distinct from delivery.
- Existing generation/binding/criterion pages: artifact capture and revision-bound
  evidence from executed tests. No new authored graph kind is required initially.
- `docs/frontends/server.md` and existing CLI/ACP/LSP/MCP pages: multi-project API,
  tool capability checks, connector mapping, event streams, and run-file access.
- `docs/web/`: project selection, accountless/login flows, delivery panels and failures.
- `docs/cloud/`: PostgreSQL, accounts, GitHub, credits, runners, and one-host operations.
- Prompt/skill payloads in the existing embedded directories for accepted model work.
  Reuse the same bytes in docs and code.

Use a separate on-demand delivery consumer worklist initially. Keep compiler goal
derivation unchanged unless an implemented fixture proves new goal kinds necessary.
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

### Delivery configuration and durability

- Model CRUD preserves unrelated TOML/comments and rejects stale revisions.
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
- Actions file tools expose candidate/uploaded files only; container-only capabilities
  are unavailable rather than simulated.

### Container and single-host operation

- A script sees only its run's files and allowed secrets. It cannot reach service
  credentials, the runtime socket, metadata, PostgreSQL, or another run's volume.
- Memory/process/disk/log limits work; exhausting one run preserves API/database
  headroom and another project's progress.
- Cancel/restart/timeout preserves unknown external outcomes and cleans orphaned runs.
- Importing an old run diff conflicts with newer source and never overwrites it.
- Restoring an off-instance backup onto a fresh host recovers graph/delivery state
  and reconciles provider effects that happened after the backup.
- Changing runner provider leaves TOML, attempt identity, and checkpoint meaning
  unchanged. Customer scripts use Actions or the isolated adapter before public use.

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

- One EC2, local PostgreSQL, and co-located runtime for the private starting point.
- Runtime only for delivery. Compilation and virtual generation have no project container.
- Local-backed cloud compilation commits graph and human decisions to the local folder.
- Named deliveries in `jazyk.toml`, editable and executable by scoped model tools.
- Git publication without containers; script entrypoints on containers or Actions.
- Watch execution and inspect supported run files/results through the shared API.
- Separate the commercial server from the customer compiler distribution.
- Preserve ACP/LSP, accountless local access, GitHub sign-in direction, and the
  established subscription path. Keep Canadian Stripe and production model choice open.

## Recommended defaults adopted for planning

These make the plan implementable without another round of abstract choices:

- Rust `jazyk-cloud` links compiler/consumer libraries; no Node wrapper.
- One GitHub App supplies OAuth identity and repository installation grants.
- PostgreSQL holds bounded cloud blobs and records initially; caches are absent.
- `.jazyk-delivery/` stores local operational memory independently of compiler output.
- A separate on-demand consumer worklist and full candidate snapshots first.
- Git/Actions before the co-located container adapter, with the same script entrypoint.
- JUnit reports plus explicit criterion mappings for initial test ingestion.
- Rootless Docker for the private runner experiment, one active container initially.
- GitHub-only sign-in and existing-repository selection; no automatic repo creation.

## Choices still needed

- Production model supplier/catalog and task policy after benchmarking. Preserve
  the [owner's private local Codex experiment](../subscription/codex-testing.md);
  do not move its credentials onto the shared EC2 service.
- Credit rates, subscription currency/allowances, execution charging policy,
  local-result retention, artifact caps, and paid backup/recovery promises.
- Instance size and exact image/toolchain profiles after measuring fixtures.
- Isolated runner provider before real customer container execution. Actions-only
  customer delivery can launch first while that selection remains open.
- Which stateful or non-software destination to implement after Git/script/Actions,
  including its specific preservation and recovery obligations.

The Rust host and OAuth App versus GitHub App choice have recommendations above.
They are not blockers waiting for another permission question. Implemented behavior
still needs canonical specification and the acceptance gates before rollout.
