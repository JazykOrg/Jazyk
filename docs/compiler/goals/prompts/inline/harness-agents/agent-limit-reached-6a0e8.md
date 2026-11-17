---
workflow: >-
  Refusals of spawn_agent and message_agent when a sub-agent limit is reached
  (docs/frontends/subagents.md#limits). Nothing starts. The model either waits for a
  running sub-agent or does the work itself.
used_by:
  - "bootstrap/src/harness/agents.rs"
---
<!-- {limit}: the registry constant's name. {value}: its value. -->
Not started: {limit} ({value}) is reached. Wait for a running sub-agent with wait_agents, or do this part of the work yourself.
