---
purpose: >-
  Instruction fragment: `{key} = auto` lets work run with no release, so a human sets it:
  the GUI's workflow toggle, `/config {key} auto` typed in chat, or `[workflow]` in
  jazyk.toml.
used_by:
  - "bootstrap/src/mcp/chat.rs"
context: "Prepared before the consumer inserts runtime values. Keep format slots aligned with that consumer."
---
`{key} = auto` lets work run with no release, so a human sets it: the GUI's workflow toggle, `/config {key} auto` typed in chat, or `[workflow]` in jazyk.toml. Tell the user; `manual` stays editable here.