# Decisions and alternatives

Status: review list for the [runtime and delivery plan](README.md). Recommendations
are not accepted decisions unless recorded as such here.

## Recorded from this discussion

- The owner wants generic document-defined delivery, with ordinary generated
  scripts and simple environment settings.
- The runtime must serve generation, test generation, programmatic tests, and
  delivery, with enough agent tools and prompting to manage execution autonomously.
- Local and hosted use should share one path as far as practical. Cloud execution
  is the commercial priority.
- The owner selected managed isolated runtimes with little operational work as
  the preferred launch direction.
- The owner selected Runs beside Files, using the existing tabs.
- The owner selected finite actions: execute a specific script or step sequence,
  collect logs/file changes, then shut down the container. No on-demand commands
  in an open environment, manual consoles, or interactive shells.
- Execution must be accessible through Cloud, local MCP inside a coding agent,
  and ACP inside an IDE. These use the same action contract.
- Delivery autonomy is undecided. The owner requested options.
- This is a planning worktree. No runtime or UI implementation is requested yet.

## Delivery autonomy

All options let an agent inspect logs, repair its own files, run ordinary builds
and tests, choose an admitted size, and stop work within budget. The difference is
who may initiate an external write and how broadly that authority persists.

| Option | Example | Benefit | Cost or risk |
| --- | --- | --- | --- |
| A. Click for each external publication | Agent builds the EPUB; user clicks Publish for that candidate. | Simple, precise control of externally visible changes. | Interrupts unattended publishing and frequent sync. |
| B. Saved publication scope | User enables this project's push to repository X/branch Y; later agent pushes within that scope. | Good autonomy after setup, without repeat confirmations. | Needs real credential/destination enforcement and clear grant invalidation rules. |
| C. Broad project publishing authority | User grants a project its chosen credentials and lets docs drive all runs within budget. | Maximum autonomy for trusted workflows. | Arbitrary generated scripts can use all granted privileges; prose is not an enforcement boundary. |
| D. Per-delivery mixture | Automatic GitHub sync and preview builds; click-to-publish for production or App Store. | Matches different consequences inside one project. | More settings, though defaults can keep the common path small. |

Recommendation for review: B as the underlying mechanism, with D exposed as a
simple per-delivery policy. A remains available for sensitive publication. C is a
deliberate trusted-project mode, not an accidental consequence of changing TOML.

There are two variants of B:

- Definition-pinned approval: permission names script/configuration hashes. Changes
  that alter execution require renewal. Precise but potentially noisy during work.
- Capability-scoped approval: permission names destinations, secret scopes, resource
  bounds, and allowed action purposes. Script changes within that envelope can run
  automatically. Better for an active generation loop, but only as narrow as the
  credentials and network mechanisms can actually enforce.

A token that can modify an entire account does not become repo-scoped because the
UI says so. Use narrowly scoped or short-lived credentials where available. Where
that is impossible, explain the effective scope and let the user choose A or C.
Build/test sandboxes have no publication credentials by default. The GitHub shortcut
can enforce repository and branch in its host adapter without giving a token to shell.

Question for review: should automatic GitHub sync be the first saved scope, while
arbitrary script publication initially uses per-delivery grants or a click?

## Configuration breadth

| Option | Tradeoff |
| --- | --- |
| Script only, plus products/caches | Smallest form. Cache can be saved only at whole-script success. |
| Script by default, optional ordered steps | Supports build-cache-save-test with visible phases. Recommended. |
| Full CI graph | Parallel jobs, matrices, expressions, dependencies. Powerful, substantially larger product. Defer. |

The plan uses the middle option. A project sees only name, image, and script until
it needs more. Consider shipping reusable tasks/steps after the first vertical slice
if the simpler form already solves most projects.

## Runtime count

- One active finite run per project: simplest file ownership and cost visibility.
  Recommended first release.
- Several isolated runs with one importer: enables parallel tests and independent
  deliveries. Costs more and needs revision conflicts in the UI.
The records support several concurrent finite runs eventually. Each gets a fresh
environment; the canonical workspace is independent of its filesystem. Persistent
development machines were considered in the first draft and are superseded by the
owner's finite-run decision.

## Local execution

- Docker/Podman by default gives the closest Linux parity and file isolation.
- Native execution is easier for existing coding agents and platform-specific tools,
  but has host privileges and different dependencies. Keep it an explicit trusted mode.
- Cloud-only execution would reduce adapter work, but weakens accountless local use
  and makes the open source workflow depend on a paid service. Do not recommend it.

Question for review: require a local container engine for the default path, with
native execution an explicit fallback, or initially keep native as the local default?
The draft recommends the former; either uses the same receipts and file contracts.

## Files and visible state

Show logs and retained file changes for each run. Products enter Files through
successful checkpoints and revision-checked import. Cache checkpoints show metadata;
scratch is discarded. There is no live container filesystem view. Preserve eligible
source/test diffs even on failed runs so the agent can inspect them after teardown.

Question for review: should declared partial products be retained on failure by
default, or only under a policy selected before submission? The draft favors
logs/reports/eligible diffs always and bounded opt-in partial-product retention.
The choice must be made before the run; teardown prevents retroactive recovery.

## UI and action control

Settled: Runs beside Files in the existing tabs, showing logs and retained changes.
People and agents start defined actions and can cancel. The host executes the frozen
sequence and shuts down automatically. There is no console, terminal, attach, or
live timeout/resource change. To repair or investigate, edit the project/action and
start a new attempt. This also applies through MCP and ACP, not just the editor.

Publication autonomy remains a separate decision. Finite execution does not decide
whether an agent may publish to an external destination without a user click.

## Cost and retention

- Unified usage allowance is recommended, with model/runtime breakdowns. Separate
  included runtime minutes are easier to market but add a second exhaustion model.
- Cost-based classes are recommended. A flat per-minute rate is simpler but invites
  poor margins when sizes differ.
- Include modest storage quotas initially. Storage billing can follow measured use.
- Proposed 30-day logs and 7-day cache expiry are review values. Product retention
  is independent. The current product should survive cache eviction.
- Keep the existing trial token cap and add bounded runtime use under the supplier
  ceiling, or adopt a unified cost-only trial as a separate product decision.

## Other ideas worth keeping open

- A user can use their own Cloud provider account later through the same adapter,
  with clearly separate billing and capabilities.
- A delivery can create a product without publishing it anywhere. Downloading the
  PDF is a complete useful delivery.
- A preview can be a retained rendered product. Live preview hosting is outside
  this finite-run design and would need a separate explicit product decision.
- A project can generate its own external CI workflow as an ordinary product while
  Jazyk still uses direct runtimes for fast repair/test loops.
- Publication can use separate build and publish sandboxes, passing only sealed
  products between them. This reduces secret exposure without inventing product types.
- Cache archives and prepared image layers can accelerate fresh attempts. Retaining
  running processes or resuming whole environments is outside the selected model.

## Suggested review order

First settle autonomy, configuration breadth, and the one-sandbox default. Then
review the concrete agent sequences and UI. Choose a provider after the small
experiment measures real costs and lifetime guarantees. Tune quotas and retention
from those measurements rather than treating this draft's numbers as commitments.
