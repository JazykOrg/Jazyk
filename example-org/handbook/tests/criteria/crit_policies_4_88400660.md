---
requirement: req:policies-4
statementHash: 872b0fd2afa025a5
kind: llm
name: crit_policies_4_88400660
---

# Criterion crit:policies-4 (boundary)

Statement: Every employee completes the safety training within 30 days of their start date.
Criterion: An employee who completes the safety training on day 30 after their start date meets the deadline; day 31 does not.

Quote: "Every employee completes the safety training within 30 days of their start date."

Implementing paths: (none yet — the handbook is not written)

Confirm steps:
1. Open the handbook's policies section on safety training (POL-105).
2. Find the statement of the deadline for completing the safety training.
3. Confirm it states the deadline is 30 days after the start date: an employee who completes the safety training on day 30 after their start date meets the deadline, and day 31 does not.

Verdict contract: pass when the handbook states that every employee completes the safety training within 30 days of their start date (day 30 meets the deadline, day 31 does not); fail otherwise.
