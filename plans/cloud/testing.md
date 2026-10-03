# LLM tests and CI evidence

Status: proposed, 2026-10-03. Keep the existing distinction between `llm` and
`programmatic` tests. LLM-run tests remain in Jazyk and jazyk.dev. Executable tests
in the new delivery workflow run on GitHub Actions.

## Two paths, one test view

| Test kind | Execution | Evidence |
|---|---|---|
| `llm` | Jazyk verification session using the selected backend/model | Judgment, inspected artifacts, criterion/requirement hashes, model/session identity |
| `programmatic` | GitHub Actions command delivery | Test/command result, report, exact candidate, workflow run and attempt |

`Run LLM tests` in jazyk.dev uses the existing verification contract. It does not
require repository access or start a CI job. In local mode it keeps the user's
agent/provider configuration; in cloud mode it uses the account's managed route.
This describes a product workflow, not a requirement that inference run in the browser.

`Run code tests` prepares and dispatches the associated delivery. Python, JavaScript,
native executables, dependency installation, and artifact builds execute on GitHub.
Neither the browser nor shared API host receives a customer shell. A model judging
a test's source code is not evidence that the programmatic test executed.

LLM tests can consume a CI-produced artifact once downloaded into the project's
artifact view. If required bytes are absent or stale, the judgment stays pending.
The model inspects the artifact under test, not merely its manifest or description.

## Existing contracts that need adaptation

The current [generation spec](../../docs/consumers/gen.md#runners) executes a build
before tests and runs recorded programmatic commands locally. The
[verify goal](../../docs/compiler/goals/verify.md) can settle programmatic rows
without a model session. [verify.rs](../../bootstrap/src/verify.rs) implements a
direct shell path. The hosted profile must replace this path before deployment.

- Keep `begin_verification` and `record_verdict` for LLM judgments and their gates.
- Route programmatic work through a CI execution/evidence adapter. No LLM is needed
  to parse a supported report or record a correctly mapped test verdict.
- An unresolved code-test goal becomes visibly `Awaiting CI` when delivery has not
  been authorized or its run/evidence is pending. This is proposed scheduling/UI
  behavior, not an existing ledger enum. Do not claim compiler convergence early.
- A compile or generation request does not implicitly authorize a repository push.
  Start delivery on an explicit request or applicable standing policy. Re-deriving
  the same goal cannot dispatch duplicate runs or create a polling/retry loop.
- Do not hold a project writer lease or SQL transaction while waiting for GitHub.
  Persist the candidate/work reference, release the active build, then reacquire
  the normal writer authority and revalidate when evidence arrives.
- Source generation can finish before a requested binary/artifact build. The latter
  remains pending until CI returns the expected bytes. Specify this distinction in
  the generation lifecycle instead of claiming an absent artifact was generated.

Preserve the existing accountless native CLI behavior outside the new hosted
workflow. This plan adds no local-shell delivery backend. Update canonical compiler,
generation, verification, and frontend specs before implementing these mode boundaries.

## Preparing executable tests

Map each programmatic criterion test to a stable report identity and its delivery.
For JUnit, this can be an explicit `(suite, class, test name)` mapping or a stable
criterion id emitted by a supported reporter. Keep the mapping and expected test
set with the frozen candidate. Do not guess from names or console prose after a run.

The model may generate test files, configure the reporter, and add/edit the command
delivery. The workflow generator supplies checkout, setup, execution, and artifact
upload. E.g., `npm test` requires a configured JUnit reporter before
`reports = ["reports/junit.xml"]` produces usable results. A Python delivery could
use `python -m pytest --junitxml=reports/junit.xml`.

A generic command can run without a structured report. Show its exit status as a
command result. Use a dedicated recorded test's exit code only when the command is
explicitly bound to that test and the harness can establish it actually ran. A broad
`npm test` success does not, by itself, resolve every programmatic criterion. A
failing aggregate command likewise does not mean every test in its suite failed.

Generated workflows preserve nonzero test exit codes and upload available reports
on failure. Do not turn a failure into success just to upload evidence. Setup/build
failure, cancellation, timeout, or zero discovered tests leave affected tests without
a verdict unless valid per-test results establish what actually ran.

## Ingest and validate

1. Authenticate the repository/run through the GitHub API. Resolve installation,
   repository, workflow, run id, and rerun attempt against the recorded delivery.
2. Fetch the manifest/reports through the provider artifact API. Record digests and
   retained bytes; enforce archive, path, XML, size, and test-count limits.
3. Verify the checked-out candidate SHA/digest and workflow/config/command identity.
   Workflow head and tested source can differ; establish both explicitly.
4. Match report entries to the frozen binding set. Duplicate or ambiguous identities
   need resolution, not last-write-wins. Unmapped results remain visible run results.
5. Preserve the requirement/criterion, test artifact, and implementing-file hashes
   from the candidate. Compare them with current state before applying the verdict.
6. Commit matching evidence through the ordinary verification gates and project
   writer. Deduplicate by provider run/attempt, test identity, and binding revision.
7. Update jazyk.dev, ACP events, and LSP-linked evidence from the acknowledged state.
   A local-backed graph waits for its connector to commit the imported result.

The CI workflow does not receive database credentials or write the graph itself.
The ingestion harness owns the decision to accept evidence. A webhook or manifest
claim alone cannot mark a criterion passed.

## Verdicts and freshness

- A recorded assertion failure is a failing test for the mapped criterion.
- A passing test resolves only its mapped, current programmatic criterion.
- Missing, skipped, cancelled, setup-failed, malformed, or expired evidence is not a
  pass. Keep the reason visible and distinguish failure to run from assertion failure.
- Valid results for an older candidate remain historical. They cannot rebaseline
  hashes against today's files and thereby mark changed criteria verified.
- Importing a CI result never changes the test kind to `llm` or overwrites an LLM
  judgment. LLM evidence also never substitutes for missing programmatic execution.
- The requirement's verdict still depends on all of its current criterion tests.
  A green workflow is not a shortcut around that rule or external delivery checks.

E.g.:

```text
Requirement: Checkout works
  Valid order accepted       programmatic  PASS  GitHub run 81/1, source abc123
  Invalid order rejected     programmatic  FAIL  GitHub run 81/1, source abc123
  Error message is clear     llm           PASS  Jazyk session 19, source abc123
Requirement: failing
```

If the error-message artifact changes, its LLM result becomes stale independently.
If an executable test is skipped, show it as not verified even when GitHub reports
the overall workflow successful. The same evidence and freshness rules apply in
the browser, CLI, and IDE views.
