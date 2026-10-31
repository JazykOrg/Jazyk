# Task execution

A project defines zero or more `[[tasks]]` in `.jazyk/jazyk.toml`. Each has a stable
`id`, `name`, `description`, and shell `command`. Build, test, and delivery are
purposes, not task types. The agent chooses by name and description. A command can
invoke a script but does not require a script file.

E.g.:

```toml
[[tasks]]
id = "test"
name = "Run tests"
description = "Run unit tests after source or test changes."
command = "mvn test"
image = "maven:3.9-eclipse-temurin-21"
reports = ["target/surefire-reports/*.xml"]
```

A test task is a reusable project operation, such as `test`, `test-unit` or
`test-browser`. A requirement or criterion identifies a test case, not a task or
container. Related checks extend the existing suite and use its ordinary runner.
Different toolchains, isolation needs or independently useful suites can justify
separate tasks. The agent chooses the useful execution boundary.

Before defining a task, inspect `task_context`, the dependency and runner
configuration, and existing test commands. Prefer a stable command that discovers
the suite and emits fresh JUnit cases. Each binding selects its own case from that
report. Adding a criterion does not require editing the saved task. A single
`run_tests` call can select several requirements or entities and runs each shared
task once. A later call starts a new run, even when the task is unchanged.

`run_task` accepts `task_id`, `expected_revision`, `operation_key`, and optional
`size`, `timeout_seconds`, and `wait`. It has no per-run arguments or environment override.
Test selection belongs to the saved command and runner today. Do not invent an
`args`, `inputs` or `env` field, clone a task per criterion to simulate one, or
rewrite a task between criteria when its normal suite already covers them.

## Settings

Optional fields are `image`, `cwd`, `size`, `timeout_seconds`, `env`, `products`,
`cache`, and `reports`. Defaults are `python:3.12-slim`, project root, `small`, and
300 seconds. Sizes are small (1 vCPU, 2 GiB), medium (2 vCPU, 4 GiB), and large
(4 vCPU, 8 GiB). Hosts may impose lower limits. The execution timeout is at most
7200 seconds. Environment values are non-secret. Cloud provider credentials never
enter task environments. Arbitrary publication credentials are not granted by TOML.

IDs contain up to 60 ASCII letters, digits, underscores, and hyphens. A project
supports 100 tasks. Names, descriptions, commands, and environment values are bounded.
Unknown fields and duplicate IDs are rejected. Edits preserve unrelated settings
and compare the configuration hash. Task and document-set edits share the short
configuration lock and atomic replacement. The hash is checked again while that
lock is held, so a concurrent edit cannot replace a newer matcher or task list.
A task edit cannot change a submitted run.

File globs are project-relative. `products` retain generated outputs. `cache` marks
disposable files restored before execution and saved only after command success.
`reports` identifies JUnit results. These fields default to empty. Logs and changes
to existing eligible project files are retained independently. New unmatched files
are transient. Protected compiler state, secrets, symlinks, traversal paths, device
files, and conflicting classifications are refused. Cache never overwrites input.
Use `cwd = "."` for the project root. An identical pattern cannot appear twice,
including across `products`, `cache`, and `reports`. Put test report patterns in
`reports`. Validation errors name the offending task, field, and pattern, including
both fields when a pattern is repeated.

## Runs

Submission freezes the task and project input. It returns a durable run ID. The
host executes one command in a fresh isolated environment, captures bounded results,
and destroys the environment. No interactive shell, appended command, or deadline
extension exists. A retry is a new run. Repeated operation IDs return the original
run and cannot dispatch twice.

By default `run_task` returns the run ID immediately. With `wait = true`, the host
waits for the run in code and returns its terminal status in the same reply. Waiting
makes no model inference per state change. It ends early, with the current
non-terminal status, when the owning operation is cancelled, the worker is asked to
suspend, or the wait outlasts the task timeout plus 600 seconds. Cancellation also
stops the run. Suspension leaves the run running. A poll that fails, including a
hosted poll interrupted by suspension or cancellation, ends the wait the same way:
the reply keeps the run ID and its last status and adds `wait_error`. A
non-terminal reply means: continue with `wait_run`. The reply is an ordinary tool result, so a suspended worker resumes
after it without an uncertain pending call. Waiting adds no saved state and does not
change the run's fixed deadline.

