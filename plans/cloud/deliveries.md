# Named deliveries and model tools

Status: proposed, 2026-10-03. This makes the [delivery design](../delivery/README.md)
concrete for Jazyk Cloud. Destinations and preservation rules remain ordinary prose
requirements. Configuration selects operational mechanisms.

## Two kinds, three execution paths

- `git`: publish a selected file set to a Git remote. GitHub uses its API adapter.
  No project container and no code execution are necessary.
- `script`: execute one project entrypoint. Select either a fresh container or
  GitHub Actions as the runner. Both can invoke the same script.

Use these two kinds rather than a separate product model for CI. The UI can present
three templates: Publish to Git, Run on GitHub Actions, Run in a container.
The host controls which container provider implements `container`; the project
does not choose an EC2 instance or database credential.

## Proposed TOML

All names and fields below require a documented schema before implementation.
Stable ids survive display-name changes. References identify bindings, not secrets.

```toml
[deliveries.publish]
id = "d-publish"
name = "Publish source"
kind = "git"
destination = "repo-source"
remote = "origin"
ref = "refs/heads/main"
include = ["src/**", "tests/**", "docs/**", "jazyk.toml", "scripts/**", ".github/workflows/**", "package.json", "package-lock.json"]

[deliveries.verify]
id = "d-verify"
name = "Build and test on GitHub"
kind = "script"
destination = "ci-evidence"
runner = "github-actions"
environment = "node-build"
repository = "origin"
workflow = ".github/workflows/jazyk-verify.yml"
entrypoint = ["bash", "scripts/deliver.sh"]
include = ["src/**", "tests/**", "scripts/**", "package.json", "package-lock.json"]
timeout_seconds = 900
reports = ["reports/junit.xml"]
artifacts = ["dist/**", "reports/**"]

[deliveries.preview]
id = "d-preview"
name = "Build and test in a container"
kind = "script"
destination = "preview-artifact"
runner = "container"
environment = "node-build"
entrypoint = ["bash", "scripts/deliver.sh"]
include = ["src/**", "tests/**", "scripts/**", "package.json", "package-lock.json"]
timeout_seconds = 900
reports = ["reports/junit.xml"]
artifacts = ["dist/**", "reports/**"]
```

An entrypoint is an argv array, resolved inside the pinned workspace. Inline shell
interpolation is not the configuration language. Shell scripts are ordinary versioned
files when needed. `environment` selects an approved image/toolchain profile. Resolve
container images to a digest and record Actions runner/tool versions when preparing
and running the attempt. `reports` and `artifacts` declare outputs. The full source
candidate also retains config, workflow, script, and dependency files
needed for reproducibility. Validate that include rules do not omit required files.

`destination` resolves a stable binding with actual repository/ref, publication,
artifact scope, and optional graph entities. `remote`/`repository` can use a local
Git remote name or a validated project binding. For cloud-only projects, bindings
are stored through project settings, with server-side resolved remote identities.
Credential names can be referenced through a separate secret-binding map. Secret
values never enter TOML, graph facts, model context, or exported candidates.

Keep build/test-only deliveries useful. Their destination is an artifact or evidence
target and their checkpoint certifies that scope. They do not claim a product was
deployed. A deployment script additionally needs external checks defined by its
destination and obligations.

## Editing configuration

The model can list, add, modify, rename, and delete deliveries through structured
tools. Use a lossless TOML library so comments and unrelated settings survive.
Updates carry the expected workspace/config revision. Validate the entire edited
definition, apply a normal workspace changeset, and refresh the indexed configuration.
Local-remote edits require the connector's durable commit acknowledgement.

Creating a GitHub Actions delivery can stage both the TOML definition and generated
workflow/script files in one workspace edit. Show their diff. Saving those files
does not publish or dispatch them. Templates must not guess deployment credentials
or add unintended push-triggered publication.

Deleting a definition prevents new attempts. It does not erase checkpoint history,
cancel a running attempt, delete a repository workflow, or undo a publication.
These are separate actions. Retargeting a remote increments the binding revision
and requires observation/adoption; it cannot reuse the old destination's baseline.

## Tools and authority

Illustrative tools, available through scoped MCP and ACP sessions:

| Tool | Effect |
|---|---|
| `deliveries.list/get` | Return definitions, capabilities, last confirmed result, and active attempt |
| `deliveries.add/update/delete` | Apply validated revisioned changes to `jazyk.toml` |
| `delivery.prepare` | Freeze candidate, inspect destination, derive transition and required evidence |
| `delivery.run` | Admit a prepared plan and return an attempt id |
| `delivery.status/watch` | Read state, logs, progress cursor, costs, and evidence availability |
| `delivery.cancel` | Request cancellation; reconcile already completed external effects |
| `delivery.files.list/read` | Read the attempt's materialized or retained files with ranges and limits |
| `delivery.exec` | Run a bounded follow-up command in a live container under that attempt's policy |
| `delivery.diff/import` | Preview and import selected run-file changes against expected workspace versions |

