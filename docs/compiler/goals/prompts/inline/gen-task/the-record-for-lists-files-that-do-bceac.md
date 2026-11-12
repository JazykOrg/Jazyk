---
workflow: >-
  A generation task gives the coding agent the requirements and existing files for one
  component. Jazyk then checks its saved completion report and test evidence before
  considering that task finished. These messages explain missing work or guide the
  component package.
used_by:
  - "bootstrap/src/gen/task.rs"
---
the record for `{}` lists files that do not exist under the deliverable ({}); write them, then record_generation again with every file the manifest names