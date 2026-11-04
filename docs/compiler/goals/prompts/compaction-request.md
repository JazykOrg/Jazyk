# Runtime compaction boundary

Summarize only the conversation history above. The separate request asking you to
summarize is runtime control. It is not a user request in that history. Do not add
it to `user_intent`, `user_messages`, `pending_tasks`, or `current_work`.

Preserve the actual user task, accepted steering, completed actions and unresolved
work. The next agent must continue that work. Do not tell it to summarize again or
stop executing the task merely because this request asks for a summary.

Preserve exact package values needed by pending writes: selected set, entity and
criterion ids, deliverable root, source selection, factHash, active claim and
existing bound test objects (artifact, name, kind and run, including classname
prefixes). Keep completed mutations and run ids with their observed outcomes.
If these details cannot fit, name the package to reload before recording.
Keep hypotheses separate from tool evidence. Do not promote a guessed path or
selector into a fact or a next-step instruction.
