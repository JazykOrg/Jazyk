# Generation and verification

Status: proposal for the [shared runtime](runtime.md). Prompt and tool details are
in [Agent contract](agent-contract.md).

## Keep the existing compiler responsibilities

Compilation derives graph and ledger work. Binding generates or identifies tests.
Generation implements the product against those tests. Verification records
evidence against requirements and criteria. Delivery runs a named action against a
selected candidate. Runtime execution is a capability used by those operations.

Do not create a container-management goal kind, let the model schedule goals, or
make every documentation change start a container. Pure extraction and graph
judgment need no shell by default. Build/test obligations can use the existing
generation and verification gates.

Delivery obligations in prose use the existing graph node kinds and provenance.
Generated scripts are support files shared by the deliverable. An incidental entity
must not become the sole owner of a whole-project build or publishing script.

Record requirement-to-entrypoint/settings references in generation metadata. A
change to those requirements, their scripts, or the selected runtime invalidates
the corresponding generated support evidence and verification. The harness derives
that work through the existing generation/binding machinery. The model does not
have to remember to revisit delivery after an unrelated source edit. Define these
dependencies in the docs before adding the gate; do not rely on prompt text alone.

## The source and test generation loop

1. Load the task package, bound tests, delivery requirements, current action
   definitions, and effective runtime capabilities.
2. Reuse the established language, dependencies, build, and test conventions.
   Add a runtime/script definition only when absent or inadequate.
3. Write the implementation or test files through existing gated tools. Stage a
   settings patch for entrypoints, products, and caches when needed.
4. Open a sandbox lazily. Upload the staged candidate and restore compatible caches.
5. Run the smallest useful command: syntax check, linter, selected test, or build.
   Inspect its exit status and logs. Repair the responsible files and repeat within
   the session's action and spend budgets.
6. Run the required build and verification for completion. Collect expected products,
   confirm they exist, and retain evidence. Import changes through gates.
7. Record generation with the original fact hash and runtime receipts. Close the
   existing goal only if its gate holds. Stop or release the sandbox.

An action can fail while useful source changes remain staged. A failed test need
not discard an already sealed build cache. It must not produce a passing verdict.
The agent receives enough logs to fix a failure and links to the complete bounded
record; repeated failures reach the existing retry/park limits plus a runtime spend
limit. Infinite build-and-repair loops are not an acceptable completion strategy.

## Selection and results

Preserve today's entity, requirement, kind, all, and force selectors. Extend them
to criterion IDs, named suites, and explicit bound subtest selectors. The compiler
resolves semantic targets into exact ledger rows before starting execution.

| Request | Execution |
| --- | --- |
| One criterion | Its bound case, or its suite with a selection manifest. |
| Several requirements | Union of their cases, deduplicating shared commands. |
| One entity | Its selected requirement and criterion rows. |
| A suite | The suite's declared cases and any reported extra cases. |
| All tests | All selected programmatic rows plus LLM verification packages. |
| One subtest | Exact framework selector mapped to a bound test identity. |

Run the recorded prerequisite build once for the selected candidate before tests.
If already built in the same run, reuse it only when input/configuration hashes and
required outputs still match. Do not assume a cache hit proves a build or test.

Initially support the existing exact-command exit-code contract and JUnit import.
A direct command with an explicit binding can establish its bound test's verdict,
under the current runner failure rules. A bulk suite needs unambiguous case mappings.
A suite's exit code alone does not prove every requested criterion ran.

Map `passed`, `failed`, `skipped`, `missing`, and `ambiguous` to distinct evidence.
Skipped, missing, duplicate ambiguous results, filtered-out cases, and zero tests
executed do not pass. Unknown extra test cases can be shown but cannot silently
claim an unrelated criterion. Report parsing uses existing libraries and limits.

Setup errors, OOM, timeout, cancellation, unavailable images, and transport loss
leave affected tests unverified. A build failure is a build failure, not N failed
requirements. A completed assertion failure is failing evidence. Retain the current
command-not-found and common-runner-failure distinctions when adapting legacy rows.

## Evidence envelope

Every accepted runtime receipt contains:

- Project and document-set identity, workspace revision, and graph generation.
- Input manifest digest, relevant requirement/criterion hashes, source/test hashes.
- Resolved image/platform, action definition, entrypoint hash, and command identity.
- Requested selection, actual mapped cases, exit status, and outcome classification.
- Log/report object references and hashes, timestamps, and producing run ID.

The host produces the envelope from observed execution. A project-produced JUnit
file supplies case results, not its own trusted revision identity. A report is
evidence from project tests, not cryptographic proof that those tests are honest.
Existing test binding and test-quality judgment still matter.

Changed inputs make old evidence stale. Retaining an artifact or importing a report
must not refresh test/requirement hashes to the current version. Commit verdicts
through the existing ledger API, with current fact-hash checks. Runtime bookkeeping
does not write graph shards or verification rows directly.

## LLM verification

An LLM test receives its criteria file, relevant requirements, fact hash, and
implementing artifacts. It judges and calls `record_verdict` through the existing
contract. It uses model usage only when the required files already exist. It does
not allocate a sandbox just to read source or prose.

A PDF or slide deck may need a build before it can be inspected. A presentation
criterion may also require rendered pages. That prerequisite can use the shared
runtime, retain its products and previews, then stop. The verdict remains an LLM
judgment with evidence pointing to the exact rendered artifact. If the model or
host lacks an appropriate binary/image inspection capability, report the missing
capability. File existence alone does not establish visual quality.

LLM verification can request a bounded inspection command if its criteria require
it and the host supports it. Keep this optional. Pure judgment tests should not
pay for an idle environment while the model reads.

## Delivery and test delivery

Delivery can invoke the project's build and verification actions before publishing.
Which tests must pass comes from documented obligations and any user policy. Do
not require all tests for every delivery by inventing a universal release process.
Do not skip a documented gate merely because the script exited zero.

Tests produce retained reports and evidence, whether started from the Test button,
an agent, or a delivery. They need not be disguised as GitHub deliveries. A project
can still expose `Run acceptance suite` as a named delivery if that is useful.

The agent can draft and run local product creation with no external publication.
Publication receives a frozen candidate, destination grants, and a fresh credential
scope. Record any project-defined release URL, digest, remote revision, or other
receipt. A success without a receipt is shown as `Script succeeded`, with the
known facts; it is not proof of destination correctness.

If publication may have happened but the response was lost, retain `outcome unknown`.
The next step is inspection/reconciliation using the script's documented procedure.
Do not rerun migrations, uploads, or notifications automatically. The earlier
[delivery workflow](../delivery/workflow.md) has the recovery rationale.
