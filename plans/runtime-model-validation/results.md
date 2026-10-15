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
| Model task creation and repair | Pending | |
| Generation and runtime/LLM verification | Pending | |
| Cancellation, timeout, failure recovery | Passed, deterministic | Expanded lifecycle fixture passed on Podman and Docker. Model-driven cases remain. |
| MCP and ACP entry points | Pending | |
| Docker parity | Passed, deterministic | Same expanded lifecycle cases as Podman. |
| Regression suite and release build | In progress | 668 compiler tests passed, 3 ignored after the second fixes. Final release build remains. |

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
