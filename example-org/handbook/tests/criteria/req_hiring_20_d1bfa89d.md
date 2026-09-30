---
requirement: req:hiring-20
statementHash: d1bfa89dfc34c4c3
kind: llm
name: req_hiring_20_d1bfa89d
---

# Criterion crit:hiring-11 (positive)

Statement: An interviewed application becomes offered when the hiring manager decides to hire, provided both of the candidate's references check out.
Criterion: Given an application in `interviewed`, when the hiring manager decides to hire and both candidate references check out, the observed application stage is `offered`.

Quote: "- An interviewed application becomes offered when the hiring manager decides to hire,\n  provided both of the candidate's references check out."

Implementing paths: application.md (Moving between stages section, interviewed→offered)

Confirm steps:
1. Open the handbook's application page, the "Moving between stages" part.
2. Find the transition from `interviewed` to `offered`.
3. Confirm it states that an interviewed application becomes offered when the hiring manager decides to hire, provided both of the candidate's references check out.

Verdict contract: pass when the handbook states the interviewed→offered transition gated on both references checking out; fail otherwise.

# Criterion crit:hiring-12 (negative)

Statement: An interviewed application becomes offered when the hiring manager decides to hire, provided both of the candidate's references check out.
Criterion: Given an application in `interviewed`, when the hiring manager decides to hire and fewer than both candidate references check out, the observed application stage is not `offered`.

Quote: "- An interviewed application becomes offered when the hiring manager decides to hire,\n  provided both of the candidate's references check out."

Implementing paths: application.md (Moving between stages section, interviewed→offered)

Confirm steps:
1. Open the handbook's application page, the "Moving between stages" part.
2. Find the transition from `interviewed` to `offered`.
3. Confirm the gate: when fewer than both references check out, the application does not become `offered`.

Verdict contract: pass when the handbook makes the `offered` transition conditional on both references checking out (so a missing reference blocks it); fail otherwise.
