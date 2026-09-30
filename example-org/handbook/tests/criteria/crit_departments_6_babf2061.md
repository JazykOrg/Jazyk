---
requirement: req:departments-3
statementHash: 3b305f8d99f5f4d1
kind: llm
name: crit_departments_6_babf2061
---

# Criterion crit:departments-6 (positive)

Statement: Every employee has exactly one manager.
Criterion: An employee has exactly one manager, not zero and not two.

Quote: "Every employee belongs to exactly one department. Every employee has exactly one manager."

Implementing paths: (none yet — the handbook is not written)

Confirm steps:
1. Open the handbook's Employees and Managers section.
2. Find the description of the manager relationship for employees.
3. Confirm it states that every employee has exactly one manager, not zero and not two.

Verdict contract: pass when the handbook states that every employee has exactly one manager (not zero, not two); fail otherwise.
