---
workflow: >-
  Refusal of spawn_agent when the requested write scope is wider than the caller's
  own (docs/frontends/subagents.md#write-scope). Nothing starts.
used_by:
  - "bootstrap/src/harness/agents.rs"
---
<!-- {scope}: the caller's own write scope. -->
Not started: a sub-agent's write scope can be at most your own ({scope}). Grant none, or paths inside your own scope.
