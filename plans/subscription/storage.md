# Versioned project storage

Revised 2026-10-02. Storage is independent of compiler location. The shared compiler
operates on logical project files and persistent graph state through injected
interfaces. No cloud compilation step requires a checkout or mounted filesystem.

## Contracts

Evolve the existing [VFS seam](../../bootstrap/src/vfs.rs) into per-project instances.
Do not select storage using a process-global variable or thread-local tenant. Keep
logical storage and execution as separate capabilities.

| Contract | Required behavior |
|---|---|
| Workspace | Enumerate project paths, read bytes/metadata at a pinned revision, stage writes/deletes/renames, publish a batch against an expected revision |
| State store | Load committed graph/status/ledger, atomically commit a validated generation, retain journal and recovery history |
| Project commit coordinator | Associate source revision, graph generation, changeset id, and writer epoch; return an idempotent durable receipt |
| Change feed | Bounded revision events, resume cursor, explicit rescan when history is unavailable |
| Artifact store | Scoped rendered outputs, exports, traces, execution evidence and retention |
| Execution environment | Optional materialization and shell/file tools, never inferred from the presence of a path |

Names are illustrative. Exact Rust traits and wire schemas belong in the specs.
A small in-memory implementation is a first-class test backend on native builds.

Use normalized project-relative paths internally. Reject root escape, absolute host
paths, encoded traversal, and ambiguous case/Unicode collisions. Resolve local symlinks
within the selected root using platform libraries and protect against substitution
between checking and opening. Exclude socket/device files. Virtual paths are not host
filesystem permissions. Preserve raw bytes where appropriate; source decoding is explicit.

Config discovery and document globs run against the selected workspace. A hosted
project cannot walk up into a server home directory, import another project's files,
read host environment variables, or configure an arbitrary executable.

Adapters declare capabilities: atomic batch publish, append, notifications, offline
availability, binary support, and limits. Use snapshots and overlays when a backend
cannot provide native rename/append semantics. Do not pretend GitHub implements POSIX.

## Source and graph commits

Maintain a manifest identifying source revision, graph generation, compiler/state
version, parent manifest, changeset id, dirty source state, and durable artifact refs.
A source edit can be newer than the graph. Record that fact and reconcile normally;
never present an older graph as already reflecting the newer source.

For object-backed projects:

1. Read a committed manifest and acquire the project's writer epoch.
2. Stage source and graph mutations using the existing gates and natural keys.
3. Upload immutable, tenant-scoped changed objects and validate their hashes.
4. In a database transaction, compare the expected head and writer epoch, then publish
   the new manifest/head and commit receipt. The pointer is the visibility boundary.
5. On failure, leave the old head readable. Collect unreachable staged objects later.

Do not hold a database transaction while waiting for a model or a long upload. A
retry with the same changeset id returns the original receipt. Readers never see
half the new shards. In-memory caching does not count as a durable commit.

The local adapter retains `.jazyk/` interoperability and implements the same logical
commit contract with locking, a recovery journal, and deterministic crash recovery.
Source plus graph updates need this coordinator too; ordinary writes of several
files are not an atomic transaction. Uncoordinated editors may change source, which
must be detected by expected versions/hashes before replacement.

Preserve the compiler's no-migration policy. A remote restore must check compatibility
before opening state. Keep the original checkpoint and explain an incompatible version;
never silently archive it and claim that recompilation preserved identities.

## Local-local

A native local Jazyk server opens one or several authorized folders. Source, persistent
graph, ledgers, and artifacts remain in those folders. Local filesystem events become
the same revision feed used by other adapters. Existing CLI commands can use an
in-process local server context or attach to an explicitly selected local server.

No account is required. Existing local agents and provider settings remain effective.
Logical project contexts still matter: one local server may host several projects
with different agents, settings, and models.

## Local-remote

Recommended default: the computer owns both source and durable graph state. A small
native connector provides the workspace and commit endpoints over an outbound,
authenticated connection. The shared remote server runs the compiler.

1. Select the folder and disclose remote processing. Register its logical project id,
   authorized device, storage binding, and compiler selection with the service.
2. The connector acquires the local build lease, reports the committed graph generation,
   and pins source content at a known revision. Use bounded snapshots or content hashes
   and immutable local cached blobs so later reads cannot return mixed revisions.
3. Transfer the required manifest, graph, and requested source bytes. Prefetch small
   documents in batches. Do not expose an arbitrary filesystem RPC or inbound laptop port.
4. The remote compiler stages a changeset and asks the connector to commit with an
   expected revision/generation, changeset id, and active authority epoch.
5. The connector verifies the lease and actual source state, runs deterministic commit
   validation, writes through the local coordinator, and returns a durable receipt.
6. The remote worker advances only after the receipt. On a lost acknowledgement it
   queries that changeset id instead of blindly writing or charging again.

Remote execution of Jazyk still sends source/graph context off the computer. The UI
must not say that data never leaves. Proposed initial retention: job/account metadata
is durable; project bodies and uncommitted changes use bounded session memory only,
with no durable remote project copy. Document provider-side retention separately.

On connector loss, stop new model calls and commits. Retain bounded in-memory pending
results for a proposed five-minute reconnect grace, then discard them. A service crash
can lose uncommitted work. The local committed graph remains recoverable. Usage already
incurred is still usage; disclose this and avoid automatic charged retries. A durable
remote recovery spool is a separate later opt-in, not an undisclosed backup.

