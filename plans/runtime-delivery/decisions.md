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
| Script only, plus products/caches | Smallest form. Cache can be saved only at whole-script success unless using agent checkpoints. |
| Script by default, optional ordered steps | Supports build-cache-save-test with visible phases. Recommended. |
| Full CI graph | Parallel jobs, matrices, expressions, dependencies. Powerful, substantially larger product. Defer. |

The plan uses the middle option. A project sees only name, image, and script until
it needs more. Consider shipping reusable tasks/steps after the first vertical slice
if the simpler form already solves most projects.

## Runtime count

- One active sandbox per project: simplest file ownership, cost visibility, and
  agent/human handoff. Recommended first release.
- Several isolated runs with one importer: enables parallel tests and independent
  deliveries. Costs more and needs revision conflicts in the UI.
- One persistent development machine per project: convenient live files and shell,
  but idle cost, state drift, and recovery become core concerns.

The records support several runs eventually. Start with one active sandbox without
making the canonical workspace equal to that sandbox's filesystem.

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

Recommended: promote products into Files at successful checkpoints; inspect cache
and scratch in the live run. Alternatives are a merged live filesystem view or
continuous synchronization. Those make concurrent edits and partial products harder
to reason about. A refreshed live run browser gives visibility without pretending
every temporary file is durable.

Question for review: should failed-run partial products be retained automatically
within a small quota, or only when the agent/user requests them? The draft favors
logs/reports always and bounded explicit partial-product retention.

## UI and manual control

Two questions have been presented for the owner's preference:

- Runs beside Files, a virtual Runs directory, or a bottom panel.
- Bounded manual command console first, full PTY immediately, or agent-only execution.

Until answered, the draft uses Runs beside Files and a bounded console. These are
provisional defaults, not recorded approvals.

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
- A preview URL is a short-lived runtime capability, not an implicit hosting product.
- A project can generate its own external CI workflow as an ordinary product while
  Jazyk still uses direct runtimes for fast repair/test loops.
- Publication can use separate build and publish sandboxes, passing only sealed
  products between them. This reduces secret exposure without inventing product types.
- Optional provider snapshots can accelerate return to a project, but must remain
  disposable cache. Do not make them the only home of source or products.

## Suggested review order

First settle autonomy, configuration breadth, and the one-sandbox default. Then
review the concrete agent sequences and UI. Choose a provider after the small
experiment measures real costs and lifetime guarantees. Tune quotas and retention
from those measurements rather than treating this draft's numbers as commitments.
