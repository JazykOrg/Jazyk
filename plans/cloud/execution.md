# Single-host deployment and delivery runtime

Status: proposed, 2026-10-03. Owner direction: start with one EC2 instance containing
the API, locally installed PostgreSQL, and container runtime. Runtime is used only
during delivery. Relocate it when moving to real customer execution.

## Initial topology

- One Linux EC2 instance. Prefer `ca-central-1` initially, subject to a measured
  latency/cost check. This is not a Canada-only processing promise.
- `jazyk-cloud serve` under systemd, with an established TLS proxy if needed.
- Local PostgreSQL on a Unix socket or loopback, never a public database port.
- `jazyk-cloud delivery-worker` under a different OS user, with rootless Docker
  as the first runtime adapter to validate.
- Persistent database storage on encrypted EBS, separate from disposable run scratch
  and runtime image storage. Reserve disk headroom for database/WAL and backups.
- Existing static jazyk.dev deployment remains separate. No new Node web service,
  Redis, managed SQL, load balancer, or orchestration cluster is required initially.

Suggested sizing experiment: an x86 instance class around 4 vCPU and 16 GiB RAM,
one active delivery container, and a bounded compiler pool. This is a sizing trial,
not a capacity guarantee or current price quote. Check container image architecture
and actual compiler/build memory before selecting an instance type. Keep database
and API resource reservations even when a delivery exhausts its limit.

## What starts a runtime

Only a requested `script` delivery with `runner = "container"` allocates a container.
Compile, graph queries, delivery CRUD, candidate preparation that needs no execution,
Git publishing, and Actions dispatch do not. Observation/verification procedures
that require a shell run as named phases of a delivery attempt, not on the API host.

Generating source or tests stores virtual artifacts. Running them is an explicit
delivery. A generation workflow needing execution pauses for that action and consumes
its evidence afterward. There is no hidden fallback from compilation to a container.

The runtime is not permanent hosting for the generated application. An entrypoint
can deploy elsewhere or create a bounded preview/test process during its attempt.
Destroying the delivery container must not destroy the canonical project or the
deployed product's durable state.

## Runner contract

The replaceable runner implements:

- Start from an immutable candidate manifest, approved environment digest, argv,
  resource limits, secret grants, attempt id, and action id.
- Report provider handle, sequenced status/logs, exit state, and resource observations.
- List/read bounded run files, execute authorized follow-up commands if supported,
  capture immutable outputs, and produce a diff from the base candidate.
- Cancel, destroy, and reconcile inventory after a worker/API restart.

Capabilities describe the backend. Git has receipts but no runtime files. Actions
has repository/artifact files and provider status, not interactive commands. Containers
have live filesystem access. The common API reports unsupported operations explicitly.

Issue a narrow attempt capability to the worker. It can fetch that candidate and
write that attempt's events/artifacts through the API. It cannot query accounts,
read another project, or use general cloud storage credentials. Once the backend
is remote, the same contract uses a remote agent or provider SDK.

## Container lifecycle

1. Reserve the delivery budget and worker slot. Persist intent before starting.
2. Materialize the candidate into a new run volume. Validate paths, symlinks, file
   count, and byte limits. Do not mount a local user's live project directory.
3. Start a fresh approved image as an unprivileged user. Invoke the configured
   entrypoint inside the candidate root. No command interpolation on the host.
4. Stream bounded logs and events. Expose files only through the run capability.
   A live read may race a write; report that rather than pretending it is a snapshot.
5. After exit, snapshot declared reports/artifacts and a selected file diff. Retain
   them durably before cleanup. Offer a short bounded inspection period for a failed
   attempt if requested; it is still billed/limited runtime.
6. Import approved changed files through the project's normal revision checks.
   Local projects require their connector to commit imports and evidence.
7. Destroy the container and run volume. Keep required output/evidence in the
   operational store. Reconcile orphaned volumes/containers on restart.

Do not mount the runtime socket, database files, service secrets, host home, or other
projects. Deny privileged mode, host networking, and arbitrary host mounts. Limit
CPU, memory, process count, wall time, scratch size, and log volume. Verify limits
actually work with the selected rootless/cgroup setup, including fork/disk exhaustion.

Rootless mode runs the daemon and containers without root privileges and reduces
some risks. It does not create a separate host kernel or a sufficient boundary for
hostile customer builds next to the account database.
[Docker rootless mode](https://docs.docker.com/engine/security/rootless/).

Block container access to the API's private services, PostgreSQL, worker control
socket, cloud metadata, and private/link-local networks. Permit only the network
access required by the declared task, including dependency registries and named
destinations. IMDSv2 alone is not a container egress policy; AWS documents container
hop-limit considerations. Test actual access denial.
[EC2 metadata service](https://docs.aws.amazon.com/AWSEC2/latest/UserGuide/configuring-instance-metadata-service.html).

These are private development controls for the requested single-host setup. Do not
describe it as strong multi-tenant code isolation. Cloud data processing still needs
the application-level authorization in [architecture](./architecture.md).

## Later runtime relocation

Before allowing real customers to run scripts on Jazyk infrastructure, switch the
container adapter to a separate execution host or an isolated hosting provider.
GitHub Actions can already run customer scripts outside the Jazyk host.

The migration moves candidate transfer, container launch, log/file access, credentials,
and cleanup behind the runner adapter. Account/project state, credit reservations,
delivery definitions, checkpoints, and the API remain in `jazyk-cloud`. Active attempts
keep their original provider handle until settled; new attempts use the new adapter.

Re-evaluate providers when this milestone arrives, including regions, boot time,
network policy, persistence, logs, file APIs, limits, cleanup, and pricing. The earlier
[global sandbox survey](../subscription/sandboxes.md) is background, not a selected
provider. No need to choose a permanent runtime vendor before the contract works.

## Cache policy

Fresh workspace and no retained build cache initially. Runtime image layers may
already be cached by the host; they are not project build results or recovery state.
Later allow explicitly scoped dependency/build caches keyed by project, trust scope,
toolchain, lockfiles, and cache format. Cache hits cannot count as delivery evidence.
Use quotas and eviction; never share private build outputs across tenants. A warm
cache must not change correctness or be needed to restore a checkpoint.

## Operations and failure recovery

One instance is a single failure domain. A container can contend for resources even
when it cannot read database files. Use bounded admission and refuse jobs before disk
or memory reaches the API/database reserve. Report queueing rather than overcommit.

Take encrypted, consistent PostgreSQL backups off-instance and test restore onto a
fresh EC2 host. For private testing, daily backups are a starting policy; record the
possible data-loss window. Add a measured WAL/PITR policy before paid durable storage
claims. EBS persistence or a backup stored only on the same instance is insufficient.

On restart, recover graph jobs from acknowledged commits, match runner inventory
to known attempts, and inspect uncertain Git/Actions/script effects before retry.
An already-dispatched Actions run may continue during an EC2 outage. Poll it after
recovery. A restored database may predate external effects; reconcile those too.

Back up encrypted credential records and their recoverable key configuration. Rotate
tokens and handle GitHub/Stripe webhook replay. Monitor failed logins, denied access,
queue depth, memory, disk/WAL, backup age, missing receipts, orphan runs, and model spend.
Keep private project content out of shared logs.

Cost decisions follow measurements: EC2, EBS, backups, data transfer, model usage,
GitHub charges, and later isolated runtime charges. This replaces the earlier managed
ECS/Postgres cost estimate. Retain the existing Canadian Stripe account; choose the
currency, credit rates, and public allowances before paid launch.
