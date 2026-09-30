---
requirement: req:hiring-9
statementHash: 1ac0cee86dde0222
kind: llm
name: req_hiring_9_1ac0cee8
---

# Criterion crit:hiring-14 (positive)

Statement: An application is in exactly one stage at a time.
Criterion: An application shown in `applied` is not simultaneously shown in `screened`, `interviewed`, `offered`, `hired`, or `rejected`.

Quote: "An application is in exactly one stage at a time. The stages are:"

Implementing paths: application.md (Stages section, single-stage invariant)

Confirm steps:
1. Open the handbook's application page, the "Stages" part.
2. Find the statement that an application is in exactly one stage at a time.
3. Confirm the stages are presented as mutually exclusive: an application shown in `applied` is not simultaneously shown in `screened`, `interviewed`, `offered`, `hired`, or `rejected`.

Verdict contract: pass when the handbook states the single-stage invariant and presents the stages as mutually exclusive; fail otherwise.
