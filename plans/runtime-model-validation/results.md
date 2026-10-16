# Results

## Historical log context, 2026-10-16

The sixth presentation retry exposed a remaining log leak in `task_context`.
Seven saved runs produced a 40,177-byte response in an isolated checkpoint copy.
The shared tool boundary now removes log text from run history, idempotent starts,
polling, and cancellation replies. Explicit bounded log tools retain the logs.
The same real MCP query is 6,153 bytes after the fix, with all seven history rows
preserved. Explicit log reads and cancellation of an already completed run passed.

The retry was stopped after 1,307.52 seconds and 15 calls. It saved a source
readability improvement, recovered from one truncated write, then mistyped a task
revision. No new container was admitted; all seven historical runs were terminated.
The checkpoint is preserved for the next local Ollama/Podman attempt. This is not
a completed presentation validation.

Validation: 14 runtime tests, then 703 library tests passed with three existing
ignored tests. The owner's release binary rebuilt successfully. No model call was
needed for the response-size regression.

The seventh retry configured a Markdown source selection, then reused the previous
task-settings revision. After refreshing it, the agent tried two task starts
together. Both guards refused the invalid requests and the agent began waiting
for the admitted build. Execution guidance now explicitly requires a fresh context
after any project/document-set settings save and confirmed termination before the
next task start. The active attempt retains its original binary for clear evidence.

## Earlier evidence

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
| Generation and runtime/LLM verification | Passed native and text workers, focused loop, and chat repair | Gemma passed all four generation benchmark checks under both codecs, including falsification. The focused repair loop, paired LLM judgments, and both-engine report-to-graph checks passed. |
| Cancellation, timeout, failure recovery | Passed | Deterministic lifecycle fixture passed on both engines. Gemma cancellation, timeout, failed-command repair, and adversarial-log cases passed. |
| MCP and ACP entry points | Passed | Real model MCP cases passed. Full ACP passed with native/Podman and text/Docker. Deterministic ACP cancellation passed on both engines. |
| Docker parity | Earlier lifecycle passed; latest model retry hit a host failure | Expanded lifecycle, report-to-graph verification, release smoke, and a real model full ACP workflow passed. The latest retry could not create a volume on Docker's read-only filesystem. |
| Regression suite and release build | Affected checks passed; full-suite timing flake recorded | Latest 144 affected compiler regressions, WASM check, and release build passed. The preceding full suite passed 681 tests with 3 ignored; its unchanged timing test failed under load and passed alone. |

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

## Chat workflow regressions and container rerun

- Writable native and hosted chat now explain binding prerequisites, expose guarded
  file tools by default, and offer a scoped release action for explicitly requested
  work. Manual workflow settings stay unchanged. Provider pauses retain their reason
  through conversation reopening; ordinary cancellation remains distinct.
- Compiler regressions passed: 681 tests, 3 optional tests ignored. The final release
  guard test also passed. The release binary rebuilt successfully and WASM checked.
- Fresh acceptance with that binary passed on Docker and rootless Podman. Each engine
  verified the Python 3.12 Alpine image, project documentation/source/tests, cache reuse
  across fresh containers, binary products, reports, logs, cancellation, and shutdown.
- The repeated native generation benchmark chose LLM criteria for executable output
  assertions and scored 2/4. The prompts now distinguish a suggested criteria path from
  choosing judgment, and require executable tests for runtime behavior. Its paired
  text benchmark scored 0/4 after 22 rounds. It exposed the written-artifact bug
  described below. A real chat release/bind/generate/test/repair case follows it,
  with one local model request active at a time.
- `cargo fmt --check` still reports pre-existing formatting differences across the
  repository. This change does not reformat unrelated code. Changed documentation
  links and anchors resolve, and changed docs contain no em dashes.

## Written-artifact regression

- The full text worker chose a written POSIX shell deliverable and named `hello.sh`
  as its artifact. Its generation manifest repeatedly failed because the harness
  incorrectly excluded every medium artifact as build output, including written
  files. The failure was in the compiler, not just the model's retries.
