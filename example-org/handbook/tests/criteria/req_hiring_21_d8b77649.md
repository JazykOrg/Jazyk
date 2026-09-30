---
requirement: req:hiring-21
statementHash: d8b77649e02907db
kind: llm
name: req_hiring_21_d8b77649
---

# Criterion crit:hiring-13 (positive)

Statement: An interviewed application becomes rejected when the hiring manager decides not to hire.
Criterion: Given an application in `interviewed`, when the hiring manager decides not to hire, the observed application stage is `rejected`.

Quote: "- An interviewed application becomes rejected when the hiring manager decides not to\n  hire."

Implementing paths: application.md (Moving between stages section, interviewed→rejected)

Confirm steps:
1. Open the handbook's application page, the "Moving between stages" part.
2. Find the transition from `interviewed` to `rejected`.
3. Confirm it states that an interviewed application becomes rejected when the hiring manager decides not to hire.

Verdict contract: pass when the handbook states the interviewed→rejected transition on the hiring manager deciding not to hire; fail otherwise.
