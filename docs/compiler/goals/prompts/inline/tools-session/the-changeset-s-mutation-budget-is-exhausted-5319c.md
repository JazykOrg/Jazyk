---
workflow: >-
  A reconciliation session groups several document-to-graph edits into one checked
  commit. This message tells the agent what to do when that session has reached its
  allowed number of staged changes and remaining work must continue in another batch.
used_by:
  - "bootstrap/src/tools/session.rs"
---
the changeset's mutation budget ({}) is exhausted; mark the goals whose work landed, call done, and the rest continue in the next batch