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

Recommended layout, using the existing sidebar and tabs:

```text
Sidebar                         Tabs
Files                           main.md | Create presentation #42
  docs/                         Overview  Logs  Files  Products  Tests
  scripts/                      Building · medium · 03:12 elapsed
  dist/                         Stops in 01:48 · usage reserved …
    slides.pdf  [generated]      [Stop] [Run command]
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

Alternative: a virtual directory inside Files reduces sidebar groups but risks
implying these are project paths. A bottom execution panel preserves more sidebar
space but makes many runs harder to browse. These remain open layout options.

## Run tab

The overview shows purpose, actor, candidate revision, action label, state, started
time, effective deadline, resource size, environment, cache hits, and usage. Show
provider details in an expanded technical section, not as the primary product label.

Logs stream with pause/follow, search, copy/download, stream and step filters, and
links from a test failure to its output. Reconnect by cursor after a network drop.
Historical logs use the same viewer. Missing or expired output is labeled.

Files shows the live sandbox tree while it exists, on refresh or bounded polling.
Mark input, modified, product, cache, and scratch roles. Large cache directories
are collapsed and paginated. Preview/diff works without importing into the project.
After shutdown show only retained manifests/products/reports, with a clear note
that scratch files are gone.

Products provides download and supported preview. Files receives promoted products
at their real project paths after a checkpoint. Mark generated ownership and source
revision. A cached dependency is never added to canonical Files merely because it
exists in a sandbox. `Clear cache` is in run/runtime settings, not `Delete product`.

Tests shows selected cases and passed/failed/skipped/missing results, with stale
evidence called out. A build/setup failure gets one clear cause. Pure LLM judgments
show their evidence and use no container status badge.

## Human commands

Recommended v1: a bounded command console in a run tab. The user chooses working
directory and timeout, submits a command, and gets the same streamed logs and stop
control as the agent. Commands run through the same admission and retention path.

`Start environment` is an advanced action, with image, size, and estimated usage.
It can default to opening a task-bound sandbox with a short idle limit. A terminal
tab left open does not keep resources alive indefinitely.

Only one actor submits foreground commands at a time. The user can pause agent
control or stop a command, then take over. Manual shell changes mark the sandbox
revision dirty. Before the agent resumes, it reads the diff; verification receipts
continue to refer to the exact inputs they tested.

A full interactive PTY is an alternative for v1 or a later enhancement. It needs
stdin/resize/reconnect handling, shell ownership, recording, and lease semantics.
It must use the same provider adapter and budget. Do not expose provider credentials
or SSH access to the API host. Line-command support is not advertised as a full PTY.

## History and control

History filters by purpose, action, actor, outcome, and date. Keep links from chat,
compiler sessions, generation failures, and verification rows to the run. Show a
parent run with its command steps, rather than a sidebar item for every shell call.

`Stop` remains available after allowance exhaustion. Distinguish stopping from
confirmed stopped. `Run again` creates a new attempt on a selected revision and
shows changed inputs. It never blindly repeats an unknown publication. `Start from
retained inputs` starts a new sandbox; it does not resurrect a dead one.

Show queue reason and position when practical, including `Waiting for the project's
active run`. The user can cancel or stop the current run. Do not allow a queued run
to become stale and silently execute a different candidate.

## Usage and failure presentation

Plan & usage keeps its existing monthly, five-hour, and seven-day meters, used and
pending separately. Add a breakdown for models and runtimes, resource/time history,
and active runtime reservations. The main run view can say `Small · 10-minute limit`
and show an estimated percentage. Keep internal credits and supplier keys out.

A resource/time increase shows the revised maximum use before a user confirms it,
or appears in the activity log when an agent is already allowed to choose it.
Expired idle environments read `Stopped after inactivity`. Failed tests, OOM,
unavailable runtime, exhausted allowance, and failed product import have distinct
messages with the action that can resolve them.

## Local and browser use

Local GUI uses the same run UI against the local API. It says `Runs on this machine`
and does not show Cloud billing for local compute. Missing Docker/Podman gives setup
instructions and explicit alternatives; it never silently executes on the host.
Browser-only projects show runtime capability as unavailable until connected to a
local engine or hosted execution. Reading files and LLM-only verification continue
where their existing capabilities permit.

Proposed CLI equivalents are `jazyk run <task>`, `jazyk deliver <name>`,
`jazyk runs`, `jazyk logs <run>`, and `jazyk stop <run>`. Existing `jazyk gen` and
`jazyk test` keep their role. Exact naming should be checked against the CLI before
adoption. MCP exposes the same durable operations and evidence.