Inputs include documentation, source, tests, assets, and project settings with credential fields removed. They exclude credentials, Git internals, runtime history, conversations,
and other projects. A nested `.jazyk/jazyk.toml` stops the input walk. Private
`.jazyk` paths at any depth stay excluded, except the owning project's redacted
configuration and generated ignore file. Symlink aliases cannot expose another
project. The command runs through `/bin/sh -c`. Arguments and environment
are passed as data. Concurrent source edits do not change the input snapshot.

Local Docker, Podman, and hosted execution share task, manifest, result, and
cancellation contracts. Browser-only execution returns unavailable. Local execution
requires no Cloud account. A project command never runs on the hosted API server.
The runtime helper is an independent static Linux executable. An initialization
container supplies it to the selected image; customer images need no Python, curl,
or provider SDK. The helper is built from `runtime-worker/` with the Go toolchain.
Its container image can be built locally without a Cloud account.

`JAZYK_CONTAINER_ENGINE` selects `docker` (default) or `podman`. This is a host
setting, never a project command or executable path. Discovery probes the selected
engine with a short deadline and reports unavailable daemons accurately. Submission
freezes the selected engine in the run record. Later reads, cancellation, and the
watchdog use that engine even if the environment changes. Inputs and outputs copy
through the engine API. No host directory is mounted into the project container.
Local images use the engine platform. Private images use the operator's existing
engine registry authentication. Registry credentials never enter project files.

## Cancellation and limits

Cancel is available while queued, provisioning, executing, collecting, or stopping.
It is idempotent and does not require remaining usage. Stop intent prevents queued
dispatch and further promotion. A supervisor signals the process tree, then forcibly
terminates the environment after a bounded grace period. A detached child cannot
keep the run alive. API/client disconnection does not remove the fixed deadline.

The run distinguishes command outcome, collection completeness, and confirmed
termination. It says cancelling until the provider confirms shutdown. Unknown
shutdown remains subject to reconciliation. Cancellation does not roll back a
completed external write or an already committed artifact. Unknown publication
outcomes require inspection before retry.

Setup, command, collection, and shutdown have separate bounds. Allocation and
cleanup have independent host supervision. Local Docker has a separate deadline
watchdog process. It removes overdue containers even if the run supervisor exits.
Logs and transfers have size limits.
Missing cache causes rebuilding. Oversized cache is omitted without making product
collection incomplete. Changed input files remain reviewable even if a cache glob
also matches their paths. Retained output has priority over disposable cache. Failed and cancelled commands save no new cache.
Partial results remain attached to the run and cannot replace a successful product.

## Tools and access

A host without an attached runtime reports execution unavailable. Discovery does
not panic when a portable workspace has no execution adapter.

Cloud, local MCP, and ACP use `task_context`, `update_tasks`, `run_task`,
`inspect_run`, `wait_run`, `cancel_run`, `read_run_log`, `search_run_log`, and
`read_run_changes`. Tool availability does not depend on native file-tool support.
Task definitions and execution do not require a compiled graph or a selected
document set. Read-only clients cannot edit or execute. Client transport does not choose the
execution location. All reads are bounded and historical reads allocate no runtime.
`task_context` includes `projectRoot` and the default `cwd` of `.`. A generation
package's absolute deliverable path is made relative to `projectRoot` for a task's
`cwd`. When those paths are equal, `cwd` is `.`. The project directory's own name
is not a path prefix inside the snapshot. Product, cache, and report globs remain
relative to that root even when the command uses another `cwd`.

Generation and binding receive the execution prompt and task descriptions. They
reuse ordinary project suites, repair owned files, and run useful checks within a
cumulative budget. A bind goal's one-requirement scope does not define a task or
container boundary. Generation chooses the order of implementation and test work.
Focused test-first work remains available when it is useful. Missing implementation
does not require an expensive failing baseline before source can be developed.
Graph extraction and judgment do not receive execution authority by default.
Existing goal gates still apply. The model does not schedule compiler goals.