Tools cannot choose another tenant, arbitrary host paths, a different remote binding,
or stronger credentials. Existing project editing authority covers configuration
edits. Execution uses the user's current request or standing delivery policy, scoped
to destination, pinned definition/plan, resource limit, and external effects. An
edit to TOML cannot expand that policy. Ask for review only when an effect exceeds
existing authority, after a concrete plan exists.

Do not make automatic compiler batches publish simply because delivery tools exist.
Expose execution tools in explicitly requested delivery sessions. The harness owns
readiness, gates, and the worklist. The model proposes procedures and findings; it
does not create or prioritize compiler board goals.

## Candidate and transition

1. Select a delivery and saved workspace revision. Unsaved IDE buffers must be saved
   or explicitly included first.
2. Freeze exact source/artifact bytes, config, scripts, workflow, toolchain identity,
   graph snapshot, and verification evidence. Start with a full relevant project
   graph snapshot; optimize to dependency closures later.
3. Load the destination's last confirmed checkpoint and current observations. A
   missing permission or failed read is not evidence of an empty destination.
4. Compare candidate, checkpoint, observation, and preservation obligations. Prepare
   guarded actions, expected results, recovery boundaries, and resource limits.
5. Return missing compatibility behavior to the normal requirements/generation
   process. Its changed candidate invalidates the affected delivery plan.
6. Persist the admitted plan and action intent before external work starts.

A direct Git push can be a single guarded action plus verification. A script can
implement several steps and emit progress/receipts, but an opaque script gets only
coarse interruption recovery. Do not promise automatic rollback for arbitrary scripts.

## Attempts and checkpoints

```text
prepared → queued → running → verifying → confirmed
                         ↘ failed / cancelled / outcome-unknown
```

Infrastructure completion, test verification, and confirmed delivery remain separate
fields. A queued job is not a compiler goal. A successful process without required
evidence is `verification incomplete`, not a confirmed delivery.

Persist action ids, provider run ids, candidate/config digests, destination revisions,
resource observations, event cursors, and receipts. Revalidate relevant guards before
each external action. Serialize Jazyk attempts for the same managed destination;
provider guards still handle writers outside Jazyk. Detect duplicate bindings within
the service. Cross-machine accountless coordination needs provider guards or a shared
destination record; a local lock alone cannot coordinate unrelated clients.

Git commits and Actions dispatch are not in the graph transaction. A crash after
external success but before saving a receipt produces an unknown outcome. Inspect
the ref or run before retry. Do not blindly replay a deployment or migration script.
Cancellation likewise does not undo a push or reverse already applied external work.

Confirm only after evidence matches the candidate and destination obligations. Keep
the previous confirmed checkpoint while an attempt is partial or uncertain. A
checkpoint contains recoverable inputs and evidence, not merely a hash or graph
generation number. Local and cloud storage follow [the separate lifetime](./storage.md#delivery-memory-has-a-separate-lifetime).

## Run files and test evidence

Virtual project files and execution files are distinct views. Every run-file request
names an attempt id and base revision. A running container can expose bounded reads;
the response identifies that the file may still be changing. Retained output files
have immutable digests after capture. Generated changes never silently overwrite the
project; import checks its current revision.

Container follow-up commands stay inside the delivery attempt. Mutating the candidate
or procedure invalidates affected evidence and creates a new candidate/attempt before
claiming it is verified. Do not turn this into an untracked permanent shell session.

Actions exposes the pinned repository candidate plus uploaded outputs. It has no
general interactive file or command tools on its hosted runner. Show unsupported
capabilities explicitly. See [GitHub evidence](./github.md#actions-evidence-and-file-access).

Collect structured reports, initially JUnit XML with optional Jazyk criterion-id
mapping, plus raw logs. Record run/attempt id, candidate digest, commit, toolchain,
test identity, status, and report digest. Only explicitly mapped criteria receive
verification evidence. Unknown, missing, skipped, expired, or stale results do not
become passes. Parse untrusted reports with size limits and external entities disabled.

Import matching evidence into the existing generation verification ledger through
its normal writer/gates. A newer project revision can retain historical evidence
without treating it as current. Provider success and arbitrary text in a log never
prove every criterion or external preservation obligation.
