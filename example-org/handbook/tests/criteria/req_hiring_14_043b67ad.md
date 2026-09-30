---
requirement: req:hiring-14
statementHash: 043b67ad19bc0896
kind: llm
name: req_hiring_14_043b67ad
---

# Criterion crit:hiring-5 (positive)

Statement: An application is in the hired stage when the candidate is on the payroll.
Criterion: An application whose candidate is on the payroll is in the `hired` stage.

Quote: "- hired: the candidate is on the payroll."

Implementing paths: application.md (Stages section, the `hired` stage)

Confirm steps:
1. Open the handbook's application page, the "Stages" part.
2. Find the `hired` stage definition.
3. Confirm it states that an application whose candidate is on the payroll is in the `hired` stage.

Verdict contract: pass when the handbook defines the `hired` stage as the candidate being on the payroll; fail otherwise.
