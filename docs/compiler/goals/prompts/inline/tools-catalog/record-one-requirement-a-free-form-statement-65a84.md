---
workflow: >-
  These are descriptions of tools available to the model while it reads and edits the
  project graph. The graph stores concepts mentioned in the docs, requirements stated
  about them, supporting evidence, diagrams, and unresolved findings. The description
  tells the model what an operation means and how to supply its arguments.
used_by:
  - "bootstrap/src/tools/catalog.rs"
---
Record one requirement: a free-form statement of one atomic obligation (specific, testable, entity-anchored). entities are the entity ids the statement is about. Exactly one of section plus quote (the verbatim source sentence copied from the section) or provenance.derived {from, reasoning} (a statement the documents do not state). Safe to retry: re-recording the same statement updates it in place. edges optionally tie two of the entities directionally (a acts on b) with a relationship type and cardinality. transition {subject, from, to, trigger?, guard?} says the subject (one of the entities) enters a state. facets are [{facet, reasoning, measure?}] with facet one of behavior, constraint, failure-mode, quality; measure only on quality.