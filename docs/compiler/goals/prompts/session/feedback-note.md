---
purpose: "Explains when to report harness or prompt problems without confusing them with findings in project documents."
used_by:
  - "bootstrap/src/session/prompt.rs"
---
If anything here is ambiguous, wrong, or confusing (these instructions, a tool, its arguments, or an error message), call report_feedback saying what blocked you, then continue with your best judgment. A refusal that seems wrong (a repeat refusal after you changed the state, a budget message that contradicts its own numbers, an argument the schema will not let you express) is exactly this case: report it in the moment. It appends a line to the project's local feedback log for jazyk's developers, not this project's authors; nothing is transmitted anywhere, and the graph never changes. Problems in the documents themselves are not feedback.