- Only a built medium's artifact is excluded now. The regression records a written
  artifact successfully and confirms that editing it produces `stale-code`.
  Built artifact exclusion remains covered. All 36 generation unit tests passed.
- The same transcript guessed the project's directory name as a task cwd prefix.
  `task_context` now supplies `projectRoot` and default cwd `.`. The execution
  contract explains how to convert the package's deliverable path and keep globs
  rooted at the project. Fresh model evidence is still required for these fixes.
- The full compiler suite passed 681 tests with 3 optional tests ignored and one
  timing failure: pruning took 5.73 seconds against a 5-second bound under load.
  That unchanged timing test passed when rerun alone. No threshold was relaxed.

## Chat compiler tool availability

- The first native chat acceptance passed stage release and claimed an alignment
  batch. It then tried `upsert_requirement`, `set_coverage`, and `mark_goal_done`.
  All were missing from chat's advertised tools. `done` correctly refused the
  incomplete batch. The run was stopped after this concrete reproduction, with
  its trace retained as failed evidence.
- Chat now serves compiler write and goal tools alongside its lifecycle. The same
  batch toolset, provenance checks, and commit gates still apply. A write without
  an open batch returns `no-open-batch`; conversational edits keep the dual-write
  path. Eighteen MCP tests passed, including real staging, rejected out-of-batch
  writes, rejected out-of-scope writes, abandonment, and unchanged reader access.
- Retry the same fresh chat fixture before treating the complete workflow as
  validated. It intentionally starts with an unaligned document as well as an
  unbound requirement, so it covers compiler prerequisites before generation.

## Live chat generation and repair

- `chat-model-podman-2` used the corrected native chat tools through LocalRouter.
  It released requested stages, reconciled the document, committed compiler batches,
  resolved a duplicate requirement, opened binding, generated a Python function and
  behavioral unittest, and recorded a programmatic `task:check` binding in its
  generation manifest. It submitted the saved Podman task and waited for termination.
  The task passed and its JUnit case entered the requirement ledger.
- The original driver reported failure because it demanded `run_tests` even though
  the model used the supported `run_task` path, which imported the same evidence.
  It also assumed a fixture requirement ID would survive legitimate deduplication.
  The driver now discovers the surviving requirement and checks the actual
  programmatic binding and evidence. It accepts either execution entry point and
  records which binding tool ran. The original failure trace is retained unchanged.
- Independent postflight checked the generated result, deliberately broke the
  function, and observed one assertion failure with zero runner failures. A fresh
  chat session then reclaimed the entity, repaired only its source, refreshed the
  manifest, and used `run_tests` successfully. The test and supplied runner stayed
  byte-for-byte unchanged. All three Podman runs terminated. Postflight and repair
  passed in 470 seconds; the initial chat took 1,274 seconds.
- This is measured generation plus a separate repair session, not a claim that every
  alternative tool path ran in one session. The complete worker benchmark, without
  a supplied task or report adapter, is being repeated under both codecs in
  `retry-full-generation-3`.

## Manifest retry and test-source guidance

- The native `retry-full-generation-3` benchmark passed 4/4 checks in 24 rounds and
  1,441 seconds. The model created a task and JUnit adapter from an empty project,
  corrected an XML serialization error, and ran it in Podman. Independent execution
  passed the tests and falsification made the greeting test fail.
- Reviewing its trace still found avoidable mistakes. The model put a generated
  report in the binding's `artifact` field, then wrote report XML to satisfy file
  validation. The passing grade came from fresh runtime reports, not those written
  cases. The contract and schema now identify `artifact` as test source, put report
  paths only in task `reports`, and prohibit writing report cases for a manifest.
  Validation messages direct source and selector repairs instead of suggesting an
  `llm` test for a metadata mismatch. Saved task IDs are bare; `task:` belongs in
  ledger run references.
- The same trace reproduced a guard defect: after two rejected manifests, a repaired
  file could not be recorded with the same arguments. `record_generation` and
  `record_binding` now share the finish tools' retry exemption. File, ownership,
  freshness, and evidence gates still run. A regression refuses three invalid
  records without changing the ledger, repairs the test source, then accepts the
  unchanged record for both tools. The ordinary repeated-call guard stays covered.
