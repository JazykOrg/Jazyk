# Implementation sequence

Status: proposed work breakdown. This planning task does not implement these phases.
Each accepted behavior enters the owning repository's `docs/` before code.

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
measured costs, latency, compatibility, lease semantics, and invoice evidence. Choose
one hosted adapter for launch. Avoid implementing three production backends first.

Exit: selected provider with a documented fallback, accepted publication policy,
and an agreed minimal configuration/tool surface.

## Phase 1 Shared runtime and file contracts

Update engine documentation for project settings, hosted execution, tools, sessions,
generation, verification, and local/API capability responses. Add the request,
command, manifest, receipt, lifecycle, and error schemas. Update prompt payloads in
the same docs-first change sequence.

Implement a fake provider and one real local isolated adapter using maintained
container/process/archive libraries. Add async run IDs, bounded events/log reads,
timeouts, cancellation, and file import gates. Route native command execution through
the capability while preserving an explicit trusted native mode.

Do not remove hosted subprocess denial. Add the injected dispatch seam, with no
capability resulting in a clear unsupported operation. Remove the hosted `pendingCi`
bypasses only when a configured runtime can provide valid receipts.

Exit: the same fixture runs through local and fake-remote adapters with identical
input/evidence semantics, including binary products and revision conflicts.

## Phase 2 Cloud vertical slice with cost bounds

Document and implement durable run/sandbox/command rows, admission, provider
dispatch, log chunks, private object storage, and the reaper. Add provider credentials
only to the service. Separate compiler worker slots and runtime capacity.

Add runtime reservations, metering segments, rate snapshots, and settlements to the
existing billing ledger before enabling paid execution. Implement customer/global
caps, orphan reconciliation, stop-after-budget behavior, and trial policy.

Build one end-to-end product: upload a project snapshot, render a PDF, collect it,
promote it into Files, read/search logs, stop the sandbox, then inspect the product
after the sandbox is destroyed. No GitHub repository is needed.

Exit: API restart, lost responses, cancellation, and provider timeout leave bounded
exposure and recoverable records. A stopped environment is actually confirmed stopped.

## Phase 3 Agent loop and verification

Implement restricted execution-settings mutation and staged support-file scopes.
Integrate the [prompt/tool matrix](agent-contract.md#tool-assignment-by-phase) into
native/text codecs, hosted sessions, and MCP. Update payloads, never duplicate them
in Cloud. Capability blocks include effective bounds and current grants.

Route binding checks, generation commands, ledger builds, and programmatic tests
through the shared adapter. Add suite/criterion selection and immutable report
envelopes. Keep LLM verdicts and programmatic evidence separate. Add artifact
inspection support or explicit capability errors for built non-text products.

Implement cache restore/save checkpoints and a small optional sequential step form.
Measure dependency cache versus build cache behavior. A failing test must not lose
a previously sealed build cache or become a pass through bulk workflow success.

Exit: a model creates a useful script/configuration from prose, runs it, diagnoses a
failure, repairs owned files, verifies, and retains the product within bounds. Pure
LLM tests allocate no sandbox unnecessarily.

## Phase 4 Deliveries and existing editor

Replace the GitHub-only settings form with generic script/task definitions and the
GitHub sync shortcut. Add the split button, Runs/history, logs, live files, products,
test evidence, and command console to the existing editor. Implement local and
Cloud capability responses through the same presentation path.

Preserve GitHub ownership checks, expected heads, unknown-outcome reconciliation,
and retained attempts. Extend binary handling deliberately. Bind publication grants
to the accepted policy; do not let generated settings enable themselves.

Keep existing Actions attempts and their logs/evidence readable. Offer a deliberate
conversion of inline commands into scripts and runtime definitions. Historical
definitions retain their original backend. Do not delete existing remote workflows
or silently reinterpret them. A retained Actions adapter can stay explicitly legacy
until an intentional removal decision.

Exit: zero/one/many deliveries behave correctly, generic delivery works without
GitHub, sync needs no sandbox, and users can inspect and stop work from existing tabs.

## Phase 5 Launch gates and operational handoff

Measure the actual provider bill and customer settlements under representative
workloads. Review retained-storage growth, cache hit rates, log volume, startup
cost, idle time, and failed-run recovery. Tune class sizes and retention.

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
| Single selected unit test | Only requested binding receives fresh evidence; skipped/missing cases do not pass. |
| Test-suite failure after build | Build cache retained at checkpoint; failed cases remain failed. |
| File-only LLM test | No runtime allocated; verdict keeps artifact/fact hashes. |
| Local MCP coding agent | Same execution and evidence contract as hosted generation. |
| No local container engine | Clear unavailable state, explicit alternatives, no silent host shell. |
| Source edited during a run | Results stay tied to old revision; import conflicts preserve newer source. |
| Command writes graph/settings directly | Protected state rejected; authorized edits use normal gates. |
| Malicious log instructions | Agent treats them as data; capabilities cannot expand. |
| Infinite process or abandoned terminal | Deadline and reaper terminate it; reserved usage bounds exposure. |
| Cancellation with spawned children | No surviving billable process tree. |
| Provider create/exec response lost | Reconcile existing operation before any repeat. |
| Provider stop response lost | `Stopping`/`Unknown`, pending exposure, eventual reconciliation. |
| GitHub remote edited concurrently | Conflict; no force-push or remote-only deletion. |
| Two differently targeted sync actions | Separate repository/branch grants and checkpoints. |
| Project deletion while running | Revoke transfers, stop execution, clean content, retain financial receipts. |
| Log/report/artifact quota reached | Bounded resource use and explicit missing evidence, no silent success. |
| Different tenants use the same cache key | No content or existence leak across namespaces. |
| Model and runtime reserve simultaneously | Shared caps hold; no double spending or double billing. |
| Renewing subscription during a run | Consumption and unresolved reservations remain correctly attributed. |

Use deterministic tests for contracts and accounting; provider-mocked integration
tests for failures; bounded real sandbox smoke tests for actual behavior; editor
tests for interaction; model benchmarks for tool use and product quality. Passing
one category does not establish the others.

## Work deliberately left open

Provider and size selection, exact pricing/retention, autonomy defaults, full PTY,
parallel runtimes, private images, native Apple/Windows builds, image-building
services, continuous Git sync, and custom customer providers remain decisions or
later capabilities. Mark unsupported requirements explicitly. The generic script
contract should survive their later addition.
