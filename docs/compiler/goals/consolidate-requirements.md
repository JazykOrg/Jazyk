# The consolidate-requirements goal

`consolidate-requirements` consolidates clusters of near-identical obligations.

- Class: GC. Optional at the soft limit and mandatory at the hard limit.
- Unit: one connected cluster from the mechanical duplicate-requirement signal.
- Limits live in `limits.rs`.
- Ready only when no open compile goal remains in the cluster's cone.
- Search and compare the obligations. Merge confirmed duplicates with
  `merge_requirements`, preserving all locations and redirecting absorbed ids.
- Reconcile criteria, edges, facets, and transitions before completion.
- Keep genuinely distinct obligations and record why they are distinct.
- Completion requires each candidate to be merged or explicitly distinguished.

Completion evidence lists `distinct: [{a, b, reason}]` for each surviving pair.
The store remembers distinct pair verdicts with statement hashes.
