---
requirement: req:hiring-11
statementHash: 1f4ce493be0d2eaa
kind: llm
name: req_hiring_11_1f4ce493
---

# Criterion crit:hiring-2 (positive)

Statement: An application is in the screened stage when the recruiter has finished the phone screen.
Criterion: An application whose recruiter has finished the phone screen is in the `screened` stage.

Quote: "- screened: the recruiter has finished the phone screen."

Implementing paths: application.md (Stages section, the `screened` stage)

Confirm steps:
1. Open the handbook's application page, the "Stages" part.
2. Find the `screened` stage definition.
3. Confirm it states that an application whose recruiter has finished the phone screen is in the `screened` stage.

Verdict contract: pass when the handbook defines the `screened` stage as the recruiter having finished the phone screen; fail otherwise.
