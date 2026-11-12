---
purpose: >-
  Instruction fragment: Build one level: stage a derived grouping entity from members
  (provenance derived from exactly them, reasoning why the domain would recognize them as
  one thing) and reparent every member under it, as one changeset.
used_by:
  - "bootstrap/src/tools/catalog.rs"
context: "Prepared before the consumer inserts runtime values. Keep format slots aligned with that consumer."
---
Build one level: stage a derived grouping entity from members (provenance derived from exactly them, reasoning why the domain would recognize them as one thing) and reparent every member under it, as one changeset. The grouping takes the members' shared current parent (none at the scope root) and their scope. At least two members; every member resolves; all members share one current parent (a grouping never crosses levels); the name passes the near-duplicate gate, so a lookalike of an existing area reuses that entity instead; definition is one sentence stating the grouping's responsibility. stereotype is from the existing vocabulary (system, component, module) or absent; there is no grouping stereotype.