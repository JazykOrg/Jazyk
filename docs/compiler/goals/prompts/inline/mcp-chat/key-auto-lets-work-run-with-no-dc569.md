---
workflow: >-
  A conversational agent can ask Jazyk to update project documentation, settings, or
  related records. These messages explain the result or a boundary of those operations,
  including settings that require a human decision.
used_by:
  - "bootstrap/src/mcp/chat.rs"
---
`{key} = auto` lets work run with no release, so a human sets it: the GUI's workflow toggle, `/config {key} auto` typed in chat, or `[workflow]` in jazyk.toml. Tell the user; `manual` stays editable here.