- All 95 tool, 36 generation, and 13 binding regressions passed. Changed documentation
  links and anchors resolve, including repaired obsolete editor links. The full text
  benchmark used the preceding binary; it does not validate these newer prompt
  changes. The updated release build and WASM check passed.

## Full text generation completed

- `retry-full-generation-3` passed the full `gen-basic` benchmark under both codecs:
  native 4/4 in 24 rounds (1,441 seconds), text 4/4 in 13 rounds (700 seconds).
  The model was `gemma4:31b-mlx` through LocalRouter, with rootless Podman execution.
  The benchmark records `generation: capable` for each codec. This resolves the
  earlier unvalidated full text-generation case for this model and fixture.
- The text worker chose a written `hello.sh` artifact, generated unittest source,
  saved a task with project-relative cwd `.`, and bound the actual test source.
  Its first command emitted no JUnit report. After the tool returned missing-case
  evidence, the model changed the task to run pytest with `--junitxml`, reran it,
  and completed the goal. Independent grader execution passed all tests, then a
  deliberately broken greeting failed its selected assertion.
- The native trace's handwritten-report detour remains a quality finding even
  though fresh execution passed its grade. The newer retry and source-guidance
  changes are being checked by a fresh Docker generation, falsification, and repair
  case. Passing one benchmark does not establish reliability for every project or
  model; these local model workflows remain slow.

## Final current-binary acceptance

- `final-generation-docker` reached generation and recorded a binding to the actual
  Python test source. Both execution attempts failed before running the command:
  Docker could not create its helper volume because its internal filesystem was
  read-only. The desktop reported `stopping`, with an unrelated workload still
  present. The owned test driver was stopped, the failure trace retained, and both
  run records confirmed terminated. Docker was not restarted or pruned.
- `final-generation-podman` repeated the complete fixture with compiler `cc9179a`
  and passed in 493 seconds. It recorded the test source and a qualified JUnit
  selector, passed the task, detected deliberate source falsification with one
  assertion failure and zero runner failures, then repaired only the implementation
  and passed again. The test and supplied runner stayed byte-for-byte unchanged.
  All four runs terminated, including the model's extra confirmation of the failure.
- This pass leaves 84 retained run records, all marked terminated. These include
  copied evidence projects, not 84 distinct newly created containers. Podman had
  no running containers, and its dedicated `jazyk-runtime-test` VM was stopped.
  The unrelated Docker workload was left alone. LocalRouter configuration and the
  selected owner model were not changed.
- The tested compiler fix and measured evidence are committed and merged to master.
  The Cloud pin and deployment evidence live in the private repository's
  `plans/chat-runtime-validation/README.md`.

## Docker recovery and current-binary retry

- After owner approval, Docker Desktop was restarted. Its CLI was unresponsive;
  quitting the app stopped the VM but left the backend hung. The remaining backend
  was stopped and the app relaunched. Docker 29.7.2 responded, and the previously
  running unrelated container was started again. No images, volumes, or project
  data were pruned.
- `docker-restart-baseline` passed with compiler `cc9179a`. Two fresh Python 3.12
  Alpine containers saw the documentation, source, tests, and project settings.
  The second restored the first run's cache. Binary products were imported and
  downloaded intact; JUnit reports and logs were retained; transient files were
  absent from the next run. Owner cancellation and cancellation with invalid
  project settings both stopped running work. All four runs terminated.
- `docker-restart-generation-1` passed in 470.67 seconds using `gemma4:31b-mlx`
  through LocalRouter, native codec, and the same compiler. The model generated
  `greet.py` and `tests/test_greet.py`, bound the actual test source to `task:check`,
  and passed `TestGreeter::test_greeting` in Docker. Independent source falsification
  produced one assertion failure and zero runner failures, with a failing graph
  verdict. The model confirmed the failure, read its log, reopened generation,
  repaired only the implementation, refreshed the manifest, and passed again.
  Test and supplied runner bytes stayed unchanged. All four runs terminated;
  all 15 model tool calls completed without tool errors.
