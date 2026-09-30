---
requirement: req:policies-1
statementHash: bbb9129eea707469
kind: llm
name: crit_policies_7_4f2128b9
---

# Criterion crit:policies-7 (boundary)

Statement: A manager has at most 8 direct reports.
Criterion: A manager with 8 direct reports is within the limit; a manager with 9 direct reports exceeds it.

Quote: "A manager has at most 8 direct reports."

Implementing paths: (none yet — the handbook is not written)

Confirm steps:
1. Open the handbook's policies section on span of control (POL-103).
2. Find the statement of the maximum number of direct reports a manager may have.
3. Confirm it states the limit is 8 direct reports: a manager with 8 direct reports is within the limit, and a manager with 9 direct reports exceeds it.

Verdict contract: pass when the handbook states that a manager has at most 8 direct reports (8 within the limit, 9 exceeding it); fail otherwise.
