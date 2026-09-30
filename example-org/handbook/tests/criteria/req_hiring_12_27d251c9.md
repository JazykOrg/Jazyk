---
requirement: req:hiring-12
statementHash: 27d251c9299b2893
kind: llm
name: req_hiring_12_27d251c9
---

# Criterion crit:hiring-3 (positive)

Statement: An application is in the interviewed stage when the interview panel has finished the loop.
Criterion: An application whose interview panel has finished the loop is in the `interviewed` stage.

Quote: "- interviewed: the interview panel has finished the loop."

Implementing paths: application.md (Stages section, the `interviewed` stage)

Confirm steps:
1. Open the handbook's application page, the "Stages" part.
2. Find the `interviewed` stage definition.
3. Confirm it states that an application whose interview panel has finished the loop is in the `interviewed` stage.

Verdict contract: pass when the handbook defines the `interviewed` stage as the interview panel having finished the loop; fail otherwise.
