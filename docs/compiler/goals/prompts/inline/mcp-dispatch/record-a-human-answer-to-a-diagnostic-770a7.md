---
purpose: >-
  Instruction fragment: Record a human answer to a diagnostic's prompt, relayed from
  conversation.
used_by:
  - "bootstrap/src/mcp/dispatch.rs"
context: "Prepared before the consumer inserts runtime values. Keep format slots aligned with that consumer."
---
Record a human answer to a diagnostic's prompt, relayed from conversation. Pass option (index) for a chosen option or text for a freeform reply. An edit option applies as a dual write and resolves the finding before this returns; any other answer is recorded and the reply hands the handling contract back to you: act on it with the tools, then resolve_diagnostic.