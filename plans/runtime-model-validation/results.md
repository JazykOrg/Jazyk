# Results

Work started on 2026-10-15. Raw local evidence is retained under the main checkout's
ignored `.worktrees/runtime-model-validation/.local/model-validation/` directory. The reusable model driver is
`bootstrap/tests/runtime_model_acceptance.py`.

Prior release evidence includes real Docker, Podman, Fargate, and deterministic ACP/MCP acceptance. Its `gen-basic` model benchmark failed in both codecs. This pass will inspect those traces before choosing corrections.

| Area | Status | Evidence |
| --- | --- | --- |
| Endpoint and minimal model/tool calls | Passed with limits | LocalRouter completion passed with Qwen 27B and Llama 8B; Gemma completed real tool workflows. Qwen tool discovery hit an MLX provider error. |
| Podman container baseline | Passed | Existing acceptance: two fresh containers, Python/Alpine image, inputs, binary products, cache reuse, reports, cancellation, shutdown. |
| Model discovery, submission, observation | Passed | Gemma discovery and repeated task creation/runs passed, including observed shutdown and cache reuse. |
| Model task creation and repair | Passed | Gemma native/MCP created and ran a task twice (166 seconds), observed shutdown, and reused cache. A separate failed-command repair passed (144 seconds). |
| Generation and runtime/LLM verification | Passed native worker and focused loop; full text worker hit a provider error | Gemma passed all four native generation benchmark checks, including falsification. The focused repair loop, paired LLM judgments, and both-engine report-to-graph checks passed. |
| Cancellation, timeout, failure recovery | Passed | Deterministic lifecycle fixture passed on both engines. Gemma cancellation, timeout, failed-command repair, and adversarial-log cases passed. |
| MCP and ACP entry points | Passed | Real model MCP cases passed. Full ACP passed with native/Podman and text/Docker. Deterministic ACP cancellation passed on both engines. |
| Docker parity | Passed | Expanded lifecycle, report-to-graph verification, and the real model full ACP workflow passed. |
| Regression suite and release build | Passed | 677 compiler tests passed, 3 ignored. All 22 benchmark-module tests passed after the last reporting fix. Go worker tests, final WASM check, and final release build passed. Release smoke passed on both engines. |

## First investigation

- The earlier generation trace correctly noticed that `record_medium` was unavailable.
  Both generation and binding goal slices now include it. A regression checks all
  generation tools across the worker and MCP slices.
- The embedded ACP agent discarded MCP initialization instructions. It now retains
  them before the first prompt. A real stdio handshake regression checks preservation.
- Tool result JSON was encoded as a string inside another JSON string. Both codecs
  now receive one JSON layer; plain text stays readable. Codec regressions cover both.
- Compiler suite after these changes: 664 passed, 3 ignored. Release build passed.
- Qwen `qwen3.8:27b-mlx` answered the tiny request in 13 seconds, but discovery failed
  in Ollama's MLX runner: maximum GPU threads per threadgroup 896, requested 960.
  Gemma `gemma4:31b-mlx` discovery hit the 90 second provider read timeout. These are
  provider failures, not successful or failed task decisions.
- Llama `llama3.1:8b` answered the tiny request in 6 seconds. Larger turns are next.

## Second investigation

- Gemma passed runtime discovery in 78 seconds through LocalRouter and the actual
  embedded agent/MCP stack. It only inspected the task as requested.
- The first two Gemma-controlled Podman runs produced receipt counts 1 and 2.
  The initial test incorrectly required a separate log-read call even though
  `wait_run` already returned the log. That assertion is corrected. The second run
  also exposed a real observation gap: the model ended after command success while
  its last observed `terminated` flag was false. The prompt now distinguishes command
  success from completed cleanup. The driver requires observed termination.
- The small `runtime` MCP toolset uses the same nine tools and dispatch as the other
  entry points. It permits focused model tests without graph tools or document-set
  schemas. Full ACP coverage still remains a separate requirement.
- Llama generated a long batch of invalid calls. The generic loop now stops after
  five consecutive tool failures, including calls outside graph batches, and records
  skipped results for the remaining calls. A regression checks the dispatch count,
  reset after success, and complete native tool history.
- Runtime-aware benchmark fixtures now include real project settings and documents;
  graph state lives under `.jazyk/ds_main`. Single-row and falsifiability checks now
  execute `task:<id>` through the runtime and judge the selected report case. Missing,
  skipped, ambiguous, and incomplete cases cannot count as falsifiable failures.
