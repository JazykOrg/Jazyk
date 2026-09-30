---
requirement: req:readme-3
statementHash: fdcaaa6c150c5e8c
kind: llm
name: crit_readme_6_8940a68c
---

# Criterion crit:readme-6 (positive)

Statement: The documents are the source of the Ridgeline employee handbook.
Criterion: The Ridgeline employee handbook is built from these documents as its source.

Quote: "They are the source of the Ridgeline
employee handbook."

Implementing paths: README.md, departments.md, hiring.md, expenses.md, policies.md

Confirm steps:
1. Open the handbook deliverable (the handbook/ folder of markdown pages).
2. Confirm the handbook is built from the source documents (README.md, departments.md, hiring.md, expenses.md, policies.md) as its source: its content is drawn from those documents, not invented independently.
3. Confirm the handbook reflects the source documents' content (the company, its departments, hiring, expenses, and policies).

Verdict contract: pass when the handbook is built from the source documents as its source; fail otherwise.
