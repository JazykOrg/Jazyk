---
workflow: >-
  Descriptions of the agent tools a Jazyk agent run uses to start and manage
  sub-agents: other agent runs with their own conversations that work on a delegated
  task beside it and report back (docs/frontends/subagents.md#agent-tools).
used_by:
  - "bootstrap/src/harness/agents.rs"
---
Wait for your sub-agents to finish, up to wait_ms. ids defaults to your running sub-agents. mode all (default) returns when every named one has finished, any when the first has. Returns each one's row and, once finished, its result. A result returned here is not delivered again as a message.