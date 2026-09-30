---
requirement: req:policies-13
statementHash: 13b4706ebe5b16ec
kind: llm
name: crit_policies_9_901db557
---

# Criterion crit:policies-9 (boundary)

Statement: A manager answers a time-off request within 2 business days.
Criterion: A time-off request answered on the 2nd business day is within the limit; one answered on the 3rd business day is late.

Quote: "A manager answers a time-off
request within 2 business days."

Implementing paths: policies.md

Confirm steps:
1. Open policies.md, section "Time off (POL-101)".
2. Find the rule for how quickly a manager must answer a time-off request.
3. Confirm it states that a request answered on the 2nd business day is within the limit and one answered on the 3rd business day is late.

Verdict contract: pass when the handbook states that a time-off request answered on the 2nd business day is within the limit and one answered on the 3rd business day is late; fail otherwise.
