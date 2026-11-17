---
workflow: >-
  Descriptions of the agent tools a Jazyk agent run uses to start and manage
  sub-agents: other agent runs with their own conversations that work on a delegated
  task beside it and report back (docs/frontends/subagents.md#agent-tools).
used_by:
  - "bootstrap/src/harness/agents.rs"
---
Stop one of your sub-agents and its descendants. Its result arrives as cancelled.