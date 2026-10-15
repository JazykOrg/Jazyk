# Implementation sequence

Status: first implementation is merged to master and deployed on 2026-10-15.
The phases below remain the original work breakdown. The implemented contract is
[task execution](../../docs/compiler/task-runtime.md).
Each accepted behavior enters the owning repository's `docs/` before code.

## First release

- One flat task list, shell commands, optional images/resources/timeouts, and
  product/cache/JUnit globs are implemented.
- The shared engine exposes task tools to generation, binding, verification, MCP,
  and ACP. LLM tests retain their direct verdict path. Programmatic results map
  individual cases to existing criterion bindings and tested-input hashes.
- Local Docker and hosted Fargate use finite isolated runs, retained logs and
  changes, fixed deadlines, cancellation, and confirmed shutdown. Cloud adds an
  independent watchdog, durable reconciliation, admission, and runtime metering.
- Project Settings includes Tasks. The header selects a task; Runs sits beside
  Files and opens logs, changed files, and test results in existing tabs. Managed
  GitHub sync remains separate from container allocation.
- Cloud execution passed real success, failure, skipped-case, cancellation, and
  watchdog checks. Production execution retained a file and passing test report,
  charged runtime usage, and confirmed cancellation. No test containers remain.
- The release preserved existing production project content. Live Chrome verified
  settings persistence, Runs, Files, and accountless offline reopening.
- Engine verification passed 655 tests. The ARM API passed 91 tests and its
  PostgreSQL integration test. Editor verification included 417 unit tests and
  browser suites. A compatible previous API binary is installed for rollback.

The public implementation pin is `43d7fb7b405aa9e5a1d12510ca4e37d412df9fd8`.
The private service records exact releases, infrastructure, backup, checksums, and
run IDs in `docs/operations/task-runtime.md`. GitHub Actions runner allocation was
blocked by account billing, so the ARM release used isolated AWS CodeBuild.

Initial limits and remaining work are explicit. Private images, arbitrary publishing
credential grants, additional platforms, and parallel runs within one project are
not included. Local execution requires Docker; an actual local container smoke was
blocked by the operator's unavailable daemon. No live LLM benchmark or AWS invoice
reconciliation was performed. The phases and acceptance table below preserve the
broader plan, including follow-up scenarios beyond this first release.

## Repository ownership

| Public Jazyk repository | Private jazyk-cloud repository |
| --- | --- |
| Portable settings schema and validation. | Complete editor presentation and browser tests. |
| Runtime request/receipt contract and local adapters. | Provider orchestration, credentials, admission, cleanup. |
| Generation/binding/verification semantics and gates. | Account policy, billing, PostgreSQL, object-store adapters. |
| Embedded prompt payloads, skills, and tool definitions. | GitHub App grants, hosted publication, Cloud operations. |
| CLI, MCP, ACP, local API, browser capability contract. | Subscription UI, operator controls, customer retention policy. |

The public engine must build and run without the private checkout, Cloud account,
or provider credentials. Keep provider-independent code in the engine and host
policy in adapters. Do not copy compiler semantics into the service.

## Phase 0 Review contracts and measure providers

Resolve the first decisions in [the decision list](decisions.md). Keep schema names
and exact default limits provisional until the vertical slice proves them.

