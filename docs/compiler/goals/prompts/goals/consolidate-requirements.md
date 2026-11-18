---
purpose: "Consolidate duplicate obligation concepts without losing supporting locations."
used_by:
  - "bootstrap/src/goals/consolidate_requirements.rs"
---
This goal consolidates a cluster of requirements. Compare every requirement in this cluster. Merge confirmed duplicates with
merge_requirements. Preserve sources, criteria, edges, and redirects.
Reconcile conflicting transitions first. Keep distinct obligations and record
which observable behavior distinguishes them. Complete only when each candidate
is consolidated or distinguished with evidence.

Supply evidence `distinct: [{a, b, reason}]` for surviving distinct pairs.
