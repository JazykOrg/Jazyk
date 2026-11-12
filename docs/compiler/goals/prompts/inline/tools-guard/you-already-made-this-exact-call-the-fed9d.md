---
workflow: >-
  Jazyk watches tool calls for repeated requests that cannot make progress. These
  messages stop an agent from asking the same question indefinitely and explain how to
  act on the result it already has.
used_by:
  - "bootstrap/src/tools/guard.rs"
---
you already made this exact call; the answer is unchanged. Act on it.