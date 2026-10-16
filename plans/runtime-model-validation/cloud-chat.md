# Cloud chat investigation

Review chat generation discovery, file tools, release handling, provider deadlines,
and pause presentation. Reproduce changes in disposable fixtures.

## Fix and verification plan

1. Make generation discovery explain prerequisites, including pending binding. An
   empty generation list is not evidence that an artifact exists or has passed tests.
2. Give writable native and hosted chat the guarded project file tools. Keep browser
   limitations and read-only sessions explicit.
3. Provide a scoped release action for explicitly requested chat work. Keep manual
   workflow settings unchanged, and preserve human-only ratification and decisions.
   Explain the bind, generate, verify sequence without repeated release speculation.
4. Honor the saved model deadline for each HTTP request, including its fallbacks.
   Keep cancellation bounded. Distinguish provider failure from a user stop.
5. Show the actual resource-pause reason in the Cloud chat, including historical
   sessions, rather than a misleading cancellation marker.
6. Add focused compiler, hosted transport, and editor regressions. Exercise a small
   chat workflow with a real local model and Podman, then repeat the relevant Docker
   case. Finish release builds and review the diff before integration.

## Constraints

Keep production evidence in private storage. Use disposable projects for execution
and preserve existing projects and model policies.

## Real model follow-up

Exercise chat without `--serve-files`, starting with an unbound requirement behind
a manual generation release. Ask for implementation, tests, and execution. Require
binding and release tools, a falsifiable programmatic test, a passing isolated run,
then a deliberately failing run and an implementation-only repair. Run through ACP
and MCP with Podman first. Repeat a focused execution with Docker.

The repeated generation benchmark exposed an incorrect test choice: executable
output checks became LLM criteria files. Clarify that the suggested criteria path
does not select the test kind, and missing runner setup does not justify replacing
a behavioral check with a written verdict. Re-run both tool codecs.

## Completion recovery follow-up

A provider can exhaust its completion allowance and return `tool_calls` containing
empty arguments instead of reporting `length`. Diagnose reported cap exhaustion
with missing required fields. Return the missing fields and smaller-write guidance
without dispatching the invalid call. Permit a one-request boost twice per turn.
Retain the five consecutive tool error limit. Valid calls at the cap still run.

A malformed native batch must also pair every advertised call with a skipped
result before the repair prompt. The next request must contain valid tool history.

Four provider-loop regressions cover recovery, temporary boosts, valid capped
calls, paired history, and repeated failures. All 689 library tests passed, with
three ignored. The release build and real local and hosted retries follow.
