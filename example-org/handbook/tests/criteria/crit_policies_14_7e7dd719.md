---
requirement: req:policies-16
statementHash: 140fcf0fb36d5cfe
kind: llm
name: crit_policies_14_7e7dd719
---

# Criterion crit:policies-14 (positive)

Statement: A sales associate works on site every day.
Criterion: On every workday, a sales associate is present on site at the store.

Quote: "A sales associate works on site every day."

Implementing paths: sales-associate.md

Confirm steps:
1. Open the handbook's Sales Associate page.
2. Find how a sales associate's work location is described.
3. Confirm it states that a sales associate works on site every day.

Verdict contract: pass when the handbook states that a sales associate works on site every day; fail otherwise.
