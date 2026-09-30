---
requirement: req:hiring-2
statementHash: 5aa9b33673058772
kind: llm
name: crit_hiring_16_d53c5e5f
---

# Criterion crit:hiring-16 (positive)

Statement: A hiring manager opens a requisition for a role on form HR-04.
Criterion: When a role needs filling, the hiring manager files a requisition on form `HR-04`, which is then filed under `People/Hiring/Requisitions/`.

Quote: "A hiring manager opens a requisition for a role on form HR-04, and
the recruiter runs the pipeline below for every application against that requisition.
Requisitions are filed under `People/Hiring/Requisitions/` on the shared drive."

Implementing paths: requisition.md

Confirm steps:
1. Open the handbook's requisition page.
2. Find the description of how a requisition is opened for a role.
3. Confirm it states that the hiring manager files a requisition on form `HR-04`, and that requisitions are filed under `People/Hiring/Requisitions/`.

Verdict contract: pass when the handbook describes the hiring manager opening a requisition for a role on form `HR-04` and filing it under `People/Hiring/Requisitions/`; fail otherwise.
