---
requirement: req:hiring-16
statementHash: 4e416fa0df856967
kind: llm
name: req_hiring_16_4e416fa0
---

# Criterion crit:hiring-7 (positive)

Statement: An applied application becomes screened when the recruiter completes the phone screen with the candidate.
Criterion: Given an application in `applied`, when the recruiter completes the phone screen with the candidate, the observed application stage is `screened`.

Quote: "- An applied application becomes screened when the recruiter completes the phone screen\n  with the candidate."

Implementing paths: application.md (Moving between stages section, applied→screened)

Confirm steps:
1. Open the handbook's application page, the "Moving between stages" part.
2. Find the transition from `applied` to `screened`.
3. Confirm it states that an applied application becomes screened when the recruiter completes the phone screen with the candidate.

Verdict contract: pass when the handbook states the applied→screened transition on the recruiter completing the phone screen; fail otherwise.
