---
purpose: >-
  Instruction fragment: Settle several rejudge-pair goals of the batch in one call: one
  entry per goal with its verdict and a one or two sentence justification. consistent
  stages nothing and marks the goal done.
used_by:
  - "bootstrap/src/tools/catalog.rs"
context: "Prepared before the consumer inserts runtime values. Keep format slots aligned with that consumer."
---
Settle several rejudge-pair goals of the batch in one call: one entry per goal with its verdict and a one or two sentence justification. consistent stages nothing and marks the goal done; duplicate files a duplicate-requirement diagnostic on the pair (severity info, message optional) and marks; contradiction files a contradiction diagnostic with the entry's severity, message (both required), reasoning, and prompt, and marks, unless an open contradiction already names both sides (filed by an earlier batch, or by this session before the pair opened): then that one carries the verdict, nothing is filed, and the entry needs only the goal, the verdict, and the justification; an entry may name carried_by, an open contradiction on one side that states the same conflict, and that one carries it; resolved resolves the pair's open contradiction or duplicate-requirement diagnostic with the justification as the reason and marks. Each entry runs the gates of the single calls it stands for; a refused entry is reported with the rule and the repair, leaves nothing staged, and stops none of the others. Goals outside the batch or of another kind are refused. The same-document delete stays on delete_requirement plus mark_goal_done.