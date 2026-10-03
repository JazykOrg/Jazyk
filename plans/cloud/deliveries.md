# Named deliveries and model tools

Status: proposed, 2026-10-03. This makes the [delivery design](../delivery/README.md)
concrete for Jazyk Cloud. Destinations and preservation rules remain ordinary prose
requirements. Configuration selects operational mechanisms.

## Two kinds, GitHub-hosted execution

- `git`: publish a selected file set to a Git remote. GitHub uses its API adapter.
  No project container and no code execution are necessary.
- `script`: run a project command such as `npm test`, `python -m pytest`, or
  `bash scripts/deliver.sh`. Jazyk generates a GitHub Actions workflow for it.

The UI offers Publish to Git and Run a command on GitHub. There is no backend
selector in the first version. Jazyk-hosted runtimes and containers are deferred.
LLM-run tests remain in Jazyk verification sessions; see [testing](./testing.md).

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
name = "Run code tests on GitHub"
kind = "script"
destination = "ci-evidence"
environment = "node-build"
repository = "origin"
workflow = ".github/workflows/jazyk-verify.yml"
command = "npm test"
shell = "bash"
cwd = "."
setup = ["npm ci"]
include = ["src/**", "tests/**", "scripts/**", "package.json", "package-lock.json"]
timeout_seconds = 900
reports = ["reports/junit.xml"]
artifacts = ["reports/**"]
```

`command` is executed by the declared shell on a GitHub-hosted runner, in `cwd`
relative to the pinned project. `setup` contains versioned prerequisite commands.
Neither is executed by jazyk.dev or the API host. Generate workflow YAML using a
library; never interpolate arbitrary user input into a server shell or accept an
unreviewed command as a workflow-dispatch input. The immutable definition selects it.

`environment` supplies the runner/toolchain setup. Record its resolved configuration
and actual versions. `reports` and `artifacts` declare outputs. The test runner must
be configured to emit the named report; adding `reports/junit.xml` does not make
`npm test` emit JUnit automatically. The model can propose the necessary reporter
configuration with the workflow diff. Freeze config, generated workflow/helper,
command, and dependency files with the candidate. Validate required input paths.

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

Creating or editing a command delivery automatically generates or updates its owned
workflow and any helper files in the same workspace changeset. Show the combined
diff. Stamp stable delivery identity, generator version, and definition digest in
the generated files. Detect manual edits before regeneration; preserve unrelated
workflows and surface conflicts instead of overwriting custom work. Saving these
files does not publish or dispatch them. Preparation refuses stale generated workflows.
Templates must not guess credentials or add unintended push-triggered publication.

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
| `delivery.runs` | List this delivery's attempts with GitHub run/attempt ids, source revisions, and results |
| `delivery.cancel` | Request cancellation; reconcile already completed external effects |
| `delivery.files.list/read` | Read the exact repository candidate and uploaded artifacts with ranges and limits |
| `delivery.diff/import` | Preview and import selected uploaded file changes against expected workspace versions |

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
2. Freeze exact source/artifact bytes, config, commands, scripts, workflow, toolchain identity,
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

Project files and CI output files are distinct views. Every run-file request names
an attempt id and base revision. Actions exposes the pinned repository candidate
plus uploaded outputs. It has no general interactive file or command tools on its
hosted runner. `delivery.exec` and live runner filesystem access are deferred with
our own runtime. Retained outputs have immutable digests. Importing selected changed
files checks the project's current revision and never silently overwrites it.
See [GitHub evidence](./github.md#actions-evidence-and-file-access).

Collect structured reports, initially JUnit XML with optional Jazyk criterion-id
mapping, plus raw logs. Record run/attempt id, candidate digest, commit, toolchain,
test identity, status, and report digest. Only explicitly mapped criteria receive
verification evidence. Unknown, missing, skipped, expired, or stale results do not
become passes. Parse untrusted reports with size limits and external entities disabled.

Import matching evidence into the existing generation verification ledger through
its normal writer/gates. A newer project revision can retain historical evidence
without treating it as current. Provider success and arbitrary text in a log never
prove every criterion or external preservation obligation. The detailed
[test contract](./testing.md) keeps CI verdicts separate from LLM judgments and
defines aggregate-command versus per-criterion evidence.
