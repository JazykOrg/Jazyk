---
requirement: req:readme-15
statementHash: 117303fd487273f4
kind: llm
name: crit_readme_8_cd0b8869
---

# Criterion crit:readme-8 (negative)

Statement: A handbook page never mentions a form number or a file path.
Criterion: A handbook page contains no form number such as `HR-04` and no file path, since those live in the intranet.

Quote: "A handbook page never mentions a form number or
a file path; those live in the intranet, not in the handbook."

Implementing paths: README.md, departments.md, hiring.md, expenses.md, policies.md

Confirm steps:
1. Open each handbook page under `handbook/`.
2. Scan the page text for any form number (such as `HR-04`, `HR-21`, `EX-12`) and for any file path.
3. Confirm no handbook page mentions a form number or a file path; those live in the intranet, not in the handbook.

Verdict contract: pass when no handbook page mentions a form number such as `HR-04` or a file path; fail when any handbook page contains a form number or a file path.
