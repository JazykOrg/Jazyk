---
workflow: >-
  An external agent can ask which project files have changed since its last check. This
  tool description explains the returned change list and how document edits differ from
  edits to files the project generates.
used_by:
  - "bootstrap/src/mcp/watch.rs"
---
Project files edited since the cursor `since` (since this serving started when absent), by any tool: your own edit tools, an editor, git. Edit files with your own tools; this shows their effect before any build. A document row lists the open compile goals its edit derived (dirtied sections); a source row names the entity owning the file. Pass the reply's cursor as the next since. Read only: never compiles.