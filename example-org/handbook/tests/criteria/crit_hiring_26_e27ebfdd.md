---
requirement: req:hiring-22
statementHash: e515c65d76567669
kind: llm
name: crit_hiring_26_e27ebfdd
---

# Criterion crit:hiring-26 (positive)

Statement: The recruiter records every stage change with its date and the person who made it.
Criterion: When an application moves from `applied` to `screened`, the recruiter records the change with its date and the person who made it.

Quote: "The recruiter records every stage change with its date and the person who made it."

Implementing paths: application.md (Moving between stages section, recording)

Confirm steps:
1. Open the handbook's application page, the "Moving between stages" part.
2. Find the description of how the recruiter records a stage change.
3. Confirm it states that when an application moves from `applied` to `screened`, the recruiter records the change with its date and the person who made it.

Verdict contract: pass when the handbook states that the recruiter records every stage change (including the move from `applied` to `screened`) with its date and the person who made it; fail otherwise.
