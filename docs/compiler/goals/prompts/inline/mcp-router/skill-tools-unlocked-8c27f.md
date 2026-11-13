---
workflow: >-
  A skill-gated chat serving lists a core catalog and adds a workflow's tools when
  the agent reads that workflow's skill with read_jazyk_skill
  (docs/frontends/agent-workflows.md#skill-gated-tools).
purpose: >-
  Rides on the read_jazyk_skill reply beside `unlocked`, the skill's tools that the
  read added. Most clients refresh on tools/list_changed; one that does not keeps
  its old list, and the note names the server option that avoids gating.
used_by:
  - "bootstrap/src/mcp/router.rs"
---
The tools in unlocked are now in your tool list; the skill body explains when to call them. If they do not appear, your MCP client did not refresh its tool list: ask the user to start the jazyk MCP server with --all-tools.
