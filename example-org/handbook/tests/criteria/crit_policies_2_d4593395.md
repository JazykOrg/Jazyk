---
requirement: req:policies-12
statementHash: 2c95833706b210c6
kind: llm
name: crit_policies_2_d4593395
---

# Criterion crit:policies-2 (boundary)

Statement: An employee requests time off on form HR-21 at least 5 business days ahead.
Criterion: An employee who requests time off on form `HR-21` 5 business days ahead meets the lead time; 4 business days does not.

Quote: "An employee requests
time off on form HR-21 at least 5 business days ahead."

Implementing paths: policies.md

Confirm steps:
1. Open policies.md, section "Time off (POL-101)".
2. Find the lead-time rule for requesting time off on form HR-21.
3. Confirm it states that a request made 5 business days ahead meets the lead time, and that a request made 4 business days ahead does not.

Verdict contract: pass when the handbook states that a time-off request on form HR-21 must be made at least 5 business days ahead, with 5 business days meeting the lead time and 4 business days not meeting it; fail otherwise.
