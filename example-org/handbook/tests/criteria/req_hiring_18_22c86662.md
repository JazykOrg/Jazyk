---
requirement: req:hiring-18
statementHash: 22c86662b5cac03e
kind: llm
name: req_hiring_18_22c86662
---

# Criterion crit:hiring-9 (positive)

Statement: A screened application becomes interviewed when the interview panel completes the loop with the candidate.
Criterion: Given an application in `screened`, when the interview panel completes the loop with the candidate, the observed application stage is `interviewed`.

Quote: "- A screened application becomes interviewed when the interview panel completes the loop\n  with the candidate."

Implementing paths: application.md (Moving between stages section, screened→interviewed)

Confirm steps:
1. Open the handbook's application page, the "Moving between stages" part.
2. Find the transition from `screened` to `interviewed`.
3. Confirm it states that a screened application becomes interviewed when the interview panel completes the loop with the candidate.

Verdict contract: pass when the handbook states the screened→interviewed transition on the panel completing the loop; fail otherwise.