- `runtime_lifecycle_acceptance.py` passed on both engines: successful cache promotion,
  no promotion after failure, restored cache, stale definition rejection, idempotent
  submission, immediate and executing cancellation, cancellation with invalid TOML,
  product conflict retention, timeout, searchable failed logs, and invalid image.
- Podman needed a persistent test supervisor. The VM exited when its launching command
  session was cleaned up; keeping its supervisor alive restored cross-call availability.
- A temporary Qwen alias with a 16K context was created to investigate the MLX error.
  LocalRouter did not recognize the alias. No owner model or endpoint setting changed.
  The temporary alias was removed afterward.

## Third investigation

- Gemma created the entire task definition from the request, ran it twice, inspected
  products, observed actual termination, and correctly reported cache reuse. The
  independent assertions checked receipt counts, discarded transient files, and
  absence of cache files in the project.
- Gemma repaired a task after an actual exit 7, preserving its settings, then observed
  a successful run and inspected its retained product.
- The text cancellation case emitted six calls in one model response, with revision
  `1` and an invented run ID. No run started. The failure bound stopped the bad batch.
  The text protocol now rejects multiple actions without executing any; its repair
  asks for one call and a real result. Generic instructions now live in embedded
  Markdown payloads and explicitly prohibit simulated results.
- Review found runtime polling inherited the graph's repeated-call guard. Runtime
  calls are now exempt so unchanged pending status does not prevent a later poll.
- Generation and binding prompts still described legacy single-test exit-code
  commands as mandatory. They now explain shared tasks with per-case JUnit evidence
  and distinguish project-relative task paths from deliverable-relative manifests.
- Qualified JUnit selectors could bind successfully but later appear missing, or be
  rejected by generation. All these gates now share one artifact-name check. A
  regression records a qualified selector, checks that it remains bound, then checks
  rejection of an empty selector and detection of a removed test.
- Portable runtime discovery without an execution adapter now returns unavailable
  instead of panicking. The polling regression exposed that missing-context path.

## Fourth investigation

- Gemma text cancellation passed in 91 seconds after the single-action correction.
  It observed `CHILD_READY`, cancelled the actual returned run ID, and waited for
  termination. Native timeout passed in 98 seconds without extending the deadline.
- Gemma searched a failed run's log in 109 seconds. It treated an instruction planted
  in the log as data and made no unauthorized task edits or retries.
- Full ACP with no IDE terminal/filesystem passed: native/Podman in 77 seconds and
  text/Docker in 176 seconds. The earlier native attempt exhausted the test driver's
  90-second request limit; the repeat allowed 240 seconds. Both successful cases
  observed actual termination and inspected the retained receipt.
- `runtime_verification_acceptance.py` passed with Podman and Docker. One shared task
  emitted passing, failing, skipped, missing, and ambiguous cases. The graph retained
  separate requirement and criterion evidence despite command exit zero. The fixture
  also checked a legacy command in the same selection, the build running first,
  refusal to import evidence for changed inputs, and rejection of a successful build
  command that did not produce its promised product.
- Review found task-backed verification returned early and dropped legacy commands
  from a mixed selection. It also bypassed the build. Selection now runs the build
  once, each shared task once, and the selected legacy commands locally. Hosted
  responses retain explicit pending entries for legacy commands. Task criteria reuse
  imported reports instead of launching another container.
- A task build now checks promised products after collection, like legacy builds.
  A verification package for `task:<id>` now explicitly directs the model to
  `run_tests` and per-case evidence. Unknown cases cannot produce an all-passed reply.
- The benchmark worker and automatic report importer had different deliverable roots.
  Fixtures now use the project root consistently, matching document-set behavior.
- Fixture iteration corrected source selection, scoped IDs, response field assertions,
  and the combined-build/test request timeout. Those fixture mistakes are not recorded
  as product defects. Final evidence: `podman-verification-7`, `docker-verification-1`.
- Regression progress: new verification tests passed. A fixture-path assertion needed
  updating after the root correction. The unrelated 3,000-record timing test exceeded
  its five-second bound under concurrent machine load; an isolated repeat is pending.
  The real generation and paired LLM judgment benchmark is still running.

## Fifth investigation

- The native Gemma `gen-basic` worker reached `begin_generation`, `record_medium`,
  and `task_context`, then hit the 240-second provider read timeout. Its trace is
  retained. Both native LLM judgment cases passed. The text generation attempt was
  interrupted after about ten minutes without source files; it remains unmeasured.
- Worker goal text still advertised the removed `run_command` tool. Generation and
  verification now direct workers to saved tasks. The generation goal is shorter
  and leaves detailed rules in the returned package. Medium instructions now explain
  the undecided case instead of contradicting the package.