- The previous Docker host failure remains preserved as failed evidence. The fresh
  pass resolves the current-binary Docker generation gap for this fixture. No
  prompt or implementation change was needed for the retry. Docker has no remaining
  test containers or named helper volumes, the unrelated workload is running, and
  the dedicated Podman VM remains stopped. This evidence-only update needs no
  production redeployment.

## Empty source mapping follow-up, 2026-10-16

- Compiler `59d3e12` guides an explicit generation request through document-set
  source setup. The workspace regression preserves identity and documents and
  observes newly available criteria goals after the edit. All 19 MCP, 30 goal,
  and 26 board tests passed, with one existing ignored board test. The release
  build passed. The changed documentation's 176 relative links resolved.
- `source-setup-chat-1` used `gemma4:31b-mlx` through LocalRouter and Podman. The
  model configured initially empty source selections, released generation,
  wrote and bound a behavioral test, generated `src/greet.py`, and passed its
  programmatic check. It corrected one rejected criterion identifier. The driver
  reached independent verification after 794.07 seconds but then falsified a new
  root-level file instead of the implementation. That is a harness failure, not a
  passing falsification result. The corrected driver finds the single generated
  module, and `source-setup-chat-2` retries from fresh state.
- `source-setup-docker-lifecycle` passed on the same release: cache success and
  failure behavior, revision conflicts, idempotency, queued and running
  cancellation, cancellation with invalid settings, retained-file conflicts,
  timeouts, searchable failed logs, and invalid images.
- The unchanged large-store pruning regression passed in 2.08 seconds under its
  existing five-second assertion. This confirms the isolated case; it does not
  establish a portable wall-clock limit under unrelated host load.
- `source-setup-chat-2` passed the full cycle in 1,539.06 seconds with 38 model
  tool calls. The model configured source selections, compiled, generated and
  bound `src/greet.py` and `tests/test_greet.py`, and passed the check. Independent
  source falsification produced one assertion failure, zero runner failures, and
  a failing graph verdict. The model confirmed the failure, reopened generation,
  repaired the implementation, refreshed the manifest, and passed again. Test
  and supplied runner bytes stayed unchanged. All four Podman runs terminated.
- The full compiler library suite passed: 685 tests, zero failures, three existing
  ignored tests. The Docker lifecycle fixture's nine runs all terminated. The
  dedicated Podman VM was stopped after verifying no test containers remained.

## Bounded tool recovery and local presentation follow-up, 2026-10-16

- Compiler `eca04df` rejects cap-exhausted native calls with missing required
  arguments before dispatch, pairs rejected calls with results, and bounds repair
  budget boosts. Four provider-loop regressions cover successful recovery, valid
  calls at the cap, malformed mixed batches, and termination after repeated bad
  calls. All 689 library tests passed, with three existing ignored tests. The owner
  release binary was rebuilt.
- `cap-recovery-podman` passed in 153.14 seconds with `gemma4:31b-mlx`, native
  codec, and MCP. LocalRouter routed to Ollama's MLX runner. Two fresh Python 3.12
  Alpine containers saw project inputs and imported receipt products. Cache count
  advanced from 1 to 2; cache and transient files did not enter the product tree.
  Both containers terminated.
- `cap-recovery-generation` failed after 1,390.48 seconds. The model configured
  source, generated files, and recorded a binding, but Podman was no longer
  reachable when the tasks launched. Both runs terminated without executing the
  command. This is not a passing generation or falsification result. The dedicated
  VM is now held by a separate supervisor while model tests run. The driver checks
  engine availability before spending model time and prints launch errors before
  the task definition.
- Falsification now follows the requirement binding's implementing file. An
  unbound leftover source copy cannot choose the target or mask a broken bound
  implementation.
- The private local copy of the original Cloud presentation exposed an unrelated
  search defect: `kind: view` with no match returned entities and suggested
  `upsert_entity`. View misses now inventory only views, include staged titles
  once, and guide view reads or direct entity/requirement reads. The inventory is
  bounded, and truncated replies permit a more specific title search. This fix
  does not establish presentation generation success; that attempt is still
  evaluated from actual run results and retained files.
