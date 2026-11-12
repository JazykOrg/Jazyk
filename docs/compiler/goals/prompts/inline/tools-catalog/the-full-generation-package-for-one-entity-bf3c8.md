---
workflow: >-
  These are descriptions of tools available to the model while it reads and edits the
  project graph. The graph stores concepts mentioned in the docs, requirements stated
  about them, supporting evidence, diagrams, and unresolved findings. The description
  tells the model what an operation means and how to supply its arguments.
used_by:
  - "bootstrap/src/tools/catalog.rs"
---
The full generation package for one entity: instructions (the generation contract), context pack, requirement groups (with suggested test names), change diff, the deliverable directory, factHash, the manifest of already generated files with what each holds, the medium, and the build. Write the deliverable files with your own tools, then record_generation with the manifest.