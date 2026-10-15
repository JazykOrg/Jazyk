# Generation and verification

Status: revised for one flat task list. See [configuration](configuration.md) and
[agent prompts](agent-contract.md).

## Existing responsibilities stay in place

Compilation reconciles prose and derives graph/ledger work. Binding identifies or
generates tests. Generation implements requirements. Verification records evidence.
Tasks give those operations a common way to execute a shell command.

A task is not a compiler goal. The model chooses a task from its name and description
while resolving the goal the harness assigned. It does not create, route, or
prioritize goals. Pure extraction and graph judgment need no runtime by default.

Build and delivery obligations use existing graph node kinds and provenance. Keep
requirement-to-task/support-file references in generation metadata. Changes to those
requirements, commands, scripts, or environments invalidate affected evidence through
the existing binding/generation machinery. Shared support files belong to the
whole deliverable, not an arbitrary incidental entity.

## Source and test generation loop

1. Read the assigned goal, relevant prose, bound tests, flat task list, and effective
   runtime capabilities. Choose by name and description, not a required task type.
2. Reuse existing commands and toolchain conventions. Write source/tests, and create
   or update a task only if the needed operation is missing. A short command does
   not need a wrapper script.
3. Submit `run_task` with the task ID and staged input revision. Optional selection
   identifies the criteria/cases whose evidence is sought.
4. The host snapshots inputs, restores disposable cache, runs the command, collects
   available logs/reports/changes, and destroys the container.
5. Inspect the results. Repair owned files and run another task if useful and within
   the cumulative budget. Every attempt uses a fresh container.
6. Satisfy the existing generation/verification gate with actual evidence and inspect
   retained products where required. Record generation with the original fact hash.

An expected assertion failure can be useful during test binding. Do not change the
product just to make a newly written test green in that phase. A failed run can
retain useful changes and valid completed test results. It saves no new cache.

## Existing status belongs to the ledger

