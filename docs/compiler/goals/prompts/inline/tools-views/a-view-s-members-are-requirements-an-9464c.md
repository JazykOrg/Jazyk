---
workflow: >-
  A diagram view records which facts from the graph a diagram includes. This message
  explains which kind of graph item belongs in the selected diagram, so the agent does
  not fill it with incompatible members.
used_by:
  - "bootstrap/src/tools/views.rs"
---
a {} view's members are requirements; an entity-matching query cannot drive it. Omit query and pick members