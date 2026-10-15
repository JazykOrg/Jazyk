# Results

Work started on 2026-10-15. Raw local evidence is under this worktree's ignored
`.local/model-validation/` directory. The reusable model driver is
`bootstrap/tests/runtime_model_acceptance.py`.

Prior release evidence includes real Docker, Podman, Fargate, and deterministic ACP/MCP acceptance. Its `gen-basic` model benchmark failed in both codecs. This pass will inspect those traces before choosing corrections.

| Area | Status | Evidence |
| --- | --- | --- |
| Endpoint and minimal model/tool calls | In progress | LocalRouter completion through the production agent passed with Qwen 27B and Llama 8B. |
| Podman container baseline | Passed | Existing acceptance: two fresh containers, Python/Alpine image, inputs, binary products, cache reuse, reports, cancellation, shutdown. |
| Model discovery, submission, observation | In progress | Gemma runtime discovery passed. Two model-controlled Podman runs produced correct receipts and reused cache. Shutdown observation is being tightened. |
| Model task creation and repair | Passed | Gemma native/MCP created and ran a task twice (166 seconds), observed shutdown, and reused cache. A separate failed-command repair passed (144 seconds). |
| Generation and runtime/LLM verification | Pending | |
| Cancellation, timeout, failure recovery | Passed, deterministic | Expanded lifecycle fixture passed on Podman and Docker. Model-driven cases remain. |
| MCP and ACP entry points | In progress | Real model MCP cases passed. Deterministic ACP cancellation and subsequent run passed on both engines. Full model ACP cases remain. |
| Docker parity | Passed, deterministic | Same expanded lifecycle cases as Podman. |
| Regression suite and release build | In progress | 670 compiler tests passed, 3 ignored after the third fixes. Go worker tests passed. Final release build remains. |

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
