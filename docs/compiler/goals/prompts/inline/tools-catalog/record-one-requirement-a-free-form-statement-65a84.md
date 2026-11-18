---
workflow: >-
  These are descriptions of tools available to the model while it reads and edits the
  project graph. The graph stores concepts mentioned in the docs, requirements stated
  about them, supporting evidence, diagrams, and unresolved findings. The description
  tells the model what an operation means and how to supply its arguments.
used_by:
  - "bootstrap/src/tools/catalog.rs"
---
Record one requirement: a free-form statement of one atomic obligation (specific, testable, entity-anchored). entities are the entity ids the statement is about. Exactly one of section plus quote (a verbatim contiguous passage copied from the section, which may span consecutive sentences) or provenance.derived {from, reasoning} (a statement the documents do not state). Search existing requirements first. Safe to retry: the normalized statement within entity scopes matches the existing obligation and unions its sources. Use add_requirement_source to attach a differently worded source to a known id. edges optionally tie two of the entities directionally (a acts on b) with a relationship type and cardinality. transition {subject, from, to, trigger?, guard?} says the subject (one of the entities) enters a state.
