# Project workflows with local and remote Jazyk servers

Revised 2026-10-02. This replaces the earlier coupling of a project's storage and
compiler location. The [previous clickable prototype](./ux/index.html) is historical;
its subscription and cloud-runtime flows are not the current proposal.

## Three supported modes

| Mode | Files and durable graph | Compiler | Account |
|---|---|---|---|
| Local-local | Computer folder | Local Jazyk | Not required with your own provider/agent |
| Local-remote | Computer folder through connector | Remote Jazyk | Required, with entitlement and credits |
| Remote-remote | Cloud workspace or GitHub-backed workspace | Remote Jazyk | Required, with entitlement and credits |

A local project is defined by its source authority, not by where the compiler runs.
A subscription can serve both local and cloud projects. Hosted compilation uses the
same shared service in either case. A project container is not part of opening one.

## Project panel

Show `Files`, `Compiler`, and `AI / billing` as separate facts. Use ordinary labels in
the main UI. Technical storage/protocol details belong in a details panel.

```text
Projects                                      [Sign in]   [+ New project]

Project          Files                  Compiler          AI / billing
Acme ERP         This computer          This computer     Your provider
Local handbook   This computer          Jazyk Cloud       Jazyk credits
Product spec     Jazyk Cloud            Jazyk Cloud       Jazyk credits
Service docs     GitHub: acme/docs       Jazyk Cloud       Jazyk credits

Selected: Local handbook
Files:      ~/Projects/handbook
Graph:      Saved in the same folder, generation 42
Compiler:   Jazyk Cloud                 [Change compiler]
AI:         Auto, using Jazyk credits   [Advanced]
Connection: Computer connected          [Details]
Saved:      Local commit acknowledged

[Open project]   [Use in an IDE]   [Move files...]
```

The example lists all configurations to explain the design. In accountless mode,
show only locally known projects plus the sign-in entry, not another account's cloud
projects. After login add authorized remote projects and credit balance. Do not upload
or scan the computer's project list merely because the user signs in.

Inside the editor, keep a compact strip:

`Files: This computer · Compiler: Jazyk Cloud · AI: Jazyk credits`

Clicking it opens the same project details. Save state distinguishes local acknowledged
commit, cloud durable revision, and unpublished GitHub changes. `Connected` does not
mean `Saved`. The graph revision can lag the source revision and must show that lag.

## Accountless entry

The first page offers `Open on this computer`, `Try an example`, and `Sign in`.
Local use connects to native Jazyk and its selected folders. CLI and IDE users can
remain in those interfaces. No subscription or account is needed to use their own
provider or local agent.

If Jazyk is missing or disconnected, show installation/connection instructions and
`jazyk gui`. Keep the pending project selection. Never silently substitute a different
compiler backend. A local connector remains required for remote access to a local folder.

Browser WASM examples remain a secondary accountless playground. Label IndexedDB as
`This browser only`; label a memory-only fallback as temporary. Browser model reading
assistance is optional. A paid WASM compilation product is outside the server MVP.
Do not remove existing browser projects or workflows without a transition/export path.

## Local project to subscription

1. From the same folder choose `Use Jazyk Cloud to compile`.
2. Sign in, obtain entitlement/credits, and authorize the selected project connection.
3. Review the change: files and graph remain on the computer; compilation moves to
   the service; source and graph context are processed remotely; AI uses Jazyk credits.
4. Finish/cancel the current build and transfer writer authority at a committed
   generation. Preserve graph ids and human decisions.
5. Compile remotely. Writes return through the connector's conditional commit path.

The computer must stay connected while remote work needs source or commits. Closing
only the web tab need not stop the native connector. Losing the connection visibly
pauses work rather than turning it into an implicit cloud-stored copy.

`Change compiler` can return to local operation after the old writer is stopped and
its epoch revoked. Restore the selected local provider/profile explicitly. An exhausted
account never silently charges a different provider or starts another execution mode.

Keeping compilation local while purchasing managed model access can remain an advanced
future route. It is not required for the first server release and is not the primary
subscription journey in this revision. Avoid adding a fourth onboarding mode for it.

## Cloud and GitHub entry

A signed-in user creates a cloud workspace or, when the adapter ships, connects a
GitHub repository. Both use the remote Jazyk server and managed AI. Cloud workspace
reads and compilation do not start a project VM or incur an always-running VM charge.
Storage allowance and compilation credit policy are visible before use.

For GitHub show repository/branch, draft save state, last published commit, and where
the graph is retained. GitHub login and repository authorization are separate. A GitHub
badge does not imply graph state is committed unless that storage policy is enabled.

`Move files...` is separate from `Change compiler`. Moving a local folder into cloud
storage allows work while the computer is offline. Use a consistent export/import;
keep any original copy explicitly independent, without automatic synchronization.

## CLI and IDE

Keep ACP and LSP as first-class ways to use all three modes. A local proxy maps the
IDE's file identities to the selected server/workspace and preserves standard stdio
entry points. Proposed login/project-binding commands belong in the CLI spec before
implementation; this plan does not claim they already exist.

Passive LSP diagnostics and navigation require no inference credits. Explicit actions
that start model work use the selected server and billing route. An IDE's unrelated
assistant and an external agent's own provider remain separate bills.

Cloud project browsing needs an IDE virtual-workspace integration or explicit local
checkout/export. ACP/LSP endpoints are in the foundation; universal remote folder
browsing in every IDE is not implied. See [protocols](./protocols.md).

## Execution is an explicit task

`Compile` uses the shared server. `Generate source/tests` can produce virtual artifacts
when its contract permits. `Run tests`, `Build`, or execution needed by a generation
task starts a separately authorized isolated environment.

Before starting, show the task, base revision, execution limit, and rate. During it,
show `Execution: isolated container` and the run's files/logs. Afterward show imported
changes, test evidence, and any conflict with newer edits. A generated test is not
shown as passed merely because the model wrote it. No container persists as the
project's source authority.

## In and deferred

| In the foundation | Deferred or deliberately excluded |
|---|---|
| Three storage/compiler modes | Automatic local/cloud mirroring |
| Concurrent projects on a shared server | Arbitrary customer code on that server |
| Accountless local use and signed-in credits | Anonymous hosted model work |
| ACP/LSP for local and remote contexts | Universal remote filesystem support in all IDEs |
| Cloud and GitHub storage interfaces | GitHub adapter timing may follow the first beta |
| Optional execution contract | Container provider and generation/deployment product until needed |
| Owner's local Codex test profile | Personal Codex authentication as a hosted customer backend |
| Provider-neutral managed AI | Customer cloud BYOK, paid browser inference, and signup promotions |

The local folder is authoritative for the graph and human decisions in local-remote
mode. The connector must be online to save results. The UI reports saved only after
a local commit acknowledgement. The folder remains portable, and accountless local
use can resume without reconstructing graph identities or human decisions.