## Results and verification

Reports map exact case identities to existing criterion bindings. A task can produce
reports regardless of its name or purpose. The harness imports programmatic verdicts
into the generation ledger. Status is derived from verdicts and tested input hashes;
execution never writes authored graph facts claiming a pass.

Passed and failed cases are conclusive evidence. Skipped, interrupted, proven not-run,
ambiguous, and missing results are not passes. Absence from a report is not proof
that a case never executed. Never ran means no execution history. Completed cases
can survive a cancelled run, but the run remains cancelled. A requirement with
criteria is verified only if every required bound criterion has current passing
evidence. Command exit zero cannot verify a whole suite by itself.

Reports are fresh run outputs, never cache. Old generated reports are removed before
execution. Statement hashes cannot be refreshed by running tests. Test/source hashes
can identify only the actual tested inputs. Concurrent edits make old evidence stale.
If a command changes source/tests, rerun the resulting candidate before claiming it
verified. LLM verification keeps its criteria and `record_verdict` contract and
uses no container unless a task must first produce an artifact.

Results include logs, file changes, products, test cases, frozen input identity, and
termination. Imports compare revisions and preserve newer edits. Binary products
remain binary files. Historical results remain readable after container teardown.
`task_context` history and the `run_task`, `inspect_run`, `wait_run`, and
`cancel_run` replies omit log text, including idempotent starts and cleanup.
Repeated context requests do not reload historical logs into the model context.
`read_run_log` and `search_run_log` read the retained log in bounded parts.
`read_run_log.offset` is a zero-based Unicode character offset. Its `limit` counts
characters, not bytes or lines. Continue with the returned `next_offset`.
`search_run_log.offset` skips lines from zero and `limit` bounds the matching lines.
Each match's `line` is one-based. A search match's line number is not a read offset.
Search for a failure message first. Read a bounded character page when more log
context is needed.

An owned worker's MCP status replies name the frozen task by `id` and `name` instead
of repeating its definition. They carry one `result`, without a duplicate `summary`. All lifecycle,
input identity, termination, error, and result fields remain available. Saved task
definitions and the retained execution result do not change. HTTP task views keep
their full response. A standalone MCP serving also keeps the full response.

`task_context` accepts optional `task_ids`. Without it, the reply carries every task
definition. With it, `tasks` carries full definitions only for the named IDs and
`index` lists every other task by `id`, `name`, and `description`. An empty list
returns the index alone. An unknown ID is an error that names it. The revision,
defaults, execution capabilities, and run history are unchanged. `update_tasks`
replaces the whole list, so read the full default before editing tasks.

An owned worker's `task_context` returns at most five recent runs. `before` and
`next_cursor` page through older runs. It still returns every current task definition
(or the `task_ids` selection and index), the configuration revision, defaults, and
execution capabilities. A standalone
serving keeps the normal history page. Status polls never include source snapshots
or file contents.

A programmatic binding uses `run = "task:<id>"`. Its `name` is an exact
`classname::name` JUnit selector, or a unique unqualified case name. A missing or
ambiguous selector records `none` with the observed execution state as evidence.
Several criterion bindings can share one task. `run_tests` runs each referenced
task once and imports all of its matching cases. Legacy command bindings remain
local compatibility paths and cannot execute on a Cloud API host.
Related requirements and criteria should share a saved suite task and use separate
JUnit selectors. One `run_tests` pass groups by task ID, not by requirement or
criterion. A later pass freezes its own current input. Separate owners never share
a task environment or retained evidence. An explicit single-test rerun remains a
new run. Generation does not require a failing test baseline before implementation.
Durable task receipts distinguish the nearest pending GDK tool invocation. A new
`run_tests` call receives a fresh task intention, including after source edits in
the same agent session. Replaying the same invocation retains its original
intention, destination run, and deadline. Parent compiler progress keeps its
existing command identity. This contract was introduced in checkpoint operation set
`jazyk-gdk-2` and is retained by later operation sets. Release preflight refuses incompatible operation sets, rather than
guessing which invocation an old task receipt belongs to. Compatible checkpoints
continue to resume normally.
Single-row reruns and benchmark checks use the same task adapter. They judge the
selected case, not the whole command's exit code. A skipped, missing, ambiguous, or
incomplete result cannot count as a successful test or a falsifiable failure.