- All 96 tool regressions passed, including empty, staged, duplicate, bounded,
  batched, and unlisted-title view-search cases. The edited documentation's links
  and anchors resolve. Repository-wide `cargo fmt --check` still reports existing
  formatting differences across many files; unrelated formatting was preserved.
- The first local presentation attempt stopped after 1,082.31 seconds. It launched
  two Python 3.12 containers. The first exposed a missing slide title placeholder;
  the attempted repair introduced extra escaping and the second returned a Python
  syntax error. Both terminated. The next model reply contained seven binding
  calls without a top-level verdict, followed by a source repair. Five repeated
  schema failures in that single reply exhausted the tool-error allowance before
  the model could see the feedback. The repair was skipped. No successful PPTX or
  verified requirement is claimed.
- Required-field validation now rejects the whole reply before dispatch and counts
  it as one failure. All calls receive paired skipped results. After a dispatched
  call fails, later calls in that reply are also skipped without consuming more
  allowance. Five repeated failures still stop the turn. Ordinary schema repair
  does not increase completion budgets. The chat prompt names `begin_binding`,
  package-provided criterion IDs, and the top-level binding verdict explicitly.
- The final library suite passed 693 tests, with three existing ignored tests.
  Provider-loop regressions cover a seven-call malformed batch followed by repair,
  bounded repeated schema failures, and bounded execution failures with complete
  native tool history. The real presentation retry remains local Ollama and Podman.
- Review found that cancellation between native calls left later calls unanswered
  in the saved history. A local scripted regression reproduced one result for a
  three-call reply. The loop now preserves a dispatched call's actual result and
  records the rest as skipped, without dispatch. A subsequent prompt can reuse the
  history. Cancellation before dispatch and during the final call both return
  `cancelled`, even on the last permitted round.
- All 20 agent-loop tests passed. The library suite passed 697 tests, with three
  existing ignored tests. The 71 relative links and anchors in the changed ACP
  page resolve, and it contains no em dashes. These transport regressions use local
  scripted HTTP responses, not a paid model provider.
- The local release binary rebuilt successfully in 62 seconds. The ongoing
  presentation attempt uses its immutable earlier executable snapshot.
- The third presentation attempt repaired its tests, passed three fresh JUnit
  cases, and retained its PPTX in a second successful Podman run. Both containers
  terminated. The model then fetched ten binding packages together. Ollama saw a
  73,095-token prompt with 28,536 uncached tokens. LocalRouter returned HTTP 502
  after 300,011 ms while Ollama was still processing the prompt. Eleven binding
  packages over the turn repeated 101,893 characters of instructions. The turn
  ended after 1,468.05 seconds without completed bindings. It did not pass the
  full presentation acceptance gate.
- The agent loop now shares identical top-level instruction blocks from the same
  tool within a prompt turn. The first and every changed block remain complete;
  later identical blocks refer to the last complete block. Requirement fields and
  full audit events remain intact. Native and text-codec regressions verify both
  changed instructions and unchanged per-call data. All 21 loop tests and 698
  library tests passed, with three existing ignored library tests. The release
  build passed in 53.17 seconds. All 71 ACP documentation links resolve.
- An independent copy replaced all cat jokes with plain statements and still
  passed the three unchanged tests. That outcome is a coverage finding, not
  completed semantic verification. The next checkpoint retry receives this
  finding and the rendered deck's contrast findings as explicit review feedback.
- The fifth local presentation retry exposed repeated full logs in `wait_run`
  status replies while completed commands were still cleaning up. Status polls now
  omit the log field. Explicit bounded log reads and searches remain available
  during cleanup and after termination. The regression failed before the fix and
  passed afterward. All 13 runtime module tests passed. The changed runtime design
  page contains no em dashes and has no relative links to check.
