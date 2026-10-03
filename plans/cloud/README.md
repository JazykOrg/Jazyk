# Plan: Jazyk Cloud and delivery

Status: proposed implementation plan, 2026-10-03. This combines the owner's latest
direction with [subscription planning](../subscription/README.md) and the
[delivery design](../delivery/README.md). It supersedes their deployment topology,
execution scope, GitHub timing, and implementation sequence where they differ.
It does not claim that these features exist or authorize infrastructure deployment.

## Recommended shape

Build a separate Rust `jazyk-cloud` API server that links the portable Jazyk compiler
and delivery libraries. Run it on one AWS EC2 instance with PostgreSQL installed
locally. GitHub Actions is the only execution backend for delivery commands in the
current scope. Jazyk-hosted runtimes, containers, and their workers are deferred.

The customer `jazyk` executable keeps CLI, ACP, LSP, and its local connection to
jazyk.dev. It does not acquire the cloud account server, PostgreSQL, Stripe, or
container orchestration. Keep the compiler suitable for extraction into a separate
open-source project. Do not start a full Jazyk process per customer.

Use GitHub-only account sign-in initially. Recommend a GitHub App with OAuth user
login and a separate repository installation grant. Linking a repository is easy
after login, but neither automatic publication nor access to every repository is
implied. Accountless local use remains available.

## Product model

The project panel shows files, compiler, and AI. Delivery is an on-demand action on
that project, not another project location or a permanently running environment.

| Mode | Authoritative source, graph, and human decisions | Compiler | Account |
|---|---|---|---|
| Local-local | Local folder | Local Jazyk | Not required |
| Local-remote | Local folder, online connector commits results | Jazyk Cloud | GitHub login and credits |
| Remote-remote | Cloud workspace, or GitHub source with a private draft overlay and cloud graph | Jazyk Cloud | GitHub login and credits |

Switching compiler location does not move storage. In local-remote mode, a save is
complete only after the connector acknowledges the local commit. Browser/WASM
examples remain a secondary accountless path; they do not get a hosted delivery
runtime. ACP and LSP remain supported in all three full project modes.

Each `jazyk.toml` can define named deliveries with two kinds:

| Delivery kind | Execution choice | Result |
|---|---|---|
| `git` | GitHub API or Git remote adapter, no project container | Publish a pinned file set to a repository/ref |
| `script` | GitHub Actions only | Generate a workflow for a command such as `npm test`, publish and dispatch it, collect run evidence |

A script can build, test, package, publish, or deploy. A Git delivery can publish
workflow files without running them. Publishing a commit, passing tests, and
confirming an external deployment are separate results. No automatic delivery on
document save or compilation completion.

LLM-run tests remain in Jazyk, launched and inspected from jazyk.dev with the selected
backend/model. Programmatic Python, JavaScript, and other executable tests run through
an Actions delivery. Import matching pass/fail evidence into the criterion ledger;
an LLM judgment is not proof that code tests ran. See [testing](./testing.md).

## What this retains from the delivery design

- A stable destination identity and its last confirmed checkpoint.
- A frozen candidate containing exact artifacts and relevant specification.
- Fresh destination observations, preservation obligations, and a transition plan.
- Durable attempts, including partial effects and unknown outcomes.
- Verification before advancing the confirmed checkpoint.
- Delivery evidence survives compiler output resets. It is not a new graph node kind.

Start with Git, a simple script, and Actions. Stateful upgrades and collaborative
document publishing remain fixtures for the common contract, not required provider
integrations for the first cloud release. A successful shell exit alone does not
prove preservation of a live database or outside editorial changes.

## Read in this order

- [Architecture](./architecture.md): Rust host decision, package boundaries, shared compiler, API.
- [Project layout](./projects.md): separate cloud specification, graph output, application, and local library build.
- [Storage](./storage.md): PostgreSQL records, virtual workspaces, local commits, delivery memory.
- [Deliveries](./deliveries.md): TOML, model tools, candidates, attempts, verification.
- [GitHub](./github.md): OAuth, repository grants, Git publishing, Actions, test reports.
- [Testing](./testing.md): LLM judgments in Jazyk, code tests in CI, criterion results and freshness.
- [Execution](./execution.md): one EC2 for the API/database, GitHub-hosted execution, deferred runtime.
- [Workflows](./workflows.md): project panel, delivery panel, subscription path, IDE behavior.
- [Implementation](./implementation.md): milestones, estimates, acceptance gates, open choices.

The linked pages distinguish owner decisions from recommended defaults. TOML, API,
tool, package, and table names are illustrative until adopted into `docs/`.

## In and out

| In the first complete private implementation | Deferred |
|---|---|
| One EC2, local PostgreSQL, Rust cloud API | ECS, Kubernetes, managed database, distributed services |
| Concurrent projects, one compiler writer per project | One container or process per compiling customer |
| Three storage/compiler modes, ACP/LSP | Automatic local/cloud storage mirroring |
| GitHub login, repository linking, Git delivery, Actions | Google login, teams, general cloud BYOK |
| Command delivery through generated Actions workflows | Jazyk-hosted runtimes, containers, interactive shells, application hosting |
| Delivery CRUD and execution through model tools and UI | Automatic publication merely because the model edited TOML |
| Checkpoints and recovery records | A mandatory release catalog or universal migration engine |
| Repository snapshots and uploaded CI outputs | Live runner filesystem access and build-cache management |
| LLM tests in Jazyk and programmatic results from CI | Treating an LLM judgment or green workflow as every criterion passing |
| Existing private local Codex experiment | Personal Codex credentials serving hosted customers |

Keep the previous [private Codex testing boundary](../subscription/codex-testing.md).
Use a supported provider integration for the deployed service. Model supplier,
stage-specific model policy, frontier-model credit rates, and commercial prices
remain open. Keep the Canadian Stripe account and credit reservations; paid hosting
does not eliminate free local use. Promotion credits remain later work.

## First useful result

One project can compile through a scoped server context, edit a named delivery in
`jazyk.toml`, publish an exact candidate to GitHub, run its command in Actions,
and show the matching test report. A second project can do the same concurrently
without mixing files, graph state, events, credentials, or accounting.

The same jazyk.dev test view shows LLM verdicts and imported code-test results with
their separate evidence sources. No Jazyk container implementation or provider
selection is required for this release.
