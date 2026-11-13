---
workflow: >-
  The embedded agent runtime decides which proposed tool calls can run and how to resume
  interrupted work. These messages distinguish calls that ran from calls that were
  skipped, blocked, or left with an uncertain outcome.
purpose: >-
  A skill-gated catalog lists a workflow's tools only after its skill is read
  (docs/frontends/agent-workflows.md#skill-gated-tools). A call to an unlisted tool
  is rejected before dispatch; this tells the model how to get the tool listed.
used_by:
  - "bootstrap/src/harness/runtime.rs"
---
<!-- {name}: the tool the model called. Sent only when the catalog offers read_jazyk_skill. -->
Tool '{name}' is not available in this session's tool list. If a workflow skill uses it, read that skill with read_jazyk_skill and the skill's tools are added; list_jazyk_skills shows each skill's tools
