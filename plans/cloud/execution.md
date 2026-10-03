# Single-host service and GitHub execution

Status: revised 2026-10-03. Owner decision: rely on GitHub Actions for all delivery
commands now. Jazyk-hosted runtimes and containers are deferred, including private
co-located experiments. One EC2 hosts the API and locally installed PostgreSQL.

## Initial topology

- One Linux EC2 instance. Prefer `ca-central-1` initially, subject to a measured
  latency/cost check. This is not a Canada-only processing promise.
- `jazyk-cloud serve` under systemd, with an established TLS proxy if needed.
- Local PostgreSQL on a Unix socket or loopback, never a public database port.
- Background jobs in the Rust service dispatch GitHub runs and collect evidence.
- Persistent database storage on encrypted EBS, with disk headroom for WAL/backups.
- Existing static jazyk.dev deployment remains separate.

There is no container daemon, delivery worker, project shell, or self-hosted Actions
runner on this host. No new Node service, Redis, managed SQL, load balancer, or
orchestration cluster is required initially. Size the machine for compilation,
LLM sessions, API traffic, and bounded report ingestion, not customer builds.

## Work placement

| Work | Where it runs |
|---|---|
| Parsing, graph reconciliation, generation, scoped file tools | Selected local Jazyk or shared Jazyk Cloud context |
| LLM-run tests | Jazyk verification sessions, launched/viewed in jazyk.dev using the selected backend/model |
| Git publication and Actions orchestration | API adapters, without executing project code |
| Delivery commands, dependency setup, builds, Python/JavaScript tests | GitHub-hosted Actions runners |
| Criterion evidence validation and ledger import | Jazyk harness, with the project's normal commit authority |

The web app initiates work and displays results. It does not need to run Python,
Node, or a shell in the browser. LLM tests also do not imply browser-only inference.
Their file inspection and model session stay in the existing Jazyk path. When an
LLM check needs a built artifact, wait for CI to produce and upload it.

## External run contract

Implement the GitHub adapter with these operations:

- Prepare and publish an immutable candidate and generated workflow.
- Dispatch with delivery/attempt identity, pinned source, and a recorded timeout.
- List runs, fetch status/jobs, consume webhooks, and poll after missed events.
- Fetch available logs, test reports, and uploaded files into bounded retained storage.
- Cancel or explicitly retry with provider run/attempt identity and known effects.

Persist intent before dispatch and save provider receipts afterward. Timeouts with
unknown outcomes require inspection before retry. Match results to the checked-out
candidate and workflow revision. See [GitHub](./github.md) for permission and setup
requirements and [testing](./testing.md) for verdict rules.

No general runtime broker, materialization service, interactive terminal, or live
filesystem protocol is required now. Use existing HTTP/GitHub libraries and a small
adapter around the delivery contract. Do not build future container APIs speculatively.

## Outputs and retention

GitHub owns execution scratch and runner cleanup. The workflow uploads selected
artifacts and structured reports. Jazyk retains necessary evidence before it expires,
under project quotas and retention policy. A provider URL alone is not retained bytes.

Model file tools expose candidate source and uploaded files only. Missing artifacts
are unavailable, not empty files. Optional file imports use revision checks. Local
projects require an online connector to save imported evidence or source changes.
An already dispatched Actions run can continue while the connector is offline.

Do not introduce build-cache management in this scope. Workflows can adopt GitHub's
caching facilities later. A cache is an optimization, never a delivery checkpoint,
proof of verification, or a dependency for restoring project state.

## Operations and failure recovery

One instance is a single failure domain. Use bounded admission, compilation memory
limits, and report/archive byte limits. Refuse work before memory or disk reaches
the API/database reserve. Report queueing rather than overcommitting resources.

Take encrypted, consistent PostgreSQL backups off-instance and test restore onto a
fresh EC2 host. Daily backups are a private-test starting policy with a disclosed
data-loss window. Add a measured WAL/PITR policy before paid durable storage claims.
EBS persistence or backups stored only on the same instance are insufficient.

On restart, recover graph jobs from acknowledged commits and reconcile GitHub runs
and refs before replaying external effects. Actions can continue during an EC2 outage.
A restored database may predate a push or run; inspect those effects after recovery.
Resume report imports idempotently rather than rerunning tests to reconstruct receipts.

Back up encrypted credential records and recoverable key configuration. Rotate tokens,
verify/deduplicate webhooks, and handle App revocation. Monitor queue depth, memory,
disk/WAL, backup age, GitHub API limits, missing reports/receipts, and model spend.
Keep private project content out of shared logs.

Cost decisions cover EC2, EBS, backups, data transfer, model usage, and GitHub-billed
Actions. Jazyk does not sell its own execution minutes in this scope. Keep the
Canadian Stripe account; choose subscription currency, allowances, and any declared
orchestration charges before paid launch.

## Deferred Jazyk runtime

An isolated runner provider, materialized workspaces, live file access, follow-up
commands, runtime secrets, resource accounting, cleanup, and build caches are all
later work. No provider selection or runtime migration is a prerequisite for launch.

If this returns to scope, preserve candidate/attempt/checkpoint identities and add
an isolated execution adapter. Re-evaluate the earlier container design and provider
research then. Do not place customer execution next to the account database merely
because the initial service uses one EC2 instance.
