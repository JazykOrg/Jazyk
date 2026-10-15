# Files, caches, logs, and history

Status: proposed storage contract for the [runtime plan](README.md).

## Canonical input plus three output classes

The three requested classes describe newly created runtime files. Existing
documentation, source, tests, configuration, and compiler state already belong to
the canonical project. They are not accidentally discarded because no product
glob matched them.

| Class | Examples | Retention and UI |
| --- | --- | --- |
| Product | PDF, EPUB, PPTX, binary, rendered site | Durable object with a project path, visible in Files after promotion. |
| Cache | Maven dependencies, npm cache, Cargo target | Disposable, quota-bound, restored opportunistically; show hit/save status and size. |
| Scratch | Temporary extraction, sockets, intermediate files | Discarded when the finite run ends; no live browser. |

Source and test edits made by commands form a separate import diff. They can become
canonical only through normal ownership and revision gates. A lockfile such as
`Cargo.lock` or `package-lock.json` may be important source, not scratch. Its filename
alone does not decide retention. Process locks and package locks have different roles.

Logs, execution metadata, and test reports are harness-owned operational records.
They survive failed commands independently of user product/cache globs.

## Classification and extraction

Use project-relative glob patterns with documented syntax, exclusions, and case
behavior. Preview concrete matches before a configured task runs when possible.
Resolve new matches after execution. Reject overlapping product, cache, and report paths
instead of letting ordering decide. Exclusions narrow a class, never override
protected-path rules. A product glob cannot import `.jazyk` graph shards or secrets.

Apply path normalization on upload and extraction, not just at settings validation.
Reject traversal, absolute paths, device files, sockets, and escaping symlinks.
Count expanded bytes, file count, path depth, and total archive size. Initially
reject symlinks in retained outputs, or preserve only validated in-root links if a
real project needs them. Use maintained archive and filesystem libraries.

Existing canonical files cannot be overwritten by restoring a cache. Restores go
only into declared cache roots and must not overlap source, command input, report, or protected
state paths. A broad cache pattern cannot capture the whole project by accident.
Unmatched new files remain scratch. Retain a bounded excluded-path summary so the
agent can notice a missing product rule and fix the next run's definition. The
container has already shut down; undeclared contents cannot be recovered by
attaching later. Do not imply that every listed path has retained bytes.

## Input manifests and delta transfer

A frozen input manifest maps normalized paths to content digests, sizes, roles,
and bytes or object references. Materialize all relevant canonical project files,
including binary assets. Mount the selected `.jazyk` projection read-only.

Use content hashes to avoid redundant transfers of retained input objects. Every
run still materializes a fresh filesystem. The manifest records exactly what the
command could read. Source saves during a run do not mutate that snapshot. For large
retained products, the engine's filesystem
adapter must be able to load referenced bytes on demand; simply replacing file
contents with a URL would break existing reads and model inspection.

A product-producing task can reference the complete input manifest even when it
uses a small subset. Do not invent a lossy delivery projection that excludes source
files the command might need. Excluded sensitive/operational files are explicit.

## Product collection

Collect declared products at the end, after stopping their writers. Successful
commands can promote retained outputs into Files. Failed/cancelled attempts retain
logs, reports, and eligible source/test/support-file changes. The host may also
retain bounded partial products under that run, clearly marked incomplete, without
replacing the latest successful product. This policy is fixed before dispatch.

There are no intermediate product/cache checkpoints or task steps. A product built
before a later command failure is only a partial run artifact in this first model.
The agent can inspect and explicitly promote a valid retained artifact through the
normal ownership/revision gate, but command failure never silently publishes it.

Seal file digests, upload bytes, verify the manifest, then atomically update project
references with the expected revision. Outputs required by documented build/ledger
obligations must exist even after exit zero. Optional glob patterns can match no
files; the UI says no product was produced. Do not add a required/optional workflow
language just to express file collection.

Promotion creates/replaces generated-owned paths. It cannot overwrite an authored
file merely because a command used its name. Keep previous versions until the new
manifest is complete. On concurrent source edits, retain results under their run and
offer compare/promote; do not claim they are the current product.

Cancellation prevents further promotion after stop intent is accepted. An atomic
promotion completed earlier is not rolled back. Execution outcome and import status
remain separate, including a successful command with an import conflict.

## Retained file changes

Every run produces a bounded change manifest against its frozen input, covering
added, modified, and deleted eligible project files. Record before/after hashes,
role, size, content references, and import status. Eligible contents include declared
products and changes within the generation task's authorized source/test/support
scope. Newly created source files need that scope too; arbitrary new paths do not
become canonical merely because a script wrote them.

Preserve before/after text or retrievable objects for diffs. Binary changes get
metadata and retained artifact handles. Deleted files retain their before reference.
Failed runs can have useful diffs; retaining one does not apply it or mark tests
passing. Apply permitted changes only through normal ownership and revision gates.
An authored-file deletion needs its existing authorization, not just a run receipt.

Cache/scratch contents are excluded from durable diffs. Their bounded path/count/size
summaries explain exclusions. Mark quota-limited or interrupted collection as partial
and identify missing paths where known. No changes and no captured changes are
different states. Text diffs are computed from retained objects, never by shelling
into a still-running container.

During execution, logs stream. The Changes view appears after final collection. Teardown proceeds under a fixed
deadline even if the model or user never opens the view. Retain diffs under the
same default 30-day policy as logs/reports; referenced canonical files and products
have their separate lifetime. Reading history allocates no execution environment.

## Cache semantics

The task declares only `cache` globs. Restore before execution. Save an immutable
new generation only after command exit zero, with no accepted cancellation or
timeout. Failed attempts keep the prior cache. Cache storage failure is a warning;
missing cache must never prevent a correct build from scratch.

