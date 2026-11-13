---
workflow: >-
  A skill-gated chat serving lists a core catalog and adds a workflow's tools when
  the agent reads that workflow's skill
  (docs/frontends/agent-workflows.md#skill-gated-tools). A tool result or error can
  still name a tool that is not listed yet.
purpose: >-
  Rides on such a result beside `lockedTools`, which maps each named but unlisted
  tool to the skills that list it, so the agent unlocks the tool instead of
  concluding it does not exist.
used_by:
  - "bootstrap/src/mcp/router.rs"
---
lockedTools names tools this result mentions that are not in your tool list yet. Read one of the listed workflow skills with read_jazyk_skill to add them, then call them.