Run the [provider experiment](providers.md#provider-selection-experiment) when
implementation is authorized, using bounded test credentials and budget. Record
measured costs, latency, compatibility, fixed lifetime enforcement, and invoice
evidence. Choose one hosted adapter for launch. Avoid implementing three production backends first.

Exit: selected provider with a documented fallback, accepted publication policy,
and an agreed minimal configuration/tool surface.

## Phase 1 Shared runtime and file contracts

Update engine documentation for project settings, hosted execution, tools, sessions,
generation, verification, and CLI/MCP/ACP/API capability responses. Add flat task,
frozen run, allocation, manifest, diff, receipt, lifecycle, and error schemas.
Update prompt payloads in the same docs-first change sequence.

Implement a fake provider and one real local isolated adapter using maintained
container/process/archive libraries. Add async run IDs, bounded events/log reads,
fixed timeouts, cancellation, automatic collection/teardown, and retained file diffs.
Route recorded native commands through finite task runs while preserving an explicit
trusted native adapter. Remove arbitrary live-exec tools from participating toolsets.

Do not remove hosted subprocess denial. Add the injected dispatch seam, with no
capability resulting in a clear unsupported operation. Remove the hosted `pendingCi`
bypasses only when a configured runtime can provide valid receipts.

Exit: the same fixture runs through local and fake-remote adapters with identical
input/evidence semantics, including binary products, retained failure diffs, automatic
shutdown, and revision conflicts. Inspecting results never requires a live container.

## Phase 2 Cloud vertical slice with cost bounds

Document and implement durable run/allocation rows, admission, provider
dispatch, log chunks, private object storage, and the reaper. Add provider credentials
only to the service. Separate compiler worker slots and runtime capacity.

Add runtime reservations, metering segments, rate snapshots, and settlements to the
existing billing ledger before enabling paid execution. Implement customer/global
caps, orphan reconciliation, stop-after-budget behavior, and trial policy.

Build one end-to-end product: upload a project snapshot, render a PDF, collect it,
promote it into Files, and automatically destroy the sandbox. Read/search logs and
inspect the product and file diff after teardown. No GitHub repository is needed.

Exit: API restart, lost responses, cancellation, and provider timeout leave bounded
exposure and recoverable records. A stopped environment is actually confirmed stopped.

## Phase 3 Agent loop and verification

Implement restricted flat task-list mutation and staged support-file scopes.
Integrate the [prompt/tool matrix](agent-contract.md#tool-assignment-by-phase) into
native/text codecs, hosted sessions, local MCP toolsets, and the ACP IDE proxy's
tool injection. Implement the [access contracts](access-paths.md): routing independent
of client transport, structured progress/diffs, explicit cancellation ownership,
and durable result reads after disconnect. Capability blocks include effective
bounds and current grants. Update payloads, never duplicate them in Cloud or ACP.

Route binding checks, generation, ledger builds, and programmatic tests through
ordinary task IDs and the shared run adapter. Add optional report globs and immutable
report envelopes. Bind exact case identities in the existing ledger; no separate
suite/test settings. Distinguish completed passes/failures, skipped, proven not-run,
and unknown cases. Guard aggregate verification against missing required evidence.
Keep LLM verdicts separate. Add artifact inspection or clear capability errors.

Restore task-local disposable cache before commands; save only on whole-command
success with no cancellation. Keep the prior cache on failure. There are no custom
cache keys, profiles, step lists, or intermediate save checkpoints in the first UI.

Exit: a model creates a useful task command/configuration from prose, runs it,
diagnoses a failure, repairs owned files, verifies, and retains the product within
bounds. Pure
LLM tests allocate no sandbox unnecessarily. The model diagnoses and repairs from
retained results between attempts, with no container kept for its next command.

## Phase 4 Task picker and existing editor

Replace the GitHub-only settings form with a flat task list and command editor.
Expose GitHub sync as a managed task backed by connection settings. Add the split
button, Runs beside Files, history, logs, retained Changes, products, and test
evidence to the existing tabs. People start tasks and cancel runs at any time;
there is no command console or live file browser. Implement local and Cloud
responses through the same presentation path, and plain-text/path
result fallbacks for MCP/ACP clients without the Cloud UI.

Preserve GitHub ownership checks, expected heads, unknown-outcome reconciliation,
and retained attempts. Extend binary handling deliberately. Bind publication grants
to the accepted policy; do not let generated settings enable themselves.

Keep existing Actions attempts and their logs/evidence readable. Offer a deliberate
conversion of inline commands directly into task commands, with optional image
overrides. Do not require creating script files. Historical definitions retain their
original backend. Do not delete existing remote workflows
or silently reinterpret them. A retained Actions adapter can stay explicitly legacy
until an intentional removal decision.

Exit: zero/one/many tasks behave correctly, generic command execution works without
GitHub, sync needs no sandbox, and users can inspect and stop work from existing tabs.

## Phase 5 Launch gates and operational handoff

Measure the actual provider bill and customer settlements under representative
workloads. Review retained-storage growth, cache hit rates, log volume, startup
cost, collection/teardown latency, repeated fresh-run cost, and failed-run recovery.
Tune class sizes and retention.

Document provider outage procedures, emergency runtime admission shutdown, active
sandbox enumeration/termination, stuck reservations, expired credentials, object
cleanup, and customer export. Existing API availability should not depend on a
healthy runtime provider.

Perform staged rollout with account-scoped feature flags and small concurrency.
Release work in the correct repository. Engine binary changes end with
`cargo build --release`; Cloud/editor checks and deployment stay in `jazyk-cloud`.
Planning changes alone require no compiler rebuild or deployment.

## Acceptance scenarios

| Scenario | Required outcome |
| --- | --- |
| Book with PDF and EPUB | Both retained and downloadable after sandbox deletion. |
| Presentation generation | Model inspects available rendered output against criteria, not just path existence. |
| Single selected unit test | State whether the command filters or runs a broader suite; only actually reported mapped cases receive conclusive new evidence. |
| Test-suite failure after build | No new cache saved; previous cache preserved; reported cases retain their individual outcomes. |
| Command succeeds with zero tests or a missing report | Command success does not verify the selected criteria. |
| Cancellation after some cases completed | Completed valid evidence retained; interrupted/unreported cases do not pass. |
| Old report left among generated inputs | Cleared before execution; cannot establish fresh passing evidence. |
| Passing headline with an unknown required criterion | Requirement remains unverified; no aggregate false pass. |
| Task changes source/tests while executing | Diff retained; rerun resulting candidate before claiming it verified. |
| File-only LLM test | No runtime allocated; verdict keeps artifact/fact hashes. |
| Local MCP coding agent | Same execution and evidence contract as hosted generation. |
| ACP IDE with no terminal capability | Submit the same task, observe updates, inspect retained logs/diffs, and cancel. |
| MCP/ACP client selects hosted execution | Same account admission and billing as the Cloud editor. |
| Client disconnects during execution | Action still finishes or times out and tears down; reconnect reads its durable result. |
| ACP session or compiler operation explicitly cancelled | Its child runs cancel; independently owned deliveries remain separate. |
| No local container engine | Clear unavailable state, explicit alternatives, no silent host shell. |
| Source edited during a run | Results stay tied to old revision; import conflicts preserve newer source. |
| Command writes graph/settings directly | Protected state rejected; authorized edits use normal gates. |
| Malicious log instructions | Agent treats them as data; capabilities cannot expand. |
| Infinite process or detached child | Fixed deadline and supervisor terminate it; reserved usage bounds exposure. |
| Successful or failed finite action | Logs and eligible diffs collected, environment torn down without another model call. |
| Request to append commands, attach shell, or extend deadline | Rejected; edits and changed limits apply only to a new attempt. |
| Cancellation during an executing command with children | Signal/kill whole tree, confirm allocation termination, settle actual use. |
| Cancellation while queued, provisioning, or collecting | Stop intent prevents new work/promotion; late allocations terminated; partial results labeled. |
| Cancellation races completion or promotion | Atomic finalization decides result; committed files and external writes are not rolled back. |
| Cancel called twice or after allowance exhaustion | Idempotent and permitted; never starts work or incurs a second reservation. |
| Provider dispatch/start response lost | Reconcile existing operation before any repeat. |
| Provider stop response lost | `Stopping`/`Unknown`, pending exposure, eventual reconciliation. |
| GitHub remote edited concurrently | Conflict; no force-push or remote-only deletion. |
| Two managed GitHub sync tasks | Separate repository/branch grants and external outcome records. |
| Project deletion while running | Revoke transfers, stop execution, clean content, retain financial receipts. |
| Log/report/artifact/diff quota reached | Bounded resource use and explicit missing evidence or partial diff, no silent success. |
| Different tenants use the same cache key | No content or existence leak across namespaces. |
| Model and runtime reserve simultaneously | Shared caps hold; no double spending or double billing. |
| Renewing subscription during a run | Consumption and unresolved reservations remain correctly attributed. |

Use deterministic tests for contracts and accounting; provider-mocked integration
tests for failures; bounded real sandbox smoke tests for actual behavior; editor
tests for interaction; Cloud/MCP/ACP conformance tests; model benchmarks for tool use
and product quality. Passing one category does not establish the others.

## Work deliberately left open

Additional providers and sizes, pricing/retention tuning, publication autonomy,
parallel runtimes, private images, native Apple/Windows builds, image-building
services, continuous Git sync, and custom customer providers remain decisions or
later capabilities. Mark unsupported requirements explicitly. The flat task/command
contract should survive their later addition.

Persistent environments, interactive terminals, live exec, and deadline extensions
are outside the selected design, not unresolved v1 options.
