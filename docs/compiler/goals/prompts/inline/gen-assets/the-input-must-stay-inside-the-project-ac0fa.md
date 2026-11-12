---
workflow: >-
  A requirement may refer to an existing input file, such as an image the generated page
  must use. Jazyk checks that the input can be read safely and identifies its exact
  bytes before giving it to the coding agent.
used_by:
  - "bootstrap/src/gen/assets.rs"
---
The input must stay inside the project and outside private paths and nested projects.