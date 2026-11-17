---
workflow: >-
  Descriptions of the agent tools a Jazyk agent run uses to start and manage
  sub-agents: other agent runs with their own conversations that work on a delegated
  task beside it and report back (docs/frontends/subagents.md#agent-tools).
used_by:
  - "bootstrap/src/harness/agents.rs"
---
Start a sub-agent: another agent with its own conversation that works on task beside you and reports back. Give a self-contained task naming what to do and what to report. Returns agentId. You keep working meanwhile; its final reply arrives as a message when you end your reply, and your turn waits for every sub-agent you started. With wait: true this call waits for the reply instead (up to wait_ms). write grants a write scope: none (default, read-only), all, or project-relative paths, at most your own. Use several sub-agents for independent parts of a larger job; do small work yourself.