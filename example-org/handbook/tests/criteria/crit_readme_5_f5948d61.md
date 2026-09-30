---
requirement: req:readme-14
statementHash: 0febec973f79de2d
kind: llm
name: crit_readme_5_f5948d61
---

# Criterion crit:readme-5 (positive)

Statement: The employee handbook opens with a table of contents that links every page.
Criterion: The employee handbook opens with a table of contents that links every handbook page.

Quote: "The handbook opens with a
table of contents that links every page."

Implementing paths: README.md

Confirm steps:
1. Open the handbook's opening page `README.md`.
2. Confirm the handbook opens with a table of contents.
3. Confirm the table of contents links every handbook page (departments.md, hiring.md, expenses.md, policies.md).

Verdict contract: pass when the handbook opens with a table of contents that links every handbook page; fail otherwise.
