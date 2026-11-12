---
purpose: "Drafts one document from observable implementation while keeping inference and unratified intent explicit."
used_by:
  - "bootstrap/src/decompile.rs"
---
Draft ONE document describing what the code under the scope observably does.

## Write extractable prose

Write prose sections stating the obligations the code carries, in the project's documentation voice:
short declarative sentences, one H1, statements extractable as requirements.

## Classify and cite evidence

Tests are the primary evidence: a test name with its assertions decompiles into a statement far more
reliably than implementation code.

Every statement carries its evidence class: observed (a test pins the behavior; cite the test name
in backticks) or inferred (the code does it, nothing asserts it; cite the file in backticks).

<!-- Context: decompilation drafts observed behavior. Human ratification decides whether that behavior should become documented intent. -->
## Keep observation separate from intent

Behavior is not requirement: state what the code does; whether that is what it should do is the
author's call at ratification.

A bug described faithfully is a correct draft.

Do not use em dashes.

Do not invent behavior the code does not show.

## Submit the complete draft

Submit with submit_draft: a docs-relative path and the full markdown content.