Successful products and per-case evidence enter the workspace only when its
eligible input still matches the frozen snapshot. An intervening edit leaves the
result in run history. Source changes from a command remain reviewable changes;
they do not silently overwrite source. Existing files outside `products` stay as
run output until an agent or person applies them through ordinary file edits.

An owned hosted worker imports its submitted run's terminal result into its active
workspace before returning `run_task`, `wait_run`, or `inspect_run`. A following
tool or task sees those products while the worker still owns the project lease.
The worker uses the frozen task and input identity, validates the retained result,
and applies the same input comparison as other imports. It does not acquire a
second lease or write the server filesystem. Browser-only execution stays unavailable.

The submission and import receipt live in the protected execution capsule and
enter the ordinary checkpoint with the workspace. Repeated polls reuse the import
outcome. They do not overwrite a later edit to an imported product or reimport old
test evidence.
Historical runs without that worker's submission receipt remain read-only. A
resumed worker uses the same run and receipt. Status replies include the import
outcome without retained file bytes. Internal build and test calls use this same
import path and fetch a terminal result once. Completed executions retain no
submission files in the workspace.

Product selection does not grant writes to Documents, protected paths, or nested
projects. Submitted input changes outside `products` stay reviewable. Public
artifacts such as `target/*.jar` can be retained and replaced by a later run
without becoming Source.

## Local supervision and ownership

A local run stores its owning operation separately from its observer. MCP build
cancellation and ACP session cancellation stop active child runs. Independent runs
keep running when another session is cancelled. Connection loss alone does not
cancel a run. The supervisor observes cancellation without a further tool call.
Publication and cache promotion share a finalization lock with cancellation. Once
finalization commits, later cancellation cannot change its outcome.

## Remote execution from a local engine

`JAZYK_RUNTIME_URL` selects a Cloud project's runtime endpoint.
`JAZYK_RUNTIME_TOKEN_FILE` names a private host file holding its runtime access
token. These are host settings. They are not project TOML or model tools. HTTPS is
required except for loopback development servers. Redirects are refused.

The Cloud project owner creates a project-scoped token in Tasks settings. The token
permits execution and retained run reads, not project editing, account operations,
or credential grants. It expires after 30 days and can be revoked. Cloud limits and
billing apply. The local snapshot is the run input; retained products return to the
local project with revision checks. The Cloud project's source is not replaced.
MCP and ACP use this adapter through the same runtime tools. No Cloud account is
needed when no remote endpoint is selected. Remote failure never selects a local
shell or another provider automatically.

## Granted credentials

A Cloud grant belongs to the account and exact task definition, outside project
TOML. It can supply named secret environment values and private registry credentials.
Changing the task requires a new grant. Revocation prevents new dispatch and cancels
active uses. Runtime tokens and model tools cannot create or replace a grant.
The helper masks exact secret values in retained and streamed logs, including values
split across writes. This is accidental-disclosure protection, not a restriction on
what an authorized command can publish. Generated files can contain command output.

Remote submission records ownership before making the network request. A cancel
notification marks pending submissions as well as accepted runs. Starting another
prompt cannot clear cancellation of a request still in flight. Lost responses
leave the Cloud operation bounded by its original deadline and operation key.

Inspecting or cancelling an existing run does not parse the current task settings.
An invalid edit after submission cannot prevent stopping that run. Its frozen
request remains the authority for execution and retained results.

Declared product globs override the default omission of `target`, `node_modules`,
and `.cache` input directories. Matching retained products are available to later
tasks. Other files in these directories remain omitted from project input and use
the disposable cache when configured. Protected paths and symlinks stay excluded.
