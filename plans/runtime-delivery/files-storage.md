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
| Cache | Maven dependencies, npm cache, Cargo target | Disposable, quota-bound, restored opportunistically; browse under the run or cache inspector. |
| Scratch | Temporary extraction, sockets, intermediate files | Dies with the sandbox; optionally inspect while live. |

Source and test edits made by commands form a separate import diff. They can become
canonical only through normal ownership and revision gates. A lockfile such as
`Cargo.lock` or `package-lock.json` may be important source, not scratch. Its filename
alone does not decide retention. Process locks and package locks have different roles.

Logs, execution metadata, and test reports are harness-owned operational records.
They survive failed commands independently of user product/cache globs.

## Classification and extraction

Use project-relative glob patterns with documented syntax, exclusions, and case
behavior. Preview concrete matches before a configured action runs when possible.
Resolve new matches after execution. Reject overlapping product and cache paths
instead of letting ordering decide. Exclusions narrow a class, never override
protected-path rules. A product glob cannot import `.jazyk` graph shards or secrets.

Apply path normalization on upload and extraction, not just at settings validation.
Reject traversal, absolute paths, device files, sockets, and escaping symlinks.
Count expanded bytes, file count, path depth, and total archive size. Initially
reject symlinks in retained outputs, or preserve only validated in-root links if a
real project needs them. Use maintained archive and filesystem libraries.

Existing canonical files cannot be overwritten by restoring a cache. Restores go
only into declared cache roots and must not overlap source, script, or protected
state paths. A broad cache pattern cannot capture the whole project by accident.
Unmatched new files remain scratch. Report them in the run diff so the agent can
notice a missing product rule before terminating.

## Input manifests and delta transfer

A frozen input manifest maps normalized paths to content digests, sizes, roles,
and bytes or object references. Materialize all relevant canonical project files,
including binary assets. Mount the selected `.jazyk` projection read-only.

Use content hashes to transfer only changed files after the initial upload. The
manifest records exactly what the command could read. Source saves during a run
do not mutate that snapshot. For large retained products, the engine's filesystem
adapter must be able to load referenced bytes on demand; simply replacing file
contents with a URL would break existing reads and model inspection.

A product-only delivery can reference the complete input manifest even when it
uses a small subset. Do not invent a lossy delivery projection that excludes source
files the script might need. Excluded sensitive/operational files are explicit.

## Product checkpoints

Default: collect declared products after a successful action. On failure, retain
logs and reports, with a bounded option to retain partial products as failed-run
artifacts. Never overwrite the latest successful product with a partial build.

Collection happens at a quiescent command or step boundary. Seal file digests,
upload bytes, verify the object manifest, then commit the project references with
the expected workspace revision. A product required by the build but absent after
exit zero is a failed build. Optional glob matches can be empty only when declared
optional; the UI shows that no product was produced.

Promotion into Files is an atomic metadata update after uploads complete. It
creates or replaces generated-owned paths and advances a workspace revision. It
does not overwrite authored files merely because the output used the same name.
Keep the previous version until the new manifest is complete. If source changed,
keep the result under its run and offer a compare/promote operation; do not call it
the current product.

Individual projects can request checkpoints after earlier steps. A PDF built before
an upload failure can remain a valid local product while external delivery failed.
The UI presents those two outcomes separately.

## Cache semantics

GitHub's combined cache action saves a new cache on a cache miss only if the job
completes successfully. Existing keys are immutable. Keys and ordered restore keys
control reuse. Its documented retention includes eviction of unused entries.
[GitHub cache reference](https://docs.github.com/en/actions/reference/workflows-and-actions/dependency-caching).
GitHub also provides a standalone save action, allowing a checkpoint before a later
test step. [Save action](https://github.com/actions/cache/blob/main/save/README.md).

Adopt those ideas without requiring an Actions YAML file:

- Namespace by account, project, execution trust scope, runtime image/platform,
  cache profile version, and hashes of declared key files.
- Default to exact restore. Optional compatible restore may relax selected
  dependency-key components, never tenant, trust scope, OS, architecture, or image
  compatibility. The build tool must validate reused material.
- Publish an immutable cache generation only at a declared successful checkpoint.
  Upload to a new object, then atomically make that generation discoverable.
- A restored exact-key dependency cache normally needs no rewrite. For evolving
  build caches, append a generation under the compatibility key and choose the
  latest successfully sealed generation. Keep this distinction explicit.
- Snapshot when the relevant writer is stopped or paused. Do not archive a `target`
  directory while a background compiler is changing it.
- Missing, corrupt, evicted, or incompatible cache means rebuild. A cache never
  becomes the sole source of a required product or verification verdict.
- No shared writable cache between sandboxes. Concurrent attempts use private
  restores and separate generations. No cross-tenant deduplication that exposes
  cache presence or contents.
- Keep publish-credential scopes separate from less trusted exploratory caches.
  Cached executable code can run later. A successful exit does not certify it safe.

Proposed initial policy: up to 2 GiB cache per project, 7 days since last access,
LRU eviction when the quota is reached. These are adjustable launch values, not
compiler constants. Larger plans may differ. Show cache size and offer `Clear cache`.
Artifact deletion and cache clearing are separate actions.

Prefer package-manager download caches over copying every installed dependency
tree. `node_modules` and build output caches can still be useful when keyed by the
right platform/toolchain inputs. Let generation choose with evidence from timings.

## Durable storage layout

| Data | Local adapter | Cloud adapter |
| --- | --- | --- |
| Run, step, command state | Small durable records under `.jazyk/runtime/` | PostgreSQL rows, linked to jobs and account/project IDs. |
| Input and product manifests | Content-addressed manifests | PostgreSQL manifest references and object metadata. |
| Products and binary input objects | Project files plus retained object store | Private S3-compatible object storage. |
| Compressed logs and reports | Run files | Object storage; small indexed metadata in PostgreSQL. |
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
restoration, commands, collection, and shutdown. Each record has run/command/step
IDs, stream, timestamp, and monotonic sequence. Preserve bytes where needed and
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
- Logs and test reports stay for 30 days by default, with a visible expiry date.
- Run summaries, input digests, outcomes, and object tombstones stay for 90 days.
  Referenced publication checkpoints can outlive ordinary history.
- Caches use their shorter disposable quota and TTL. Scratch has no retention.
- Billing receipts survive operational history deletion under existing policy.

These are options for review. Expired evidence reads `Evidence expired`, not
`Never ran`. Longer retention and provider storage costs need product decisions.
