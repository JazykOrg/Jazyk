---
requirement: req:hiring-17
statementHash: 2d81913ba4002881
kind: llm
name: req_hiring_17_2d81913b
---

# Criterion crit:hiring-8 (positive)

Statement: An applied application becomes rejected when the recruiter finds no match at the phone screen.
Criterion: Given an application in `applied`, when the recruiter records no match at the phone screen, the observed application stage is `rejected`.

Quote: "- An applied application becomes rejected when the recruiter finds no match at the phone\n  screen."

Implementing paths: application.md (Moving between stages section, applied→rejected)

Confirm steps:
1. Open the handbook's application page, the "Moving between stages" part.
2. Find the transition from `applied` to `rejected`.
3. Confirm it states that an applied application becomes rejected when the recruiter finds no match at the phone screen.

Verdict contract: pass when the handbook states the applied→rejected transition on the recruiter finding no match; fail otherwise.
