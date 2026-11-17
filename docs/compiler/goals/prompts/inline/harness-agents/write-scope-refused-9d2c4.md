---
workflow: >-
  Refusals returned as tool results to a sub-agent whose call falls outside the write
  scope its parent granted (docs/frontends/subagents.md#write-scope). The call is not
  dispatched. The model reads the refusal and continues without the change.
used_by:
  - "bootstrap/src/harness/agents.rs"
---
<!-- {tool}: the refused tool name. {scope}: the granted scope, as none, all, or the path list. -->
`{tool}` is outside this sub-agent's write scope ({scope}). Do not retry it. Finish the task with the tools you have and name the change you could not make in your final reply.