The connector may outlive the browser tab. Closing jazyk.dev does not itself drop the
folder connection if the native connector remains running. If the computer sleeps,
the server cannot promise unattended local-backed work. Cloud-backed source is the
option for work that must continue without the device.

Local LSP reads the last committed graph while disconnected. Local recompilation is
available after explicitly releasing the remote lease and selecting the local compiler.
Use an authority epoch so a late remote result cannot overwrite new local work. Do not
silently switch compiler location or provider billing during a failed run.

## Remote-remote with cloud storage

Use immutable source/graph objects and versioned manifests in object storage, with
transactional heads in Postgres. Keep the current YAML graph representation initially.
The service reconstructs a logical workspace in memory or streams through an adapter;
it does not untar a project into a shared host directory.

The browser and IDE adapters read and edit this workspace through the project API.
A job can continue after the client disconnects. Two clients share one project head
and serialized compiler writer, with optimistic document edits. Collaborative text
merging is outside the first release. Conflicts must preserve the rejected edit.

A save is durable only after its manifest is published. Display source save status
and graph revision separately. Keep exports containing source, graph, decrees,
proposals, diagnostics, journal, and generation/verification ledgers.

Start with bounded per-account/project storage, named checkpoints, disclosed retention,
and deletion/export. The earlier 5 GB project, 20 GB retained-data, and 30-day export-grace
figures remain proposals to cost, not new commitments. Retained versions and transfer
consume resources even when no compilation job is running.

## GitHub as a workspace adapter

GitHub is a remote source backend. Bind one installation, repository, branch, and
known commit. Read trees/blobs through supported GitHub APIs or libraries, without a
native checkout on the compiler host. Detect truncated listings, large files, LFS,
and submodules. Fail explicitly for unsupported content instead of silently omitting it.

Use a private versioned overlay for draft edits. The workspace revision is the base
commit plus overlay revision; graph checkpoints identify that exact pair. A publish
creates a commit/tree and updates the intended branch without force. A moved branch
requires an explicit update/conflict decision or a new branch/PR. The GitHub repository
owns published source; the overlay owns unpublished work. They are not independently
editable mirrors of the same revision.

GitHub credentials stay in the trusted integration. Login with GitHub does not grant
repository access. App installation and minimum Contents/PR permissions are separate.
Revocation stops remote reads/publish while preserving already durable unpublished work
for the disclosed retention period. The first adapter supports one branch per workspace.

There is no atomic transaction spanning GitHub and Jazyk's database. Journal publish
intent, expected base, resulting tree/commit, and graph checkpoint before updating a
ref. After timeout, read the remote ref/commit and reconcile the receipt. Never make a
second commit merely because the first acknowledgement was lost. A concurrent change
that cannot be proven to match the intent becomes a conflict.

Sources: [Git trees](https://docs.github.com/en/rest/git/trees),
[Git references](https://docs.github.com/en/rest/git/refs), and
[installation authentication](https://docs.github.com/en/apps/creating-github-apps/authenticating-with-a-github-app/authenticating-as-a-github-app-installation).

### GitHub can store the graph

Git can preserve identities and decisions. `.jazyk/` is ignored by default, so a
source-only checkout cannot restore graph state it never stored. Two valid policies:

- Source in Git, graph and unpublished work in matching Jazyk checkpoints. Recommended
  initial managed default.
- Source plus a consistent durable graph checkpoint in Git. Include the required
  status/journal/ledger state, and exclude credentials, locks, caches, and transient
  process files. Restore with a compatible compiler.

The latter makes graph history portable through Git but adds larger commits and
sensitive project metadata. Ordinary YAML merges cannot validate divergent semantic
histories. Start with linear single-writer snapshots; do not promise graph merging.

## Transfers and execution copies

Changing the compiler between local and remote does not change source authority.
Changing storage is an explicit export/import or authority handoff, preserving the
persistent graph and checking format compatibility. Pause the writer, obtain a
consistent checkpoint, import and verify, then activate the destination. Keep the
original as an independent copy until the user chooses to remove it.

A container materialization is an execution copy pinned to a workspace revision, not
new source authority. Its accepted changes return through the same conditional commit
contract. Never synchronize an entire container root back to a project.

Use bounded archive libraries for exports/imports. Exclude credential material and
validate paths, symlinks, sizes, and file counts. A filename filter is not a guarantee
that arbitrary user files contain no secrets.

## Effort and release options

These ranges are included in the [delivery phases](./README.md#delivery-sequence),
not additive estimates. Shared contracts precede provider adapters.

| Increment | Planning effort |
|---|---:|
| Per-instance local/memory interfaces and compiler context work | 8–14 days |
| Local-remote snapshots, conditional commit, reconnect and protocol bridge | 8–15 days |
| Cloud object-backed state and basic project UI | 8–12 days |
| GitHub read/overlay/publish adapter after the shared contracts | 6–10 days |
| Two-way live local/cloud sync or divergent graph merge | Deferred; requires a separate design |

The first useful slice is concurrent local/memory-backed compilation. GitHub and
container-provider selection are not prerequisites for proving that contract.
