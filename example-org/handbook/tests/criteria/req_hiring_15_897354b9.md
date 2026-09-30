---
requirement: req:hiring-15
statementHash: 897354b97fca81af
kind: llm
name: req_hiring_15_897354b9
---

# Criterion crit:hiring-6 (positive)

Statement: An application is in the rejected stage when the application is closed and the candidate has been told.
Criterion: An application that is closed after the candidate has been told is in the `rejected` stage.

Quote: "- rejected: the application is closed and the candidate has been told."

Implementing paths: application.md (Stages section, the `rejected` stage)

Confirm steps:
1. Open the handbook's application page, the "Stages" part.
2. Find the `rejected` stage definition.
3. Confirm it states that an application that is closed after the candidate has been told is in the `rejected` stage.

Verdict contract: pass when the handbook defines the `rejected` stage as the application being closed and the candidate having been told; fail otherwise.