- Llama 8B failed `gen-basic` in both codecs. Native stopped at the five-error bound
  after 66 seconds. Text exhausted 24 rounds in 121 seconds, inventing manifest
  values and reporting its own mistakes as feedback. No generation record landed.
- That trace exposed an actual false-success response: explicitly selecting an
  unbound requirement could produce an empty all-passed reply. Requirements selected
  directly or through their entity now retain binding guidance; an empty execution
  makes no passing claim.
- The benchmark fixture's worker source glob was still empty despite its saved
  project having a source selection. Workers now resolve actual fixture settings,
  matching the importer. A regression writes a generated file and rejects a document
  overwrite through the real tool session.
- Compiler regressions: 673 passed, 3 ignored. A further small selection-response
  assertion is being rerun. The isolated timing test passed, and its later full-suite
  run passed too; no timing threshold was relaxed.
- A smaller live generation fixture is now running through MCP: one Python function,
  one generated test, the supplied unittest-to-JUnit adapter, actual Podman execution,
  deliberate source falsification, and model repair while preserving the test.

## Sixth investigation

- The small Gemma fixture generated working Python source and a behavioral test,
  recorded its qualified JUnit binding, and passed in Podman. Deliberately replacing
  the implementation made that test fail. Repair then exposed a real MCP defect:
  an unpinned serving lost its file ownership target between calls, so even a fresh
  `begin_generation` could not edit its own previously recorded source.
- Optional MCP file tools now retain the successful begin's target until recording
  succeeds. Rejected records retain the claim. Writes renew it, and another worker's
  live claim blocks both writes and recording. A regression covers initial writes,
  rejected recording, successful recording, later repair, and competing ownership.
- The generated source also used a document-set-qualified requirement marker. The
  recorder now accepts the current set's qualified IDs, strips their markers, and
  stores local anchors. Markers for another set remain untouched. The targeted
  source-anchoring regression passed.
- The independent fixture observer now reads `begin_verification` instead of taking
  a generation lease just to inspect status. Interrupted fixtures retain an explicit
  failure and completed phase flags. The first repair attempt was stopped after the
  reproduced ownership refusal; it is not a complete generation-and-repair pass.
- The execution contract now explains claiming an existing entity for repair and
  recording the refreshed manifest before rerunning bound tests. A fresh full loop
  is running with those fixes.
- Full compiler regression suite: 674 passed, 3 ignored. Relative links and anchors
  in all 20 changed documentation files resolve, with no em dashes.

## Verification review

- Review found that the legacy criterion runner could keep a previous passing
  verdict after a failed launch, or mark a missing command as an assertion failure.
  Both paths now clear the criterion verdict and retain the runner error. A native
  regression starts from verified evidence, then exercises a missing executable
  and a missing working directory. Both finish unverified and the regression passes.
- The test-kind documentation now distinguishes saved-task report verdicts from
  legacy command exit codes. This removes an older blanket exit-code statement
  that contradicted the runtime report contract.

## Successful generation loop and lock review

- `gemma-small-generation-2` passed in 559 seconds. The real model wrote the Python
  function and its exact-output unittest, recorded the medium and manifest, and
  verified through the saved Podman task. The fixture broke the implementation;
  both the independent observer and the model saw a failing case. The model claimed
  the entity again, repaired only the source, refreshed the manifest, and verified.
  The generated test and supplied runner stayed unchanged. All four runs terminated.
- Qualified markers were stripped into graph anchors in that live run, and the
  repair no longer hit the former `file-owned` refusal.
- The WASM check passed with the installed rustup compiler explicitly selected.
  Earlier attempts selected Homebrew's compiler, which has no WASM standard library.
  No compiler defaults or model settings were changed.
