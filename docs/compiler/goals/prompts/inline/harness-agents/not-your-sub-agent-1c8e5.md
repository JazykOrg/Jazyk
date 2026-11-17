---
workflow: >-
  Refusal of an agent tool call that names an id outside the caller's descendants
  (docs/frontends/subagents.md#agent-tools). An agent reaches only the sub-agents it
  started and theirs.
used_by:
  - "bootstrap/src/harness/agents.rs"
---
<!-- {id}: the id the call named. -->
`{id}` is not one of your sub-agents. Call agent_status to list them.
