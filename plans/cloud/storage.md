# PostgreSQL and project storage

Status: proposed, 2026-10-03. PostgreSQL is installed on the same EC2 instance as the
API. It stores cloud state and operational records. It does not replace local graph
authority for local-backed projects.

## Start with one durable database

Use PostgreSQL for identities, accounts, credits, projects, revision manifests,
serialized cloud graph state, jobs, delivery checkpoints, and retained evidence.
Store bounded immutable file/blob chunks in PostgreSQL initially. Put these behind
a blob-store interface so larger artifacts can move to object storage later.

This is the recommended simplicity tradeoff. It gives the first implementation
one transactional backup and restore boundary. It increases database size, WAL,
and backup cost. Set project, artifact, log, and upload limits before accepting work;
do not stream unbounded CI artifacts or dependency caches into PostgreSQL. Execution
scratch and build environments belong to GitHub-hosted runners, not the EC2 instance.

An alternative is PostgreSQL manifests plus immutable EBS files from day one. That
reduces database bulk but adds ordering between file durability, SQL references,
garbage collection, and coordinated backup. Defer it unless measured fixtures do
not fit the bounded database design. S3 is initially an off-instance backup target,
not a mandatory live project filesystem.

Keep YAML graph serialization and journals behind the state-store interface. Do
not rewrite every graph node as a relational entity to launch Cloud. The compiler
owns semantic validation; the SQL adapter owns durable transactions and version checks.

## Logical records

These are record groups, not final SQL table declarations. Tenant-owned keys and
foreign keys include tenant identity; project-owned records also include project id.

| Records | Minimum responsibilities |
|---|---|
| Accounts, identities, sessions, devices | GitHub stable user id, one account owner, revocable sessions and connector binding |
| Projects, memberships, storage bindings | Authorization, local/cloud/GitHub authority, compiler compatibility |
| Repositories, installations, secret references | Authorized remote identity, repository grant, encrypted credential references |
| Workspace revisions, entries, blobs | Immutable bytes and manifests, expected-head writes, tenant-scoped content addressing |
| Graph generations, project heads, commit receipts | Graph/status/ledger/journal references, source revision, changeset id, writer epoch |
| Jobs, leases, events, outbox | Admission, durable queue, sequencing, restart recovery, external action intent |
| Grants, reservations, usage, settlement receipts | Atomic credit admission and idempotent settlement, separate funding and use |
| Destinations, bindings, definition revisions | Stable delivery identity, config digest, remote identity and managed scope |
| Candidates, observations, plans, attempts, checkpoints | Frozen inputs, prepared effects, partial/unknown outcomes, last confirmed delivery |
| Execution files, reports, artifacts, retention roots | Attempt-scoped outputs and evidence, expiry, recovery references |
| Verification observations and imports | LLM or CI origin, criterion/test identity, input hashes, provider attempt, idempotent ledger receipt |

`jazyk.toml` is the source of delivery definitions. SQL keeps a validated index and
immutable revisions used by attempts. It is not a separately editable configuration.
A definition edit commits the workspace first; indexing is rebuildable and may be
replayed. Running an attempt revalidates the pinned definition revision.

## Virtual workspace transactions

The portable contracts are workspace reads at a revision, staged path mutations,
state loading, and an atomic commit coordinator. Logical paths are project-relative.
Reject traversal and unsupported links; never interpret a tenant string as a host path.

A cloud commit:

1. Read a pinned source revision and graph generation into the scoped context.
2. Stage new immutable content and run the compiler's existing gates.
3. In a short SQL transaction, validate the writer epoch and expected heads.
4. Insert the new generation, journal, relevant source changes, and unique receipt.
5. Advance the project head and record the event/outbox entry in that transaction.
6. Publish progress after commit. A lost acknowledgement retrieves the same receipt.

No SQL transaction or connection stays locked during an LLM call or delivery run.
Source edits racing a build invalidate its assumptions instead of being overwritten.
Read-only views can use the last committed generation while a build is active.

