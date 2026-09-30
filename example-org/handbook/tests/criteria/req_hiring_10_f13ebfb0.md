---
requirement: req:hiring-10
statementHash: f13ebfb0834f9b71
kind: llm
name: req_hiring_10_f13ebfb0
---

# Criterion crit:hiring-1 (positive)

Statement: An application is in the applied stage when the candidate submitted the application through the careers page at `ridgeline.example/careers`.
Criterion: An application submitted by a candidate through `ridgeline.example/careers` is in the `applied` stage.

Quote: "- applied: the candidate submitted the application through the careers page at\n   `ridgeline.example/careers`."

Implementing paths: application.md (Stages section, the `applied` stage)

Confirm steps:
1. Open the handbook's application page, the "Stages" part.
2. Find the `applied` stage definition.
3. Confirm it states that an application submitted by a candidate through `ridgeline.example/careers` is in the `applied` stage.

Verdict contract: pass when the handbook defines the `applied` stage as the candidate having submitted through `ridgeline.example/careers`; fail otherwise.
