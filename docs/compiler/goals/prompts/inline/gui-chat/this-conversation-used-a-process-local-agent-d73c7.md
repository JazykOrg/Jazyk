---
workflow: >-
  The local editor connects a chat to a selected coding agent and passes it the current
  user message and earlier conversation evidence. These texts explain that chat context
  or a restriction on reusing the selected agent after a restart.
used_by:
  - "bootstrap/src/gui/chat.rs"
---
This conversation used a process-local agent selection. Choose your agent and open a new conversation after restarting jazyk gui.