Acquire jobs using a short locked claim and expiring lease. `FOR UPDATE SKIP LOCKED`
is suitable for queue consumption, not general consistency reads. Persist retry,
owner epoch, and attempt identity independently of the claim transaction.
[PostgreSQL SELECT documentation](https://www.postgresql.org/docs/current/sql-select.html).

The outbox does not make external effects exactly once. Persist intent before Git
updates and workflow dispatch. On restart inspect uncertain
outcomes before retry. Use provider receipts and action-specific deduplication.

## Local-backed projects

The settled rule remains: source, graph, and human decisions are authoritative in
the local folder. The connector acquires the local build lease, pins input bytes,
and validates/commits returned changesets. The service reports saved only after
the durable local receipt. PostgreSQL holds cloud job/account metadata and receipt
indexes, not an undisclosed durable replica of the local project.

On connector loss, stop new compilation calls and commits. A short bounded reconnect
buffer is a policy to specify; five minutes remains a proposed default. A late result
cannot commit after a new local writer epoch. Returning to local compilation is
explicit and preserves the graph rather than rebuilding it from prose.

Delivery is a separate, disclosed transfer of a frozen candidate. Git/Actions sends
the selected files to the selected repository. It does not change the project's
storage authority. Source publication for CI requires the repository grant even
when source and graph normally remain local.

Before starting external work, save the local candidate, prepared plan, and delivery
intent in a separate local operational store. After dispatch, the cloud service
owns the hosted attempt's operational record; the connector mirrors receipts and
evidence when online. Losing the connector need not kill an already admitted delivery
whose complete candidate is staged. Its UI can say `Executed remotely; local receipt
pending`, never that the local project was saved.

Durable candidate recovery content remains in the local delivery store or the
explicitly chosen Git destination. Retained CI artifacts/reports use the disclosed
delivery evidence policy; they do not become a second graph authority. Mirror verified
results through the connector before reporting them saved to the local ledger. Longer
cloud source retention is not implied by remote compilation. If required evidence is
unavailable, confirmation remains pending.

## Delivery memory has a separate lifetime

Recommended local location: `.jazyk-delivery/`, separate from resettable `.jazyk/`.
It contains stable destination bindings, immutable candidates/checkpoints, attempt
receipts, and retained evidence. Exclude it from compilation globs and ordinary
publication by default. Provide deliberate backup/export instead of automatically
publishing snapshots, private traces, or operational records to Git.

For cloud-backed projects, delivery records and retained blobs live in PostgreSQL.
The state-store reset/version rule cannot cascade-delete them. Normal SQL migrations
apply to account/delivery tables; the compiler's no-migration policy applies to its
graph format. Incompatible graph snapshots stay recoverable and cannot silently be
recompiled while claiming to preserve ids or human decisions.

Retain content referenced by a current checkpoint, active/uncertain attempt, support
commitment, or recovery policy. Historical success records can expire once none of
these references need them. A digest or expired provider URL cannot recover bytes.

Accountless local deliveries use the same operational contract and existing local
Git credentials. Cloud-managed attempts require cloud connectivity to reconcile
their current state. An offline client does not assume an unresolved remote attempt
failed and repeat its external effects. Switching storage explicitly transfers the
graph and delivery memory, including unresolved attempt references.

## GitHub-backed source and graph identity

Treat GitHub as an adapter for versioned source, not as a POSIX filesystem. Read
trees/blobs at a commit and stage edits in a private overlay. Saving an edit need
not push a commit. Publication happens through an explicit delivery.

The default graph remains a cloud checkpoint. Git can preserve immutable ids, human
decrees, redirects, and journals if their canonical serialized state is deliberately
committed. Source-only commits cannot preserve facts that are absent from their tree.
Offer an explicit portable graph export/checkpoint later; diagrams alone are not one.

A repository linked only as a delivery destination does not make it the source
authority. The UI distinguishes `Files: Cloud` from `Deliver to: owner/repo`.
If it is also the source adapter, update the overlay base after confirmed publication
only against the expected workspace revision. Newer edits remain a separate draft.

## Authorization and accounting

Every read, write, subscription, run-file request, and artifact download checks the
project capability. Use tenant/project keys throughout SQL and storage handles.
Do not offer global blob-existence lookup or infer a project from a node id.

Add row-level policies as defense in depth. Use a non-owner runtime role without
superuser or `BYPASSRLS`; set context transaction-locally on pooled connections.
Migration and maintenance roles are separate. PostgreSQL owners normally bypass RLS,
and constraint checks require care when returning errors.
[PostgreSQL row security](https://www.postgresql.org/docs/current/ddl-rowsecurity.html).

Keep Stripe event deduplication, paid grants, atomic reservations, settlement, and
reconciliation from the subscription plan. One account's concurrent projects share
the same balance. Model usage, any declared orchestration charge, and GitHub-billed
Actions usage are distinct. There are no Jazyk container-minute charges in this scope.
Test grants are explicitly nonmonetary. Reading/exporting saved
work and delivery receipts must not require a new model purchase.
