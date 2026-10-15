# Runtime model validation

Exercise the actual Jazyk prompts and tools through the owner's LocalRouter and local models. Use Podman first and repeat the core cases with Docker. Run one model request at a time. Keep each initial case short, inspect the transcript, and increase scope only after its prerequisites work.

## Action plan

1. Inventory the endpoint, models, engines, and existing traces. Check a bounded completion and one tool round trip before running a compiler session. Distinguish provider errors, codec errors, model mistakes, and harness defects.
2. Establish a real container baseline. Confirm the selected image, input files, working directory, command output, products, cache restoration, transient file disposal, and shutdown. Reuse the existing acceptance fixture where possible.
3. Start model sessions with one existing task. Ask the model to discover it, submit it, wait, inspect its log and retained changes, and report evidence. Then test creating and editing a task from prose, short inline commands, and retry after a failed command.
4. Cover generation and verification. Generate a small artifact, run its build or check, repair a failure, rerun, and record generation. Exercise runtime test bindings with passing, failing, skipped, absent, ambiguous, and stale cases. Exercise direct LLM verdicts separately.
5. Cover lifecycle and recovery. Cancel queued and executing work, expire a bounded command, handle an invalid image and invalid settings, reject stale file imports, inspect failed logs, and verify no container survives completion. Confirm logs cannot authorize unrelated actions.
6. Cover the entry points. Exercise the embedded worker, local MCP, and ACP proxy with the same task tools. Reuse deterministic transport tests for protocol boundaries and use real model transcripts for task selection and reasoning. Repeat the core lifecycle and file/cache cases on Docker.
7. Review and regress. For each failure, save a small reproduction, inspect exact prompts, arguments, and tool results, update the documented contract before behavior changes, add an appropriate regression, and rerun the failed case plus a fresh variation. Run relevant compiler tests and finish with a release build.

## Test discipline

- Start with tiny fixtures and bounded requests. Avoid a full compile until discovery, submission, observation, and completion work independently.
- Use the configured LocalRouter endpoint. Record model and codec explicitly. Probe Ollama availability without replacing the owner's configuration.
- Model assertions are not evidence. Check files, run records, actual image identity, exit status, report evidence, and engine state independently.
- Use disposable local projects without real publishing credentials. Use finite commands and stop test containers and the dedicated Podman VM afterward.
- Save raw traces in ignored local evidence storage. Commit reusable fixtures and a concise results ledger here. Do not commit secrets or generated state.
- Preserve unrelated work. Commit and push coherent changes from this worktree. Merge reviewed fixes into master when complete.

## Coverage ledger

The results file records measured outcomes, defects, reruns, and remaining limits. An unexecuted case stays unexecuted; a deterministic adapter test does not count as a successful model test.

See [results](./results.md).

## Reproduce

Run from the repository root with a built binary and a running local engine. The
environment file supplies the existing LocalRouter connection without copying its
credentials into the fixture. Use a new evidence directory for each attempt.

```sh
python3 bootstrap/tests/runtime_model_acceptance.py \
  --binary bootstrap/target/release/jazyk --env-file .env \
  --model gemma4:31b-mlx --engine podman --entry mcp --codec native \
  --case create --repeat --evidence .local/model-validation/create-1

python3 bootstrap/tests/runtime_lifecycle_acceptance.py \
  --binary bootstrap/target/release/jazyk --engine podman \
  --evidence .local/model-validation/lifecycle-1
```

Model cases include discovery, execution, definition creation, repair, cancellation,
timeout, and log inspection. `--entry acp` uses the full IDE proxy with no IDE terminal
or file capabilities. `--entry mcp` gives the production embedded agent the focused
runtime serving; `--toolsets generate,verify` selects the larger ledger serving.
`--codec text` exercises the text protocol, and `--engine docker` repeats engine parity.

For report-to-graph, mixed runner, and build-output checks:

```sh
python3 bootstrap/tests/runtime_verification_acceptance.py \
  --binary bootstrap/target/release/jazyk --engine podman \
  --evidence .local/model-validation/verification-1
```

For larger ACP prompts, `--call-timeout 240 --deadline 900` raises the model driver's
per-call and per-turn test budgets. It does not change the product defaults.