Jazyk already has requirement rows and per-criterion tests in its
[generation ledger](../../docs/consumers/gen.md#status-is-derived-never-stored).
Bindings record test kind, artifact, exact name, criterion hash, and command today.
Verdicts are `none`, `pass`, or `fail`. Requirement status is derived from those
verdicts, artifact existence, and requirement/test/source hashes.

Feed runtime results through this ledger API. The graph and editor consume the
derived status; do not add authored graph facts saying "this test passed". Add
run references and execution reasons to evidence, preserving the existing graph
identity and provenance model.

Generated programmatic bindings additionally reference a task ID and exact reported
case identity. The task contains the command once. The ledger contains the criterion
mapping once. An optional exact selector remains binding metadata, not another
suite/test configuration table. Legacy per-row commands can become ordinary tasks
or use a temporary compatibility adapter while the recorded binding is updated.

## Selection and results

Any task can emit test reports. `reports` supplies project-relative globs for JUnit
XML in the first release. No `type = test`, suite definition, or separate delivery
wrapper is required. A task invoked from generation, the picker, MCP, ACP, or a
publication workflow imports results the same way.

Before dispatch, freeze the expected bound cases and their requirement/criterion,
source, test, and task hashes. Default expectations come from programmatic bindings
that reference that task. Existing semantic selectors for entity, requirement,
criterion, kind, all, and force resolve into ledger rows.

The default task runs its declared command unchanged. A request for one criterion
may execute a broader suite. State that clearly; do not claim the command filtered
execution. All unambiguously mapped, current cases actually reported may receive
fresh evidence. Report extra unbound cases in the run without assigning graph IDs.

For efficient filtering, a generated command or script may read the host's optional
`JAZYK_TEST_SELECTION` JSON file. It contains literal case IDs/selectors and whether
the request is all. It is input data, not shell text to evaluate. The host does not
append framework-specific arguments to arbitrary commands. Otherwise the agent
can define a focused task such as `mvn -Dtest=CatalogTest test` and bind it to those
cases. These are ordinary tasks; filtering never requires a task-composition DSL.

`jazyk test` and the existing semantic verification API can select the bound tasks,
execute each once for the candidate, and collect evidence. They are convenience
orchestration over the same `run_task` contract, not another runner or task type.
LLM verification packages remain a separate judgment operation.

Match report identities exactly to ledger bindings, using a qualified identity such
as JUnit `classname::name` and explicit parameterized-case identities where needed.
Never fuzzy-match a display name to a criterion. Duplicate or ambiguous identities
need rebinding/runner repair. Frameworks whose reports cannot distinguish the bound
cases need a generated adapter before they can establish those verdicts.

If several cases or parameterized subtests jointly cover one criterion, keep their
expected identities in binding metadata and aggregate only when every required
member has current evidence. Running one subtest cannot mark the entire criterion
passed. For dynamically discovered cases, require a complete runner enumeration
before claiming complete coverage; a partial report cannot define its own smaller
expected set.

| Observed result | Ledger and UI behavior |
| --- | --- |
| Bound case passed | `pass`, with the producing run and exact tested hashes. |
| Bound case assertion failed | `fail`, with assertion evidence. |
| Explicitly skipped case | `none`, reason `skipped`; never a pass. |
| Proven not started, e.g. setup failed or queued run cancelled | `none`, reason `not-run`. |
| Expected case absent from a partial/missing report | `none`, reason `no-result`; do not assert it never executed. |
| Case interrupted by timeout/cancellation | `none`, reason `interrupted`, unless a valid completed result was already sealed. |
| Ambiguous, malformed, or stale evidence | `none` for affected expected cases, with the import error. |
| Unselected and unreported case | Keep its previous evidence, subject to normal staleness checks. |

These reasons describe the latest attempt. `Never ran` means there is no prior
execution evidence, not that the case is absent from this report. Keep prior history
visible, but do not carry yesterday's passing verdict forward as the result of an
inconclusive attempt that explicitly expected this case. Proven completed results
from a cancelled run can survive as individual evidence. The run still says cancelled.

A build/setup failure is one runner failure, not a failing assertion for every
requirement. Zero collected tests cannot verify the requested criteria. A successful
command with missing reports says `Command succeeded; verification incomplete`.
A nonzero suite exit may still contain some valid passes and assertion failures.

Retain the existing exact-command verdict contract for legacy bindings that execute
one known test, with current runner-failure rules. It cannot be used to fan a bulk
command's exit code out into passing verdicts for a suite. New generated framework
tests should emit per-case reports; when available, use runner counts to reject
zero-test selections even for an exact command.

## Fresh evidence and aggregation

Collect reports even after a failed command or cancellation, within a bounded
collection window. Reports are operational evidence, not products or cache. Clear
previous generated report outputs in the sandbox before execution and never restore
them from cache. Reject broad report patterns that overlap authored input/test files.
Seal the fresh report with the host's run/input manifest; a timestamp alone does
not establish freshness. Validate parsers, byte/case limits, and archive/XML handling.

A partial valid report can establish completed cases but cannot make the entire
selection pass. Truncated/unparseable evidence leaves affected cases unknown.
A requirement with criteria becomes verified only when all required bound criteria
have current passing evidence. A passing headline cannot hide skipped or unknown
criteria. Preserve the existing stale, failing, partial, and unimplemented precedence,
then add an incomplete-evidence guard before any aggregate `verified` result.

The existing implementation aggregates pass/fail counts and a headline verdict.
The new all-required-criteria guard must be specified in docs and tested explicitly;
it is a required change, not an assumption about today's behavior.

Each accepted receipt records project/set, input revision/digest, graph generation,
requirement/criterion hashes, source/test hashes, task/command/image digest, expected
and observed cases, outcomes, logs/reports, and run timestamps. The host stamps this
envelope. Project reports cannot claim their own trusted revision identity.

A run can rebaseline test/source hashes only to the bytes actually exercised, under
the existing ledger rules. It never rewrites requirement or criterion statement
hashes. If the project changed meanwhile, retain old evidence as historical/stale;
do not stamp newer files as tested. If the command changes source or tests, a final
diff alone does not prove which version was exercised. Withhold a current pass for
changed inputs until a fresh run over the resulting candidate establishes it.

## LLM verification

LLM tests keep their criteria files, `begin_verification`, and `record_verdict`
contract. Judge exact requirements and retained artifacts with their fact hashes.
Reading source or prose requires no container. If a PDF, presentation, or rendered
page must be built first, choose its ordinary task, retain the output, and inspect
it after teardown. Lack of a binary/image inspection capability is explicit;
file existence is not proof of visual quality.

The two test kinds remain in the ledger because their evidence differs. They do
not become two task-definition kinds. Changing a runtime report cannot forge an
LLM verdict, and an LLM summary of logs cannot forge a programmatic pass.

## Tasks that publish

A task may build, test, and publish in one command, or the agent may run separate
tasks based on their descriptions and documented obligations. There is no task
DAG. Rebuild needed outputs or consume explicitly retained products in a later run;
never assume the previous container still exists or its cache will be available.

Which tests must pass before publication comes from prose and granted user policy.
Do not invent a universal release gate. Where enforced, check current ledger
evidence against the frozen candidate before releasing publication credentials.
A command with credentials from startup cannot enforce a protected prepublication
gate by its description alone; use a separate publication task for that policy.

Record any release URL, remote revision, or digest available. Command success is
not proof of destination correctness. Cancellation stops remaining work but cannot
undo an already completed external write. Preserve `outcome unknown` when the
response was lost, and inspect the destination before any retry.
