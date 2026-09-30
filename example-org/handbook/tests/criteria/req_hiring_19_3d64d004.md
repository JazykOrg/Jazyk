---
requirement: req:hiring-19
statementHash: 3d64d004e3fdaf1c
kind: llm
name: req_hiring_19_3d64d004
---

# Criterion crit:hiring-10 (positive)

Statement: A screened application becomes rejected when the interview panel declines to run a loop.
Criterion: Given an application in `screened`, when the interview panel declines to run a loop, the observed application stage is `rejected`.

Quote: "- A screened application becomes rejected when the interview panel declines to run a\n  loop."

Implementing paths: application.md (Moving between stages section, screened→rejected)

Confirm steps:
1. Open the handbook's application page, the "Moving between stages" part.
2. Find the transition from `screened` to `rejected`.
3. Confirm it states that a screened application becomes rejected when the interview panel declines to run a loop.

Verdict contract: pass when the handbook states the screened→rejected transition on the panel declining to run a loop; fail otherwise.
