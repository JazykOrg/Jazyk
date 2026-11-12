---
workflow: >-
  Before changing the project graph, Jazyk validates the fields the agent sent to a
  write tool. These messages explain how to represent the documented fact using the
  existing schema, rather than inventing unsupported structure.
used_by:
  - "bootstrap/src/tools/args.rs"
---
provenance dropped: the call carried {} and a derivation; a fact the documents state keeps its quote as its one provenance, so send `provenance` only for what no sentence states