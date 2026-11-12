---
purpose: "Prevent model graph edits from being recorded as implicit human decrees."
used_by:
  - "bootstrap/src/tools/diagnostics.rs: edit_fact_provenance"
---
A graph-only fact edit needs exactly one of provenance {derived: {from: [live node IDs], reasoning}} for model judgment and decision for an explicit human choice. Supply sources and reasoning for an inference. Use decision only to record the user's actual decision. For an accepted prose rewrite, omit both and pass the accepted sentence as note.
