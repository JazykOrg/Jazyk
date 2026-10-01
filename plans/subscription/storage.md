# Project storage options

Recommendation: ship cloud-only projects first. Add GitHub-backed projects as a
separate choice when needed. A project has one authority for source files. Do not
build continuous two-way synchronization in the first release.

This recommendation is still a proposal. GitHub-first is reasonable for a beta
limited to developers with existing repositories. The owner asked for options and
work estimates rather than an assumed dual-storage design.

## Source authority and durable state

A project has several kinds of state:

| State | Authoritative location | Recovery requirement |
|---|---|---|
| Docs, source, project config | Cloud checkpoint or selected GitHub repository/branch | Recover the exact source revision, including unpublished work |
| Persistent graph, decrees, proposals, diagnostics, journal, generation/verification ledgers | Jazyk project state, normally `.jazyk/` | Preserve ids and human decisions; do not regenerate from prose |
| Runtime processes and toolchain caches | Sandbox | Recreate or resume; cache loss must not lose project meaning |
| Generated product databases and user uploads | Declared project data or a separately connected service | Back up explicitly if durable; mark disposable data clearly |
| Account, usage, payment, runtime leases | Cloud Postgres | Platform recovery, separate from compiler checkpoints |

GitHub-backed therefore does not mean that Jazyk stores no data. GitHub owns published
source; Jazyk retains graph state, unpublished work, and recovery checkpoints. Those
copies are not a second independently editable source repository. The UI should say
this plainly when selecting the mode.

Git alone is insufficient because `.jazyk/` is normally ignored. Compiling from empty
can create new ids and lose decrees, proposals, and history. That is a new compilation,
not a faithful restore.

## Options

| Option | User experience | Advantages | Costs and limitations |
|---|---|---|---|
| Cloud-only | Create or upload a project, autosave, restore, download | Simplest onboarding; works for non-software projects; no Git account | Jazyk owns durable history, export, retention, and recovery |
| GitHub-backed | Connect a repository, work on a branch, explicitly publish/open a PR | Existing source history, collaboration, local checkout, familiar ownership | App installation, permissions, branch conflicts, private repos, webhooks, and GitHub outages |
| Both modes, one per project | Choose Cloud or GitHub when creating the project | Serves both audiences with clear ownership | Two flows to test; no automatic mirroring between them |
| Continuous cloud/local/GitHub sync | Edit everywhere with automatic merging | Convenient once correct | Conflict resolution, offline edits, deletions, graph identity, and multiple writers make this substantially harder |
| Jazyk-hosted remote Git | Jazyk operates a Git server for every project | Standard Git access without requiring GitHub | Another stateful service, keys, permissions, backups, patches, and support; not the simple first option |

Use export/import for transfers initially. A cloud export can be put in GitHub by
the user; a GitHub archive imported as a cloud-only project has no ongoing link.
If a later conversion between modes is added, make it an explicit one-time operation
that preserves a checkpoint and changes the source authority.

## Shared checkpoint design

Start with bounded compressed archives plus a manifest in S3. Avoid building a
custom distributed filesystem or Git server. Content-addressed/delta storage can
follow if measured write amplification warrants it.

A manifest records:

- Account/project id and checkpoint id.
- Source revision or content digest, and Git base/published commit where relevant.
- Compiler version and graph format version, committed generation, and dirty source
  state. The checkpoint may include edits that have not yet been reconciled.
- Durable paths, file hashes/sizes, timestamp, parent checkpoint, and completion state.
- Optional product database dump/version and retained artifact references.

The runtime checkpoint operation coordinates the build lease, graph commit lock,
editor writes, and product writers. Freeze a consistent copy at a safe boundary,
then upload it. Uploading a tar of live-changing shards is not a consistency protocol.
Do not hold the graph lock through a slow network transfer.

Publish the checkpoint pointer only after upload completion and checksum validation,
using a transaction tied to the active project lease/fencing generation. The previous
checkpoint remains usable if the new upload fails. A stale runtime cannot replace
the current pointer. Failed uploads are collected later without deleting the last
known good version.

