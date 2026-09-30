---
requirement: req:hiring-4
statementHash: 4976a54a99c765bb
kind: llm
name: crit_hiring_32_5bf4262a
---

# Criterion crit:hiring-32 (positive)

Statement: Requisitions are filed under `People/Hiring/Requisitions/` on the shared drive.
Criterion: A requisition opened on form HR-04 is filed under `People/Hiring/Requisitions/` on the shared drive.

Quote: "Requisitions are filed under `People/Hiring/Requisitions/` on the shared drive."

Implementing paths: requisition.md

Confirm steps:
1. Open the handbook's requisition page.
2. Find the description of where requisitions are filed.
3. Confirm it states that requisitions are filed under `People/Hiring/Requisitions/` on the shared drive.

Verdict contract: pass when the handbook states that requisitions are filed under `People/Hiring/Requisitions/` on the shared drive; fail otherwise.
