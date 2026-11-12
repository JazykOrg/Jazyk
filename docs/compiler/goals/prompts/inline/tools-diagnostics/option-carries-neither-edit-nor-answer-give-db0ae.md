---
workflow: >-
  When the documentation is ambiguous or contradictory, the agent can record a finding
  for a person to resolve. These messages validate the finding and any proposed answer
  or document edit, so the human is offered a real decision.
used_by:
  - "bootstrap/src/tools/diagnostics.rs"
---
option {} carries neither edit nor answer; give it one (`edit` to change the document, `answer` to reply in words); an option with neither is a filled-in blank, and a diagnostic that asks nothing of a human omits `prompt` entirely