S3 versioning can protect against object overwrite/deletion, but all retained
versions consume storage. Configure lifecycle expiry as well as project retention;
versioning alone is neither a consistency protocol nor an infinite free backup.
Source: [S3 versioning](https://docs.aws.amazon.com/AmazonS3/latest/userguide/Versioning.html).

### Save and recovery semantics

- Autosave source changes to the runtime and display whether they are durably
  checkpointed. Reserve `Backed up` for a completed checkpoint. Do not report cloud
  durability merely because a write reached the sandbox filesystem.
- Target a durable checkpoint within 60 seconds of changes, coalesced at consistent
  boundaries, plus explicit Save, completed jobs, and before pause. If a long session
  delays the boundary, display the actual last durable time; do not claim a fixed
  recovery point that the implementation cannot meet.
- Graph-generation events request checkpoints, but do not upload a full multi-GB
  archive for every small mutation. Coalesce dirty state and cap archive size.
- A running dev database needs a consistent logical dump or quiesced snapshot. A tar
  of live database files is not a usable backup guarantee.
- On VM loss, restore the newest completed manifest into an empty workspace. Verify
  its hashes and compiler version. Reconcile source changes normally afterward.
- Clear stale process/lease artifacts through the runtime adapter. Never resurrect
  old authority tokens from a VM memory snapshot.
- Test recovery by destroying the sandbox. Resume on the same VM is not a restore test.

Target beta recovery within five minutes for a small project, subject to measured
archive size and setup time. State the measured recovery point/time in the product.
Unsaved work since the last durable checkpoint can be lost in a sudden failure.

### Files, secrets, and size

Include docs, source, the deliverable, required `.jazyk/` state, and declared durable
data. Exclude build caches and dependency trees such as `target/` and `node_modules/`
by policy, with exceptions when they actually contain declared project data. Do not
use `.gitignore` as a blanket backup exclusion: it deliberately excludes the graph.

Exclude provider credentials, temporary runtime tokens, secret env files, sockets,
and process files. Encrypt project state; traces and user-created files can still
contain sensitive content. Do not claim a filename filter detects every secret.

Use bounded archive libraries and validate paths, symlinks, expanded sizes, and file
counts. Process untrusted archives/repositories in an isolated helper. Give the
runtime only short-lived upload/download access to its own object, never bucket-wide
credentials or permission to delete previous checkpoints.

## Cloud-only mode

The cloud manifest is the authority for source and graph state. Keep one active
workspace per project. A second browser connects to it; it does not fork a competing
writer. Collaborative editing/merging is later work.

The first release needs:

- Create from an example, empty project, or bounded upload.
- Durable checkpoint status, named manual checkpoints, and restore with preview of
  the revision being restored.
- Downloadable portable archive containing source and graph, with secrets excluded.
- Storage/retention display and a working deletion/export path.

No internal remote Git server is required. Use checkpoint history initially. Do not
introduce `jazyk push` and `jazyk pull` as file-sync commands in this phase.

## GitHub-backed mode

Source of truth: one selected repository and branch. Start with one repository per
project. Submodules, LFS, and multi-repository work need explicit support later; detect
and refuse unsupported imports instead of silently omitting files.

1. Install a GitHub App for selected repositories. GitHub login alone grants no
   repository access. Bind the project to installation id, repository id, branch,
   and base commit.
2. Fetch a known commit using a trusted integration, then transfer files into the
   sandbox without its holding the GitHub App private key or an installation token.
3. Work locally in that runtime. Checkpoint graph state and unpublished source edits
   together. Show `Unpublished changes` separately from `Backed up`.
4. On explicit Publish, validate changed paths and expected base. A trusted publisher
   creates a commit and updates the intended branch without force. Use GitHub APIs
   or standard Git libraries, not a handwritten Git wire proxy.
5. If the branch moved incompatibly, preserve work and ask the user to resolve the
   conflict or publish a new branch/PR. Do not silently overwrite or merge graph YAML.
6. A later reopen fetches the source revision and its matching graph checkpoint.
   Incoming Git changes become normal dirty documents on the existing graph. Do not
   restore a graph snapshot from an unrelated branch and pretend it is current.

GitHub installation tokens can be restricted to repositories and permissions and
expire after one hour. They are not inherently scoped to one branch; the publisher
must enforce the project's branch rule. Sources:
[installation authentication](https://docs.github.com/en/apps/creating-github-apps/authenticating-with-a-github-app/authenticating-as-a-github-app-installation),
[non-forced reference updates](https://docs.github.com/en/rest/git/refs#update-a-reference).

Use minimum Contents/PR permissions; do not request organization administration or
workflow-writing access for a basic source flow. Surface unsupported workflow-file
changes rather than granting broad permissions implicitly. Handle revoked apps,
renamed/deleted repositories, branch protection, GitHub throttling, and outages.
Uninstall stops GitHub access; it must not erase unpublished work immediately.

Branch-aware graph histories and merging two independently edited `.jazyk/` trees
are outside the beta. One managed branch/workspace per project avoids that problem.
A local clone can edit source, then the cloud compiler reconciles those edits on its
existing graph after an explicit update. It cannot simultaneously write that graph.

## Cost and retention

Bill retained bytes and transfer, not only live workspace size. S3 has storage,
request, and transfer components. Use the chosen region's rates at implementation.
Source: [S3 pricing](https://aws.amazon.com/s3/pricing/).

Illustration, not a supplier quote: at US$0.025/GB-month, a 1 GB project with 20 full
retained archives uses 20 GB and costs $0.50/month in object storage. For 1,000 such
projects that is $500, before requests and traffic. Small Markdown graphs cost much
less; large deliverables and database dumps dominate. Coalescing and retention matter
more than avoiding a few kilobytes of graph state.

Proposed beta bounds, to validate before publication:

- 5 GB logical workspace per project and 20 GB total retained checkpoint/artifact
  bytes, enforced at upload admission.
- Retain the latest completed checkpoint, recent automatic checkpoints, and a small
  number of named checkpoints within that byte budget. Never prune the only recovery
  copy to make a new upload appear successful.
- Keep data for a disclosed 30-day export grace after the cloud entitlement ends.
  Warn before scheduled deletion. A model-only subscription does not fund unlimited
  inactive projects.
- Deletion removes provider snapshots, S3 object versions, and artifacts through a
  retriable job. Backup/accounting retention is documented separately.

Limits are proposals, not compiler `limits.rs` entries. They are cloud product policy.

## Effort and release options

Estimates are engineering days for one maintainer including focused failure tests.
They exclude shared authentication, billing, sandbox orchestration, and ordinary
compiler changes. The shared storage work is required even for GitHub-backed mode.

| Work | Effort | Includes |
|---|---:|---|
| Shared checkpoint and cloud-only flow | 6–10 days | Consistent export, manifests, S3, restore, bounds, deletion, download |
| GitHub integration on that foundation | +8–15 days | App installation, import/publish/PR, branch conflicts, revocation, matching state |
| Expose and support both modes together | +2–4 days | Mode selection, clear save labels, creation/export flows, compatibility tests |
| Continuous two-way sync beyond those modes | +20–40 days, then ongoing work | Local client, offline edits, conflicts, deletions, authority transfer, graph semantics |
| Operate a hosted Git service instead | +10–20 days initially, then operations | Deployment, identity/keys, integration, backup/restore, upgrades, monitoring |

Release options:

- Cloud-only first: 6–10 storage days. Lowest integration work and widest audience.
  Recommended for the first beta.
- GitHub-first: 14–25 storage/integration days. Better if every early user already
  has a repository. Still needs graph and unpublished-work checkpoints.
- Both alternative modes at launch: 16–29 days. More complete, but delays the beta.
- Automatic mirroring/sync: defer. It is not necessary to offer either storage choice.

The user's choice can be `Store in Jazyk` or `Connect GitHub`. The internal recovery
copy is an implementation detail with an honest retention/export explanation, not
an extra storage mode the user has to manage.
