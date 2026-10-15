# Runtime and delivery plan

Status: the implementation and completion work are merged to master and deployed to jazyk.dev on
2026-10-15. See the [release status](implementation.md#first-release) and the
[implemented contract](../../docs/compiler/task-runtime.md). The follow-up work and
its measured checks are in [completion](completion.md). The remaining sections
preserve the design discussion and alternatives. These plans live in
`plans/runtime-delivery/` in the repository checkout.

## Start with tasks and runs

A project defines one list of tasks. Each task has an ID, name, description, and
shell command. Optional file globs identify products, disposable cache, and test
reports. The model reads the name and description to decide when to use it.

```toml
[[tasks]]
id = "test"
name = "Run tests"
description = "Run unit tests after changing source or test code."
command = "mvn test"
reports = ["target/surefire-reports/*.xml"]
```

A command can invoke a script, but does not have to. `mvn install`, `git push`, and
`mvn test` are valid command forms. Build, test, lint, and delivery are purposes of
a task, not separate configuration types. There are no reusable action wrappers,
step lists, or named runtime/cache profiles. An optional image sits on the task.

A run executes one frozen task in a fresh isolated container, retains logs and file
changes, and shuts down. The agent inspects results, edits the project, and starts
another run when useful. Users and agents can cancel at any time, including while
the command is executing. There is no live shell or command injection into a run.

Any task producing test reports can contribute individual results to the existing
verification ledger. The graph and UI derive status from that evidence and its
input hashes. Command success alone cannot prove that every test ran or passed.
LLM tests continue to judge files without a container unless an artifact needs
building first.

## Selected direction and remaining choices

- AWS Fargate is selected for Cloud. The provider comparison remains a record of
  alternatives, not a requirement to launch three providers.
- Cloud, local MCP coding agents, and ACP IDE sessions use the same task/run tools.
  Client transport and execution location are independent.
- Runs sits beside Files. Existing tabs show logs, retained changes, products, and
  individual test results. The UI is run history, not a container explorer.
- Local Docker/Podman is the recommended default. Trusted direct host execution
  remains an explicit alternative. Local use needs no Cloud account.
- Start with one active run per project. Different projects may run concurrently.
- Products are durable, caches disposable, and unmatched new files transient.
  Preserve authorized source/test diffs. Cache saves require whole-command success.
- Charge runtime resource cost into existing monthly, five-hour, and seven-day
  usage limits. Show used and reserved usage; do not invent LLM token charges.
- Keep the optional no-container GitHub sync shortcut in the same task picker,
  backed by a connection setting and trusted handler.
- Publication uses saved per-task credential grants bound to the exact task
  definition. The agent can execute within a grant but cannot authorize or extend
  it. [Decision and alternatives](decisions.md#delivery-autonomy).

The minimum product surface is tasks, runs, and their results. Isolation, storage,
provider, and billing details below are implementation responsibilities, not more
concepts a project author must configure.

## Reading order

| File | Discussion |
| --- | --- |
| [Configuration](configuration.md) | One flat task list, commands, globs, optional controls, GitHub sync. |
| [Generation and tests](generation-testing.md) | Choosing tasks, case results, ledger status, LLM verification. |
| [Agent tools and prompts](agent-contract.md) | Shared task tools, phase-specific prompts, autonomy and evaluations. |
| [Editor](editor.md) | Task picker, Runs, logs, changes, test results and cancellation. |
| [Runtime](runtime.md) | Isolation, lifecycle, cancellation, timeouts, local and Cloud adapters. |
| [Cloud, MCP, and ACP](access-paths.md) | Routing, IDE updates, cancellation ownership and reconnect. |
| [Files and storage](files-storage.md) | Globs, import, caches, logs and historical storage. |
| [Billing](billing.md) | Reservations, settlement, rolling limits, trials and UI. |
| [Providers](providers.md) | Managed options, indicative costs and selection experiment. |
| [Decisions](decisions.md) | Accepted corrections and remaining alternatives. |
| [Implementation](implementation.md) | Docs-first sequence, repository ownership and acceptance cases. |
| [Current state](current-state.md) | Existing implementation and proposed replacements. |

## Relationship to the compiler and earlier plans

This draft replaces the GitHub Actions-only direction in the
[earlier delivery plan](../delivery/README.md). Its destination checks, preservation
rules, and recovery for uncertain publication remain useful. The earlier version
of this draft's runtime/cache tables, task wrappers, and ordered steps is superseded.

The harness still owns goals, identity, gates, scheduling, and graph commits. A task
is executable project configuration, not a compiler goal or a second scheduler.
Descriptions help the model choose a task while resolving its assigned goals. A run
does not create or reprioritize goals on the model's behalf.

A task can produce a PDF, EPUB, presentation, application, or any other output, or
publish externally with granted capabilities. Jazyk need not classify product types.
It distinguishes command outcome, retained files, verification evidence, publication
receipts where available, and confirmed shutdown.

## First release

Complete the loop: generate source/tests and tasks, run and repair, verify both
programmatic and LLM criteria, retain or publish products, show results in the
existing editor, and charge bounded usage. No GitHub repository is required.

Start with Linux and supported OCI images. Apple/Windows builds, GPUs, privileged
Docker workloads, and long-running hosting need compatible providers and explicit
support. Unsupported capabilities must be reported.

Full CI graphs, matrices, schedules, interactive environments, step checkpoints,
and continuous two-way Git sync are outside this first release. They must not
complicate the basic task model.