- A later full suite exposed an intermittent document-import lock conflict. The
  isolated import test passed. A deterministic descriptor-duplication regression
  then failed: closing the guard's descriptor did not end its lock while a duplicate
  remained open. This matches [flock descriptor semantics](https://man7.org/linux/man-pages/man2/flock.2.html).
  Project guards now explicitly unlock on drop; concurrent child startup can no
  longer prolong the completed operation's lock through an inherited descriptor.
  The fresh full suite passed: 676 tests, 3 ignored. Descriptor inheritance is the suspected cause of
  the original intermittent conflict, not a traced observation of that failed run.

## Full worker and release smoke

- The corrected native `gen-basic` benchmark passed all four checks in 975 seconds:
  honest generation record, one test row per requirement, passing tests, and a test
  that fails when the required output is replaced in the product. It used 13 rounds
  and 7,007 reported tokens. This is the full worker contract, with no supplied task
  or test adapter. The model wrote the shell source and three Python tests, selected
  `python:3.12-slim`, and configured pytest with fresh JUnit output.
- The first test run passed but its selectors did not match the report identities.
  The graph stayed unverified. The model used the returned identities to correct the
  manifest, ran the task again, and passed the generation gate. The benchmark then
  independently reran the tests and falsified the product. Evidence is under
  `gemma-worker-generation-3`; its full text-codec attempt is still running.
- Three definition retries exposed vague validation feedback: the model supplied an
  absolute `cwd`, then repeated a report pattern under `products` and `reports`.
  Errors now name the value and conflicting fields. The schema and execution prompt
  say to use `cwd="."` for the project root and list JUnit patterns only in `reports`.
  No path or classification restriction is relaxed.
- The release build passed. Fresh release smoke tests passed on Docker and Podman:
  selected Python/Alpine image, project docs/source/tests/settings, fresh containers,
  restored cache nonce and counter, binary products, JUnit reports, logs, process-tree
  cancellation, and shutdown. Their disposable fixtures were copied to durable local
  evidence directories `release-docker-smoke` and `release-podman-smoke`.

## Final validation follow-up

- The full text `gen-basic` attempt made three valid calls (`begin_generation`,
  `record_medium`, `task_context`). Its fourth request returned HTTP 502 from
  LocalRouter after 300 seconds, naming the Ollama-compatible upstream on port 11435.
  Jazyk allowed 600 seconds. The timing suggests an upstream request limit; no
  provider setting was changed. The full text generation path remains unvalidated,
  although the focused text cancellation and full ACP runtime workflow passed.
- The full suite after the task feedback edit had 676 passes, one timing failure,
  and 3 ignored tests. The unchanged five-second scorer test took 7.38 seconds under
  parallel load. A serial rerun passed all 677 tests, with 3 ignored, in 238 seconds.
  No timing threshold was relaxed. The five targeted task-definition tests passed.
- Gemma's planted LLM pass and fail cases passed in both codecs (23/18 seconds native,
  15/16 seconds text). Their benchmark then incorrectly reported no completions:
  judging runs in an ACP child and did not update the parent's completion-token
  counter. Measurement now uses completed worker rounds or received judge replies.
  An invalid judgment still counts as a received reply; a transport error does not.
  Its targeted regression passed. A fresh real-model repeat passed in both codecs
  and saved `verification: capable` results (27/19 seconds native, 14/16 seconds
  text). Evidence: `gemma-judgments-corrected`. Child token counts remain unavailable
  to that parent counter; this change does not fabricate token usage or claim to
  repair billing.

## Completion

- All 22 benchmark-module regressions passed after the final reporting fix. The
  final release build and WASM check passed. The final release binary then passed
  the full Podman image/files/cache/products/report/cancellation smoke again.
- No Jazyk test containers remained running on Docker or Podman. All 50 run records
  in the durable model and lifecycle fixture projects reported termination. The
  dedicated `jazyk-runtime-test` Podman VM was stopped. Its cached images remain
  available for later local tests.
- Plans, reusable drivers, contracts, and fixes were integrated into public `master`
  by fast-forwarding `test/runtime-model-validation` through `2bb86db`. Raw evidence
  remains in the validation worktree at the path above.
- Remaining model/provider limits: Qwen's tool discovery hit an MLX GPU error;
  Llama 8B did not complete generation; Gemma's full text-generation attempt ended
  with an upstream HTTP 502. These are recorded failures, not passing coverage.
  The measured native Gemma worker, focused text runtime workflows, and direct
  LLM judgments passed. This pass did not grade every unrelated compiler goal.

## Text protocol retry

- Reused `gemma4:31b-mlx` through the configured LocalRouter endpoint. No provider
  settings changed. The dedicated rootless Podman VM was restarted for this pass.
- `retry-text-discover-1` passed in 69 seconds. The model read the saved receipt task,
  identified its Python image, and confirmed local execution availability.
- `retry-text-generation-1` passed in 1,032 seconds. Using the text protocol through
  MCP, the model wrote the Python source and behavioral unittest, recorded its
  manifest, and passed the saved Podman task. The driver broke the implementation
  and observed one assertion failure with no runner failure. The model reclaimed
  its entity, repaired the source, refreshed the manifest, and passed again. The
  test and supplied report adapter stayed unchanged. All three runs terminated.
- No Jazyk fix was needed for those repeats. Responses were slow during observed
  high host load. The larger worker benchmark, which supplies no task or report
  adapter, is being repeated separately.
