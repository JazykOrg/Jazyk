---
requirement: req:policies-13
statementHash: 13b4706ebe5b16ec
kind: llm
name: crit_policies_8_cd2ab572
---

# Criterion crit:policies-8 (positive)

Statement: A manager answers a time-off request within 2 business days.
Criterion: A manager answers a time-off request on the 1st business day after the employee submits it.

Quote: "A manager answers a time-off
request within 2 business days."

Implementing paths: policies.md

Confirm steps:
1. Open policies.md, section "Time off (POL-101)".
2. Find the rule for how quickly a manager must answer a time-off request.
3. Confirm it states that a manager answers a time-off request on the 1st business day after the employee submits it (within the 2 business day limit).

Verdict contract: pass when the handbook states that a manager answers a time-off request on the 1st business day after the employee submits it, within the 2 business day limit; fail otherwise.
