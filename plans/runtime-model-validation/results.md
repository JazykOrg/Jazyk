# Results

Work started on 2026-10-15. Raw local evidence is under this worktree's ignored
`.local/model-validation/` directory. The reusable model driver is
`bootstrap/tests/runtime_model_acceptance.py`.

Prior release evidence includes real Docker, Podman, Fargate, and deterministic ACP/MCP acceptance. Its `gen-basic` model benchmark failed in both codecs. This pass will inspect those traces before choosing corrections.

| Area | Status | Evidence |
| --- | --- | --- |
| Endpoint and minimal model/tool calls | In progress | LocalRouter completion through the production agent passed with Qwen 27B and Llama 8B. |
| Podman container baseline | Passed | Existing acceptance: two fresh containers, Python/Alpine image, inputs, binary products, cache reuse, reports, cancellation, shutdown. |
| Model discovery, submission, observation | Pending | |
| Model task creation and repair | Pending | |
| Generation and runtime/LLM verification | Pending | |
| Cancellation, timeout, failure recovery | Pending | |
| MCP and ACP entry points | Pending | |
| Docker parity | Pending | |
| Regression suite and release build | Pending | |

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
