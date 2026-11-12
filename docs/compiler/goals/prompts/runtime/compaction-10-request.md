---
purpose: "Requests a bounded continuation summary when the runtime compacts conversation history."
used_by:
  - "bootstrap/src/harness/runtime.rs"
---
# Runtime compaction boundary

Create a concise progress handoff for the active Jazyk session. The request to
summarize is runtime control. It is not a user request in that history. The next agent must continue
the actual task and accepted steering.

Conversation history to summarize:
{{ messages }}

Return one JSON object with these fields when relevant:
- `user_intent`: the actual task and accepted changes, as short strings.
- `files`: objects with `path` and a short `summary` of completed changes.
- `errors_and_fixes`: observed failures, exact failing selectors or error codes,
  whether each was fixed, and the evidence for that conclusion.
- `pending_tasks`: unfinished assigned work and observed blockers.
- `current_work`: the active entity, current files, and last completed operation.
- `next_step`: the next concrete action that advances the actual task.

Keep the handoff under 6000 characters. Prefer a few precise bullets over a
chronological transcript. Do not spend the available output budget restating
history. Do not reproduce source files, long tool outputs, full prompts, the
opening contract, or package payloads. The opening contract, latest successful
authoritative package exchanges for unfinished targets and latest tool exchange are retained separately
with their exact values. Source files and run logs remain available through tools.
Name a path or run id and the relevant finding instead of copying its contents.

Preserve exact identifiers needed for unfinished work: selected set, active entity,
criterion ids, current run ids, completed records and unresolved gates. Distinguish
observed tool results from plans and hypotheses. A failed or skipped write did
not change the file. A passing test proves only the exact checks it executed.
Do not turn an invented locator, guessed path, or intended implementation into a
fact. If a necessary value is absent from the retained package, name the package
to reload. Keep any partial file edit and the remaining work explicit.

Do not add this summarization request to user intent or pending work. Do not tell
the next agent to summarize again, stop the active task, or repeat completed writes
and tests. Do not add new product decisions.
