---
workflow: >-
  When the documentation is ambiguous or contradictory, the agent can record a finding
  for a person to resolve. These messages validate the finding and any proposed answer
  or document edit, so the human is offered a real decision.
used_by:
  - "bootstrap/src/tools/diagnostics.rs"
---
option {} carries both edit and answer; keep one: `edit` when the option changes the document (the edit is its answer), `answer` when it replies in words; drop the other and send the call again