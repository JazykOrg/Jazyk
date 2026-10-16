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
and compare the configuration hash. A task edit cannot change a submitted run.

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

Inputs include documentation, source, tests, assets, and project settings with credential fields removed. They exclude credentials, Git internals, runtime history, conversations,
and other projects. The command runs through `/bin/sh -c`. Arguments and environment
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
reuse tasks, repair owned files, and run focused checks within a cumulative budget.
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

A programmatic binding uses `run = "task:<id>"`. Its `name` is an exact
`classname::name` JUnit selector, or a unique unqualified case name. A missing or
ambiguous selector records `none` with the observed execution state as evidence.
Several criterion bindings can share one task. `run_tests` runs each referenced
task once and imports all of its matching cases. Legacy command bindings remain
local compatibility paths and cannot execute on a Cloud API host.
Single-row reruns and benchmark checks use the same task adapter. They judge the
selected case, not the whole command's exit code. A skipped, missing, ambiguous, or
incomplete result cannot count as a successful test or a falsifiable failure.

Successful products and per-case evidence enter the workspace only when its
eligible input still matches the frozen snapshot. An intervening edit leaves the
result in run history. Source changes from a command remain reviewable changes;
they do not silently overwrite source. Existing files outside `products` stay as
run output until an agent or person applies them through ordinary file edits.

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