GitHub's combined cache action saves a new cache on a miss only after a successful
job. Its standalone save action can save earlier. We deliberately choose the
whole-command rule here; there is no early-save syntax in the task list.
[GitHub cache reference](https://docs.github.com/en/actions/reference/workflows-and-actions/dependency-caching),
[standalone save](https://github.com/actions/cache/blob/main/save/README.md).

The host handles the following internally, without exposed profiles or restore-key
expressions:

- Namespace by account, project, trust/credential scope, task ID/definition, image
  digest, platform, and cache globs. Keep immutable generations and select the latest
  compatible completed generation. Different tasks initially have separate caches.
- Restore only into declared cache paths. Never overwrite canonical input, compiler
  state, or test reports. Validate archives, sizes, permissions, and paths on restore.
- Stop writers before capture. Upload privately, verify digests, then publish the
  cache reference atomically. Cancellation must win before publication if accepted.
- Build tools remain responsible for invalidating stale dependency/build entries.
  A cache is untrusted acceleration, never evidence that a build or test passed.
  Toolchains unable to validate their cached outputs should use download-only cache
  or omit cache. Changes to task/image create a fresh namespace.
- Missing, corrupt, evicted, or incompatible cache means rebuild. No shared writable
  directory between sandboxes and no cross-tenant cache-presence/content leaks.
- A successful command does not certify cached executables as safe. Keep publication
  credential scopes separate from less trusted exploratory runs.

Do not expose cache sharing/composition to optimize a build-then-test chain in v1.
Each task can warm its own cache after success. Reusing caches across source changes
is useful when the build tool validates them. Explicit task/image changes invalidate
reuse; dependency manifests remain inputs the tool must respect.

Proposed initial policy: 2 GiB per project, 7 days since last access, LRU eviction
under quota. These are host/product values to measure, not compiler constants.
Show cache size and `Clear cache`, separately from deleting products. Package-manager
download caches are a useful starting point; larger installed/build trees need
measured benefit and correct toolchain invalidation.

## Durable storage layout

| Data | Local adapter | Cloud adapter |
| --- | --- | --- |
| Run and allocation state | Small durable records under `.jazyk/runtime/` | PostgreSQL rows, linked to jobs and account/project IDs. |
| Input and product manifests | Content-addressed manifests | PostgreSQL manifest references and object metadata. |
| Products and binary input objects | Project files plus retained object store | Private S3-compatible object storage. |
| Compressed logs and reports | Run files | Object storage; small indexed metadata in PostgreSQL. |
| Retained file changes | Manifests and before/after object references | PostgreSQL manifest references and private content objects. |
| Cache generations | Local disposable cache directory | Object storage or measured provider volume adapter. |
| Live scratch filesystem | Sandbox disk | Provider sandbox disk. |
| Financial receipts | No hosted charge for local execution | Existing durable billing ledger, independently retained. |

`.jazyk/runtime/` is project-wide and outside any document-set graph reset. It is
gitignored and survives an individual set archive. Deleting all local state still
deletes unexported local history; that consequence is explicit.

Cloud currently stores full snapshots in PostgreSQL. Add a binary manifest layer
incrementally rather than inflating base64 JSON with PDFs, caches, and logs.
Canonical small text can remain in the existing snapshot initially. Account quotas
count canonical bytes and retained product objects under a documented policy;
encoding overhead in the old storage path remains visible during transition.

Object keys are opaque and tenant-scoped. Access always checks project ownership;
downloads use short-lived scoped URLs. Uploads are staged, size-bounded, hashed,
and finalized by the host. Garbage collection removes orphaned uploads after a
grace period, with reference checks. Project deletion revokes active transfers,
terminates sandboxes, and schedules content deletion. Financial records follow
their separate existing lifetime without retaining project payloads unnecessarily.

## Logs readable by users and agents

Capture stdout/stderr from process start, including image preparation, dependency
restoration, commands, collection, and shutdown. Each record has run ID, lifecycle
phase, stream, timestamp, and monotonic sequence. Preserve bytes where needed and
provide a sanitized text view. Do not claim perfect ordering between separate OS
streams; sequence describes arrival at the collector.

Chunk and compress incrementally, with bounded in-memory buffers. Persist indexed
sequence/byte ranges and object digests. The viewer and tools request a tail, range,
or search query. A worker scans bounded chunks for literal or RE2-compatible regex
matches and returns line context plus a cursor. No Elasticsearch dependency is
needed initially. Expensive searches have byte/time limits and resumable cursors.

Use a read/search API, not a model shell on the API server, to grep historical logs.
Downloadable text remains possible. Redact known secrets before durable storage;
sanitize terminal escape sequences and HTML in the UI. Treat log instructions as
data. Log flooding reaches a cap, emits a truncation marker, and can terminate the
command if it threatens service budgets. Logs are bounded, not promised infinite.

If live ingestion fails, record the gap and recover provider logs where available.
Missing output is explicit. If enough evidence cannot be retained, do not declare
verified success from a guessed tail. A failed artifact upload does not erase a
known remote publication; retain that distinct outcome for recovery.

## Proposed retention

- Current canonical products stay until replaced or deleted under storage quota.
- Prior product versions stay for 30 days by default, unless explicitly pinned.
  Pinning consumes durable storage allowance.
- Logs, test reports, and retained run diffs stay for 30 days by default, with a
  visible expiry date.
- Run summaries, input digests, outcomes, and object tombstones stay for 90 days.
  Referenced publication records can outlive ordinary history.
- Caches use their shorter disposable quota and TTL. Scratch has no retention.
- Billing receipts survive operational history deletion under existing policy.

These are options for review. Expired evidence reads `Evidence expired`, not
`Never ran`. Longer retention and provider storage costs need product decisions.
