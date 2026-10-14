# Editor and command-line experience

Status: proposal for the existing jazyk.dev editor, whose implementation remains
in `jazyk-cloud`. No replacement UI is proposed.

## Delivery split button

- Zero deliveries: show `Set up delivery` in the delivery slot, or a quiet disabled
  `Deliver` with a setup entry. Do not invent a default destination.
- One delivery: use its label as the main action, e.g. `Create presentation` or
  `Sync to GitHub`. A menu contains settings and history.
- Several deliveries: the main action executes the last selected delivery. Its
  label matches that selection. The arrow menu lists the other named actions and
  `Manage deliveries`.

Store selection per project, not globally. Renaming/removing an action updates the
selection safely. Long labels truncate accessibly with the full label available.
While executing, show state and a link to the run. Disable duplicate execution of
the same active action; other work follows the project's runtime queue policy.

Clicking a build-only action starts the admitted run. External publishing behavior
depends on the chosen [autonomy policy](decisions.md#delivery-autonomy). A required
approval presents the concrete candidate, destination, script, and maximum usage.
Do not ask repeatedly when an existing grant already authorizes that operation.

## Small settings form

The normal form contains name/label, script picker, and environment. The environment
defaults to the project image. Products, cache, timeout, size, environment variables,
and secret references sit in an advanced section. Selecting an existing task is
an optional reuse control. The TOML editor remains available.

GitHub sync substitutes repository and branch plus an include/exclude preview.
Show GitHub App access separately from sign-in. A project with a script delivery
never has to fill in a repository. Do not place test-case mappings in this form.

Agent changes appear as normal project settings/script diffs with their originating
requirements. Validation names missing scripts, bad globs, unsupported runtimes,
or missing grants before execution. The form shows effective host caps alongside
requested timeout/size when they differ.

## Runs alongside Files

The owner selected Runs beside Files, using existing tabs:

```text
Sidebar                         Tabs
Files                           main.md | Create presentation #42
  docs/                         Overview  Logs  Changes  Products  Tests
  scripts/                      Building · medium · 03:12 elapsed
  dist/                         Stops in 01:48 · usage reserved …
    slides.pdf  [generated]      [Cancel run]
Runs (1 active)
  ● Create presentation #42     Steps
  ✓ Unit tests #41               ✓ Restore cache
  × Generate source #40          ● Build
  Show history…                 ○ Retain products
```

`Runs` is a collapsible peer of Files, not a fake disk directory. It includes
non-container GitHub sync and LLM-only verification. This avoids making users learn
that a stopped container and a retained delivery attempt are different histories.
Runtime details remain available inside a run.

The sidebar is execution history, not a container explorer. A run opens logs and
retained file changes in the existing tabs. This layout choice is settled for the
first release.

## Run tab

The overview shows purpose, actor, candidate revision, action label, state, started
time, effective deadline, resource size, environment, cache hits, and usage. Show
provider details in an expanded technical section, not as the primary product label.

Logs stream with pause/follow, search, copy/download, stream and step filters, and
links from a test failure to its output. Reconnect by cursor after a network drop.
Historical logs use the same viewer. Missing or expired output is labeled.

Changes lists added, modified, and deleted project paths against the frozen input.
Text files open a diff; binary outputs offer a retained preview/download and metadata.
Show imported, awaiting import, conflict, and excluded states separately. Collection
at a declared checkpoint or run completion populates this view. While executing,
show the latest completed checkpoint or `File changes available when the run finishes`.
There is no live container file browser.

Retained changes remain inspectable after teardown, even when a test failed. Cache
contents and unretained scratch are not a durable project diff. Excluded-path summaries
can explain what was discarded. If timeout/provider loss interrupted collection,
show the diff as incomplete, never as evidence that no files changed.

Products provides download and supported preview. Files receives promoted products
at their real project paths after a checkpoint. Mark generated ownership and source
revision. A cached dependency is never added to canonical Files merely because it
exists during a run. `Clear cache` is in execution settings, not `Delete product`.

Tests shows selected cases and passed/failed/skipped/missing results, with stale
evidence called out. A build/setup failure gets one clear cause. Pure LLM judgments
show their evidence and use no container status badge.

## Action controls

People and agents start defined tasks, tests, and deliveries. Each run has a frozen
script/step sequence, input revision, image, size, and timeout. The user can inspect
progress or cancel. There is no start-empty-container action, command console, shell,
terminal, attach, manual command submission, or live deadline/resource change.

To investigate a failure, edit a script or define a focused diagnostic task, then
start another run. The run preview names its inputs and maximum usage. Edits affect
future attempts only. Scripts may contain multiple commands and declared steps may
checkpoint caches/products, but the executor proceeds without asking the model for
the next command.

## History and control

History filters by purpose, action, actor, outcome, and date. Keep links from chat,
compiler sessions, generation failures, and verification rows to the run. Show a
parent run with its declared steps, rather than an item for every internal command.

`Cancel run` remains available after allowance exhaustion. Distinguish requested
cancellation from confirmed shutdown. Completion always triggers collection and
teardown automatically. `Run again` creates a new attempt with a fresh container
and shows its selected revision/configuration. It never blindly repeats an unknown
publication. Inspecting historical results allocates no runtime.

Show queue reason and position when practical, including `Waiting for the project's
active run`. The user can cancel or stop the current run. Do not allow a queued run
to become stale and silently execute a different candidate.

## Usage and failure presentation

Plan & usage keeps its existing monthly, five-hour, and seven-day meters, used and
pending separately. Add a breakdown for models and runtimes, resource/time history,
and active runtime reservations. The main run view can say `Small · 10-minute limit`
and show an estimated percentage. Keep internal credits and supplier keys out.

A resource/time increase applies to a new attempt and shows its maximum use before
submission, or appears in the activity log under an existing agent grant. Timed-out
runs show the fixed bound they reached. Failed tests, OOM, unavailable runtime,
exhausted allowance, and failed product import have distinct
messages with the action that can resolve them.

## Cloud, local MCP, ACP, and browser use

Local GUI uses the same run UI against the local API. It says `Runs on this machine`
and does not show Cloud billing for local compute. Missing Docker/Podman gives setup
instructions and explicit alternatives; it never silently executes on the host.
Browser-only projects show runtime capability as unavailable until connected to a
local engine or hosted execution. Reading files and LLM-only verification continue
where their existing capabilities permit.

Local MCP clients and ACP IDE sessions can launch and inspect the same runs without
the Cloud editor. MCP returns IDs, bounded logs, and retained file/diff handles. ACP
surfaces action progress and results through the existing tool-call update stream,
with text/path fallbacks when the IDE lacks a rich diff viewer. Neither requires a
terminal. See [access contracts](access-paths.md) for authentication, cancellation,
reconnect, and local/hosted routing.

Proposed CLI equivalents are `jazyk run <task>`, `jazyk deliver <name>`,
`jazyk runs`, `jazyk logs <run>`, and `jazyk stop <run>`. Existing `jazyk gen` and
`jazyk test` keep their role. Exact naming should be checked against the CLI before
adoption. MCP exposes the same durable operations and evidence.
