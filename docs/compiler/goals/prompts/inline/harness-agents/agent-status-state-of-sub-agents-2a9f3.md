---
workflow: >-
  Descriptions of the agent tools a Jazyk agent run uses to start and manage
  sub-agents: other agent runs with their own conversations that work on a delegated
  task beside it and report back (docs/frontends/subagents.md#agent-tools).
used_by:
  - "bootstrap/src/harness/agents.rs"
---
The state, title, depth and write scope of your sub-agents, or of the named ids. Results are not included: they arrive as messages, or from wait_agents.