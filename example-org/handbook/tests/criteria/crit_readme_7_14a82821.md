---
requirement: req:readme-13
statementHash: 3e621e607e2101ed
kind: llm
name: crit_readme_7_14a82821
---

# Criterion crit:readme-7 (positive)

Statement: Each handbook page keeps the headings of its source document.
Criterion: A handbook page such as `departments.md` keeps the headings of its source document `departments.md`.

Quote: "Each handbook page keeps the headings of its source document."

Implementing paths: departments.md, hiring.md, expenses.md, policies.md, README.md

Confirm steps:
1. Open the handbook page `departments.md`.
2. Open its source document `departments.md`.
3. Confirm the headings present in `departments.md` are all present in `departments.md` (the page keeps the source document's headings).
4. Repeat for each handbook page and its source document.

Verdict contract: pass when each handbook page keeps the headings of its source document; fail otherwise.
