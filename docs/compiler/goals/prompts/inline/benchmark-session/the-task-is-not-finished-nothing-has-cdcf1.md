---
workflow: >-
  During a model benchmark, the agent must make real changes through tools and commit
  its work. This reminder is sent when the model answers as if it has finished but the
  benchmark session has committed nothing.
used_by:
  - "bootstrap/src/benchmark/session.rs"
---
The task is not finished: nothing has committed. Continue with the tool calls the instructions name, then finish with done.