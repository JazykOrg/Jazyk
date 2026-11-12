---
workflow: >-
  Jazyk watches tool calls for repeated requests that cannot make progress. These
  messages stop an agent from asking the same question indefinitely and explain how to
  act on the result it already has.
used_by:
  - "bootstrap/src/tools/guard.rs"
---
this is call {} to `{}` with identical arguments; the answer has not changed. Act on the answer you already have, or finish with done. If this refusal seems wrong, say so with report_feedback.