- The presentation's fifth attempt registered a Python test as an LLM criteria
  artifact after its Markdown write was rejected. Recording now validates Markdown,
  YAML requirement and criterion identity, the current statement hash, and explicit
  verdict outcomes. Invalid saved rows return to binding; audit cannot reconstruct
  them as valid criteria. The packages provide exact front matter and source globs,
  and rejected writes explain how to select or configure a criteria path. The
  regression covers executable substitution, missing and duplicate metadata, stale
  hashes, wrong IDs, criterion ownership, valid repair, and an invalid historic row.
  All 702 library tests passed, with three existing ignored tests. All 143 relative
  documentation links and anchors resolve; the changed pages contain no em dashes.

## Independent LLM criterion verdicts, 2026-10-16

- The seventh local presentation retry completed in 1,784.10 seconds with 35 tool
  calls. It used LocalRouter, local Ollama `gemma4:31b-mlx`, and Podman. All ten
  requirements were verified. The two new containers terminated and the PPTX was
  retained. An independent fresh test run preserved all ten verified statuses.
  The final PPTX matched the independently rendered artifact byte for byte.
- This was a review-assisted checkpoint retry. The initial attempts and their
  failures remain recorded. It is not a claim of an unassisted first-attempt pass
  or a hosted model run. The final assistant sentence misstated a secondary count
  as nine; the deterministic graph check counted ten.
- A fresh local-model negative control replaced all three jokes with unrelated
  plain statements. The model read the current source and correctly recorded fail.
  The harness updated only the headline LLM criterion, leaving the second
  criterion's old pass. The row incorrectly described a partial implementation.
- Verification packages now include every criterion test and each LLM criteria
  file. Multiple LLM criteria require an explicit, complete verdict list. The
  harness validates that list atomically, rejects programmatic or stale entries,
  and preserves separate outcomes. The built-in verifier judges each criterion.
- Refreshing implementing-file hashes invalidates old criterion evidence. Fresh
  task reports, legacy runs, or model judgments restore only the tests they
  actually cover. Audit follows the same rule. A report cannot make an old LLM
  judgment current after a source change.
- Four meaningful regressions passed, including independent and atomic recording,
  report/LLM freshness, audit invalidation, and separate built-in model judgments
  with inconclusive-output handling. The full library suite passed 707 tests,
  with three existing ignored tests. The release binary was rebuilt. All 290
  relative links and anchors in the changed design pages resolve.
- The fresh local-model negative control passed in 182.11 seconds with three
  calls: `begin_verification`, `read_text_file`, and `record_verdict`. Both LLM
  criteria recorded fail independently, the requirement became failing, the
  source stayed unchanged, and no runtime container was used. The previous
  attempt took 283.58 seconds and exposed the stale second verdict.
- The positive presentation was independently replayed once more with the fixed
  binary. Its fresh Podman test run passed, all ten requirements stayed verified,
  and the PPTX matched the reviewed bytes.
- Final review also covers a programmatic headline with an LLM criterion. Its
  package includes both execution and judgment guidance. An LLM pass leaves an
  unrun programmatic case unknown; a fresh matching report then verifies the row.
  This fifth regression and the full suite passed: 708 tests, three existing
  ignored tests. The owner's release binary was rebuilt.
- The final prompt review also found a legacy-command package still inviting a
  manual model verdict. It now directs the agent through `run_tests` and keeps
  unavailable commands unverified. The mixed-case regression checks both legacy
  and saved-task guidance. An initial full-suite run had 707 passes and one
  fixture socket timeout. That cancellation case passed alone in 0.52 seconds.
  The full suite then passed all 708 tests with four test threads in 20.01 seconds,
  with three existing ignored tests. No timeout or assertion was relaxed. All
  102 relative links and anchors in the changed design pages resolve.
- Compiler `86daa56` is now pinned by the deployed Cloud API and editor. A fresh
  Fargate test of the retained presentation passed all three report cases and
  preserved all ten verified requirements. ECS independently confirmed shutdown.
  The retained binary PPTX and project backup preserved their reviewed bytes.
  This used no Cloud model calls; model validation remained local Ollama through
  LocalRouter. The dedicated Podman VM and owned build containers are stopped.
