# Project and delivery workflows

Status: UX proposal, 2026-10-03. Keep one project experience on jazyk.dev, CLI, ACP,
and LSP. Files, compiler, and delivery are independent choices with named defaults.

## Project panel

```text
Projects                                         Continue with GitHub

ERP                                              Open
  Files       This computer /work/erp
  Compiler    This computer
  AI          My configured provider
  Graph       Saved in this folder
  Deliveries  Publish source · Build and test

Try an example in this browser
```

Accountless local use keeps current provider and agent settings. No cloud account
is needed to compile or inspect the graph. The browser example is secondary and
clearly indicates browser storage and export. Do not expose it as another paid
runtime or promise direct access to the computer's files.

After login:

```text
ERP                                      Account: matus     Credits: …

Files       This computer /work/erp       Change storage…
Compiler    Jazyk Cloud                   Switch to local
AI          Jazyk managed                 Model policy…
Graph       This folder                   Saved, generation 42
Connection  Connector online
Repository  owner/erp                     Manage repository access

Compile     Deliver…
```

The repository row is a link/destination unless the user explicitly selected
GitHub-backed source. It does not imply storage mirroring. Advanced model policy
can later select models by compilation/generation task and display their credit
rates. Do not start with an independent matrix of browser, provider, and billing
switches. Current cloud work uses managed inference; local work uses local settings.

For a cloud project, show `Files: Jazyk Cloud` or `Files: GitHub owner/repo + draft`
and `Graph: Jazyk Cloud`. A GitHub draft can be saved without being published. A
local-backed cloud compile says `Waiting for connector` when it cannot commit.
Changing compiler location never quietly moves the graph.

## Subscription path

1. Open a local folder through `jazyk gui` or an IDE. Compile using the configured
   provider/agent, with no account.
2. Select `Use Jazyk Cloud`. Sign in with GitHub and obtain paid or explicit test
   credits. Explain that source/graph context is processed remotely.
3. Keep files and graph in the same folder. The connector supplies pinned input and
   commits results. CLI and IDE use the same bound project.
4. Select `Connect repository` when publication or Actions is wanted. Grant access
   to that repository and choose the destination/ref. Login itself does not publish.
5. Move files to cloud storage only through `Change storage`. Transfer graph and
   delivery records explicitly; stop competing writers during authority transfer.

Switching back to local compilation preserves ids and human decisions. A disconnected
local connector stops new compiler work that needs it. A fully staged delivery may
continue remotely, with its local receipt/import visibly pending.

Accountless local Git publishing uses existing credentials. Cloud compilation,
cloud-managed delivery orchestration, and cloud storage use the signed-in account.
Do not require a subscription merely to inspect previously saved results or return
to local use. GitHub Actions has its own repository billing.

## Delivery panel

```text
Deliver ERP                                            Add delivery

Publish source       Git → owner/erp, main              Prepare  Edit
  Last confirmed     commit a1b2c3

Build and test       GitHub Actions                     Prepare  Edit
  Last attempt       28 passed, 1 failed · source d4e5f6

Build artifact       Container                         Prepare  Edit
  Last attempt       Not run
```

`Add delivery` offers Git publish, GitHub Actions, and container script templates.
Editing produces a `jazyk.toml` diff and any workflow/script changes. The model can
perform the same edits in chat. Removing a definition retains past receipts.
Changing configuration does not start a run.

Preparation shows the selected files/revision, destination, script/workflow, expected
effects, last checkpoint, current observations, required checks, and execution/AI
budget. Existing authority can allow `Run` directly after preparation; do not insert
repeated confirmations for every tool call. New effects require a concrete review.

For an Actions workflow not yet installed on the default branch, show `Setup required:
publish workflow` or `Waiting for workflow PR merge`. Do not offer a Run button that
silently succeeds at dispatch while nothing can execute.

## Run view

```text
Build and test / attempt 17             Running on GitHub Actions
Source d4e5f6    Candidate c17           Open GitHub run   Cancel

Status     Logs     Files     Tests     Changes

✓ Source published
✓ Workflow dispatched
• Tests running
  Verification pending

Files: candidate source and uploaded artifacts
```

The container variant shows `Running in delivery container`, live run files, and
authorized command tools. The Actions variant shows step status, available logs,
and a link for GitHub's live view. It does not offer an interactive terminal or
claim to browse files that have not been uploaded.

After completion, show independent facts:

- Publication: exact commit/ref, or not applicable.
- Execution: completed, failed, cancelled, or unknown.
- Tests: parsed counts, unmapped results, missing/stale evidence.
- Delivery confirmation: confirmed scope or checks still required.
- Project changes: file diff and explicit import, with conflict state.
- Local save: acknowledged or waiting for connector, for local-backed projects.

Run files are visibly separate from project files. Importing changes uses the
current workspace revision; an old execution cannot overwrite a newer edit. Logs
and retained reports remain available after a container is destroyed. Expired files
show their expiry and do not become empty successful output.

## CLI and IDE

Use the same delivery commands/tools from the selected local or cloud project.
Possible CLI vocabulary is `jazyk delivery list`, `prepare`, `run`, `watch`, `files`,
and `cancel`; command names must enter the frontend specification before implementation.

ACP exposes definition editing, preparation, execution, status, and run-file tools
according to granted capabilities. LSP keeps diagnostics/navigation and links to
the matching criterion evidence. The local connector maps URIs and saves graph or
TOML edits; LSP is not used as a substitute for file transfer or billing authority.

A browser closing does not stop a cloud job. Local-backed compilation depends on
the native connector, which can outlive the browser. Local-native delivery can
remain accountless where the local adapter and credentials support it. Hosted
container execution always uses cloud admission and the runner boundary.

## Common failures

| Situation | User-visible behavior |
|---|---|
| Connector disconnects before local commit | Pending result; no saved claim; reconnect or explicitly return to local |
| Connector disconnects after delivery dispatch | Delivery continues if staged; local receipt/import pending |
| Credits run out | Stop new paid work; retain committed graph and admitted attempt status |
| GitHub App loses access | Connection unavailable; preserve state and inspect before retry after reconnection |
| Remote branch advances | Show conflict/replan; no force push |
| Workflow or report is missing | Setup/evidence incomplete, not success |
| Script times out after an external write | Outcome unknown; inspect destination before retry |
| Definition is deleted during a run | Existing pinned attempt remains visible; no new runs of the deleted definition |

These wireframes replace the earlier [subscription prototype](../subscription/ux/index.html),
whose compiler/storage coupling is no longer the design. A production UI change is
a later implementation phase, not part of this planning edit.
