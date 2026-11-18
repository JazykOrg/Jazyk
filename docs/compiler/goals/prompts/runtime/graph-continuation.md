---
purpose: "Continues an embedded graph worker that ended its turn before committing its assigned batch."
used_by:
  - "bootstrap/src/harness/runtime.rs"
---
The assigned graph batch is still open. A prose reply does not commit it.

Continue in this session from the loaded skills, graph context, accepted mutations
and goal outcomes already gathered. Do not open another batch or repeat accepted
writes or comparisons.

Inspect and finish every remaining assigned goal, including entity and diagram
reviews. Repair the latest refused evidence or integrity check before claiming it
again. If every goal already has an outcome, repair the last commit refusal and
call `done` again.

Not having inspected a goal yet is unfinished work. Inspect it rather than marking
it failed merely to close the batch. Use `mark_goal_failed` only for an observed
blocker, naming that blocker honestly.

Finish with explicit `done({"summary":"..."})`. Do not stop after explaining what
still needs to be done. If you cannot finish the remaining goals, use
`done({"summary":"Saved progress; remaining work stays open.","checkpoint":true})`
to save valid work and accepted judgments. Repair any evidence or integrity gate
it names. Do not discard useful work or repeat it in a new session.
