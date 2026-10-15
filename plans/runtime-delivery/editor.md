# Editor and command-line experience

Status: proposal for the existing jazyk.dev editor in `jazyk-cloud`.

## One task picker

Use the existing split-button area for the flat task list. Build, test, render,
and publish all appear together. There is no second delivery list to maintain.

- Zero tasks: show a quiet `Set up tasks` entry.
- One task: show its name as the action, e.g. `Create presentation`.
- Several tasks: execute the last selected task; the arrow lists task names with
  descriptions and a `Manage tasks` entry.

Store selection by stable task ID per project. Renaming changes the displayed name.
Removing a task clears an invalid selection. The optional managed `Sync to GitHub`
row uses the same picker, backed by the GitHub connection settings.

A click starts the admitted task. Publication follows the chosen
[autonomy policy](decisions.md#delivery-autonomy). Where approval is required, show
the concrete command/candidate, effective destination/grants, and maximum usage.
Do not ask repeatedly when existing authority covers it.

Keep the existing semantic Test control if useful: it chooses bound cases and
uses their task references. It does not introduce another task definition form.

## Task settings

Show a simple list with ID, name, description, and command. The command is a text
field supporting multiline shell text, not a mandatory file picker. A script path
can be suggested inside the command. An ID is generated on creation and stays stable.
Descriptions explain purpose and when a person or model should use the task.

Optional globs are Products, Cache, and Test reports. Image, size, timeout, working
directory, non-secret environment, and granted credential references sit under
advanced controls. Show effective defaults. There are no task types, runtime/cache
profile editors, wrappers, step lists, or dependency graphs.

Agent task edits appear as normal settings diffs with originating requirements.
Validate empty commands, duplicate IDs, globs, paths, and unsupported capabilities.
Do not report a missing script just because the command is `mvn test`.

The managed GitHub row links to repository/branch and include/exclude settings. Its
trusted handler is not editable shell text. Sign-in and repository write access
are distinct. Ordinary task configuration requires no GitHub connection.

## Runs beside Files

The accepted layout uses a collapsible Runs group and existing tabs:

```text
Sidebar                         Tabs
Files                           main.md | Run tests #42
  docs/                         Overview  Logs  Changes  Products  Tests
  src/                          Running · small · 03:12 elapsed
  tests/                        Stops in 01:48 · usage reserved …
Runs (1 active)                  [Cancel run]
  ● Run tests #42               Command: mvn test
  ✓ Create presentation #41     14 passed · 1 failed · 3 awaiting result
  × Build #40
  Show history…
```

This is run history, not a container explorer or fake directory. Include managed
GitHub sync and LLM-only verification history with appropriate execution details.

## Logs, changes, and results

The overview shows task/name, actor, frozen revision, state, timing, effective
limits, resource size, environment, cache summary, and usage. Setup, command,
collection, and shutdown are lifecycle phases, not user-authored steps. Provider
IDs stay in expanded details.

Logs stream with follow/pause, search, download, stream filters, and failure links.
Reconnect by cursor. Historical logs use the same viewer; label gaps and expiry.

Changes becomes available after collection. It lists retained additions, modifications,
and deletions relative to frozen input. Text opens a diff; binary files offer
preview/download and metadata. Distinguish imported, pending, conflict, and excluded
paths. During execution say `File changes available when the run finishes`.
There is no live filesystem browser.

Failed and cancelled runs can retain useful diffs. Interrupted collection says
`Partial changes captured`, never `No changes`. Cache/scratch show only bounded
exclusion metadata; their bytes are not promised in history. Products enter Files
at their real paths after successful collection and revision-checked promotion.
Partial failed outputs do not replace the latest successful product.

Tests shows individual passed, failed, skipped, not-run, interrupted, and unknown
results, with current/stale evidence. A missing report is not proof that no case
ran. `Never ran` refers to absence of execution history. Show command success and
verification completeness separately. LLM judgments show their evidence without a
container badge. Linking a case opens its criterion and producing run.

## Cancellation and history

`Cancel run` is available while queued, provisioning, executing, collecting, or
stopping, including after allowance exhaustion. It sends stop intent immediately,
then shows `Cancelling` until termination is confirmed. Disable duplicate button
submissions while keeping status visible. A failed stop remains `Shutdown unknown`
with ongoing reconciliation; do not present a stopped badge prematurely.

The user can stop a running command without waiting for it to return. The supervisor
terminates children and ultimately the allocation. Logs and any completed results
remain inspectable. Explain a known published result or uncertain external write
when cancellation raced with it; cancellation does not undo publication.

Task edits apply to future runs. There is no manual console, attach action, live
command submission, or live resource/deadline change. To investigate, edit or add
a diagnostic task and start a fresh run.

History filters by task, actor, outcome, and date. Link compiler sessions, chat,
verification, and generation failures to runs. `Run again` creates a new attempt
with its selected revision/definition and admission; do not repeat an uncertain
external write automatically. Reading history allocates no runtime.

## Usage and access paths

Keep monthly, five-hour, and seven-day usage meters with used/reserved amounts.
Add model/runtime breakdowns and active run reservations. Show a size, fixed time
limit, and estimate before submission or in authorized agent activity. Differentiate
assertion failures, OOM, timeout, missing runtime, allowance exhaustion, and import
conflict with actionable messages.

Local GUI uses the same view and says `Runs on this machine`, with no Cloud compute
charge. Missing Docker/Podman offers explicit setup/alternatives; do not silently
execute on the host. Browser projects connect to local or hosted execution.

MCP returns durable IDs and bounded logs/diff/artifact handles. ACP surfaces the same
work through tool-call updates, with text/path fallbacks for minimal IDE clients.
No terminal capability is required. See [access paths](access-paths.md).

Proposed CLI commands: `jazyk run <task-id>`, `jazyk runs`, `jazyk logs <run-id>`,
and `jazyk stop <run-id>`. Existing `jazyk gen` and `jazyk test` remain semantic
entrypoints. A separate `jazyk deliver` namespace is unnecessary for the new model.
