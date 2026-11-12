---
purpose: >-
  Instruction fragment: Record one acceptance criterion of a requirement: the observable
  evidence it is met, one condition and its outcome with concrete values, written from
  outside the system (never a function, a table, or a file). requirement is the owning
  requirement id. kind is positive, negative, boundary, or quality (with measure).
used_by:
  - "bootstrap/src/tools/catalog.rs"
context: "Prepared before the consumer inserts runtime values. Keep format slots aligned with that consumer."
---
Record one acceptance criterion of a requirement: the observable evidence it is met, one condition and its outcome with concrete values, written from outside the system (never a function, a table, or a file). requirement is the owning requirement id. kind is positive, negative, boundary, or quality (with measure). Exactly one of section plus quote (a criterion supported by existing prose, including the requirement's own description; quote the original verbatim even when the criterion uses observer wording) or provenance.derived {from, reasoning} (a criterion the documents do not state; from names the requirement). Refused when the statement restates the requirement (criterion-restates). Safe to retry: the same statement on the same requirement updates in place.