# Decisions and alternatives

Status: discussion record, revised 2026-10-15. Recommendations remain proposals
unless explicitly accepted below.

## Accepted from this discussion

- One flat list of tasks with IDs, names, descriptions, and shell commands. A command
  may invoke a script, but no script file is required.
- Build, test, delivery, and other purposes share this list. The model chooses by
  meaning. Remove separate runtime/cache profiles, test/delivery wrappers, task
  references, and ordered step configuration from the draft.
- Optional file globs classify retained products, disposable cache, and test reports.
  Existing bound tests still need individual passed/failed/not-run evidence.
- Finite runs execute a frozen command, collect results, and shut down. No interactive
  environment, live shell, command appending, or deadline extension.
- Users and authorized agents can cancel at any time, including during execution.
- Cloud, local MCP in a coding agent, and ACP in an IDE use the same tools/contracts.
- Managed isolated runtimes with little operational work are the preferred Cloud
  launch direction. Cloud is the commercial priority; local remains accountless.
- Runs belongs beside Files, using existing tabs for logs and retained changes.
- The owner delegated the publication policy decision. Use saved per-task grants
  bound to the exact task definition, including its command and image. A person
  creates the grant. Editing the definition requires a new grant.
- Implementation, merge to master, and production deployment were authorized on
  2026-10-15. AWS Fargate is selected for hosted execution. Plans remain under
  `./plans/runtime-delivery/` in the repository checkout.

## Simplification tradeoffs

A task is the definition. A run is one execution. Delivery is a task's purpose,
not a separate object. Programmatic versus LLM remains a distinction in verification
evidence, not in task configuration. Short commands such as `mvn test` are first-class.

The earlier recommendation for optional ordered steps is superseded. Command exit
zero is the cache-save boundary. If `mvn install` builds successfully and then fails
tests, no new cache is saved. This reduces configuration and orchestration complexity.
The existing cache survives. A project can use separate ordinary tasks when useful,
without task references or automatic dependency composition.

Use report globs plus existing ledger mappings for individual test results. Missing
reports and proven not-run cases remain distinct. Update aggregate verification so
an old/headline pass cannot hide currently inconclusive required criteria.

Keep the optional no-container GitHub sync as a managed row in the same task picker,
backed by connection settings. This is a narrow implementation exception, not a
second user-authored task language. An ordinary `git push` stays a shell command.

## Delivery autonomy

All options let the agent choose tasks, repair owned files, inspect logs, and cancel
within granted budgets. The decision is who authorizes external writes.

| Option | Example | Tradeoff |
| --- | --- | --- |
| A. Click for each publication | Agent builds the EPUB; user publishes that candidate. | Precise control; interrupts unattended work. |
| B. Saved destination scope | Authorize this project's pushes to a repository/branch. | Autonomous within a durable grant; needs enforceable scopes. |
| C. Broad project authority | Trusted project gets chosen credentials and runs within budget. | Maximum flexibility; arbitrary code can exercise those full privileges. |
| D. Different grants per task | Automatic sync, click-to-publish production. | Matches different consequences; adds some policy settings. |

Selected for this release: saved credentials per task, bound to the exact definition
hash. This combines B with D without adding task kinds. The agent may run the saved
task repeatedly within account limits. It cannot create a grant. A definition edit
invalidates the old grant, and revocation cancels active runs using it. Project
source edits do not invalidate a grant. The selected credentials still define the
actual external privileges available to the command.

A grant can pin definition/input hashes, requiring renewal when behavior changes,
or bound capabilities/destinations while allowing command edits. The former is
precise but interrupts iteration. The latter supports autonomy, but is only as
narrow as the real credentials and network controls enforce.

A task description saying "test" cannot make a broadly privileged command safe.
Use narrow/short-lived credentials when available. The managed GitHub handler can
enforce repository/branch without exposing a token to project shell code.

Saved environment secrets and private-registry credentials are included. Managed
GitHub sync keeps its existing repository and branch authorization. Per-run human
approval and broader project credentials remain alternatives, not the default.

## Runtime count and local execution

Recommend one active run per project initially. It simplifies import conflicts and
cost visibility. Separate projects can run concurrently. Later parallel runs need
independent filesystems and one revision-checked importer.

Recommend Docker/Podman for the default local adapter. It gives useful Linux parity
and file isolation. Native execution remains an explicit trusted alternative for
existing agents and platform-specific tools. Cloud-only execution would reduce
adapter work but conflict with accountless open source use.

Review question: ship both local adapters initially, or prioritize containers and
add trusted native execution after the first release?

## Files and visible state

Logs and eligible source/test diffs survive failures. Successful products enter
Files after collection and checked import. Cache and scratch do not appear as
canonical files. There is no live filesystem or intermediate checkpoint view.

Partial product retention remains a small policy choice: retain bounded failed
outputs under the run by default, or require an opt-in set before dispatch. The
draft allows the host to select this policy, always labeling incomplete outputs
and never replacing a successful product automatically. Teardown prevents later
recovery of unretained bytes.

## Cost and retention

Recommend a common usage allowance with model/runtime breakdowns and cost-based
resource classes. Separate included runtime minutes are easier to market but add
another exhaustion rule. Flat prices across sizes create margin risk.

Proposed 30-day logs and 7-day cache expiry need measurement. Current products have
a separate durable lifetime and storage quota. Preserve the trial token cap plus
a bounded supplier-cost runtime allowance, or change to a unified cost trial as
an explicit product decision.

## Ideas to preserve without growing the first schema

- Customer-owned provider accounts through the same adapter later.
- Downloadable products as complete deliverables without external publication.
- Ordinary tasks that generate external CI workflows as retained products.
- Separate build and publish tasks when credentials should see only sealed outputs.
- Image preparation and dependency caching to reduce fresh-container startup cost.

Live preview hosting, full CI graphs, interactive containers, matrices, automatic
task dependencies, and continuous two-way sync remain outside the first release.

## Suggested review order

The simple task model and cancellation are settled directions. Next decide publication
autonomy and the local launch adapter. Review the concrete agent loop and test result
semantics, then choose a provider after the bounded experiment. Tune sizes, costs,
and retention from measurements rather than treating draft values as promises.
